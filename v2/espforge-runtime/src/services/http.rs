//! `Http` software-service component (ADR-012). A thin ergonomic wrapper over
//! `edge_http` that hides the buffer + read-loop boilerplate behind
//! `async get/post -> Result<String>`. The app never names `edge_http`,
//! `Connection`, or `Stack`.
//!
//! Lives under `services/` (ADR-013): hardware-backed components live in
//! `../components/`, software-service components here. Re-exported into
//! `crate::components` so the generated `ctx.components.http` accessor is
//! uniform with hardware components.

use core::net::SocketAddr;

use edge_http::io::client::Connection;
use edge_http::Method;
use edge_nal::TcpConnect;

/// Reusable scratch buffer size for request/response bodies.
const BUF_LEN: usize = 8192;

/// Handle to the HTTP service. Owns a reusable scratch buffer; holds a shared
/// reference to the singleton network `Stack` (built by the emitter in
/// `main.rs`, ADR-012).
pub struct Http {
    stack: &'static embassy_net::Stack<'static>,
    buf: core::cell::RefCell<[u8; BUF_LEN]>,
}

impl Http {
    /// `stack` is the emitter-built `NET_STACK` singleton.
    pub fn new(stack: &'static embassy_net::Stack<'static>) -> Self {
        Http {
            stack,
            buf: core::cell::RefCell::new([0u8; BUF_LEN]),
        }
    }

    /// Perform an HTTP GET and return the full response body as a `String`.
    pub async fn get(&self, url: &str) -> Result<String, HttpError> {
        self.request(Method::Get, url, &[]).await
    }

    /// Perform an HTTP POST with a body and return the full response body as a
    /// `String`.
    pub async fn post(&self, url: &str, body: &[u8]) -> Result<String, HttpError> {
        self.request(Method::Post, url, body).await
    }

    async fn request(
        &self,
        method: Method,
        url: &str,
        body: &[u8],
    ) -> Result<String, HttpError> {
        let mut buf = self.buf.borrow_mut();
        let (host, path) = split_url(url)?;

        // DNS lookup via the `edge_nal::Dns` impl `edge_nal_embassy` provides for
        // `&embassy_net::Stack`.
        let ip = self
            .stack
            .get_host_by_name(host, edge_nal::AddrType::IPv4)
            .await
            .map_err(|e| HttpError::Dns(e))?;
        let addr = SocketAddr::new(ip, 80);

        let mut conn: Connection<_> = Connection::new(&mut *buf, self.stack, addr);

        conn.initiate_request(true, method, path, &[("Host", host)])
            .await
            .map_err(|e| HttpError::Http(e))?;

        if !body.is_empty() {
            // Send the request body if present.
            use embedded_io_async::Write;
            conn.write_all(body).await.map_err(|e| HttpError::Io(e))?;
        }

        conn.initiate_response()
            .await
            .map_err(|e| HttpError::Http(e))?;

        // Drain the response body into a heap-allocated `String` (alloc is
        // guaranteed by the `has_alloc` flag, ADR-012).
        use embedded_io_async::Read;
        let mut out = String::new();
        let mut chunk = [0u8; 1024];
        loop {
            let len = conn.read(&mut chunk).await.map_err(|e| HttpError::Io(e))?;
            if len == 0 {
                break;
            }
            out.push_str(core::str::from_utf8(&chunk[..len]).map_err(|_| HttpError::Utf8)?);
        }
        Ok(out)
    }
}

fn split_url(url: &str) -> Result<(&str, &str), HttpError> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or(HttpError::BadUrl)?;
    match without_scheme.find('/') {
        Some(i) => Ok((&without_scheme[..i], &without_scheme[i..])),
        None => Ok((without_scheme, "/")),
    }
}

/// Errors surfaced to the app (the underlying `edge_http`/`embedded-io` error
/// types are not leaked, ADR-012).
#[derive(Debug)]
pub enum HttpError {
    BadUrl,
    Utf8,
    Dns(edge_nal::Error),
    Http(edge_http::io::Error),
    Io(embedded_io_async::ErrorKind),
}
