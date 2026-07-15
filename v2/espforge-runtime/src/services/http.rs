//! `Http` software-service component (ADR-012). A thin ergonomic wrapper over
//! `edge_http` that hides the buffer + read-loop boilerplate behind
//! `async get/post -> Result<String>`. The app never names `edge_http`,
//! `Connection`, or `Stack`.
//!
//! Lives under `services/` (ADR-013): hardware-backed components live in
//! `../components/`, software-service components here. Re-exported into
//! `components` so the generated `ctx.components.http` accessor is uniform.
//!
//! TLS/HTTPS is out of scope for the first implementation (ADR-012).
//!
//! API note (edge-http 0.8 / edge-nal-embassy 0.9): `Connection` takes a
//! `T: TcpConnect`, supplied here by `edge_nal_embassy::Tcp`, which itself wraps
//! the `embassy_net::Stack` plus a TCP socket-buffer pool
//! (`TcpBuffers`). DNS is resolved separately via `edge_nal_embassy::Dns`.

use core::net::SocketAddr;
use core::net::IpAddr;

use edge_http::io::client::Connection;
use edge_http::Method;
use edge_nal::{AddrType, Dns as _};
use edge_nal_embassy::{Dns as EmbassyDns, Tcp, TcpBuffers, TcpError};

/// Reusable scratch buffer size for request/response headers + body.
const BUF_LEN: usize = 8192;
/// TCP socket buffers: 1 concurrent socket, 4 KiB each. The stack is built with
/// `StackResources<3>` (emit/rust.rs), so N=1 is safely within the socket set.
type TcpBuf = TcpBuffers<1, 4096, 4096>;

/// Handle to the HTTP service. Owns the TCP socket-buffer pool and a reusable
/// scratch buffer; holds a shared reference to the singleton network `Stack`
/// (built by the emitter in `main.rs`, ADR-012).
pub struct Http {
    stack: &'static embassy_net::Stack<'static>,
    buffers: TcpBuf,
    buf: core::cell::RefCell<[u8; BUF_LEN]>,
}

impl Http {
    /// `stack` is the emitter-built `NET_STACK` singleton.
    pub fn new(stack: &'static embassy_net::Stack<'static>) -> Self {
        Http {
            stack,
            buffers: TcpBuf::new(),
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

        // DNS lookup via `edge_nal_embassy::Dns` (ADR-012 bridge).
        let resolver = EmbassyDns::new(*self.stack);
        let ip: IpAddr = resolver
            .get_host_by_name(host, AddrType::IPv4)
            .await
            .map_err(HttpError::Dns)?;
        let addr = SocketAddr::new(ip, 80);

        // `Tcp` is `Copy` and references the shared `Stack` + buffer pool.
        let tcp = Tcp::new(*self.stack, &self.buffers);

        let mut conn: Connection<_, 16> = Connection::new(&mut *buf, &tcp, addr);

        conn.initiate_request(true, method, path, &[("Host", host)])
            .await
            .map_err(HttpError::Http)?;

        if !body.is_empty() {
            use embedded_io_async::Write;
            conn.write_all(body)
                .await
                .map_err(|e| HttpError::Io(e.kind()))?;
        }

        conn.initiate_response().await.map_err(HttpError::Http)?;

        // Drain the response body into a heap-allocated `String` (alloc is
        // guaranteed by the `has_alloc` flag, ADR-012).
        use embedded_io_async::Read;
        let mut out = String::new();
        let mut chunk = [0u8; 1024];
        loop {
            let len = conn
                .read(&mut chunk)
                .await
                .map_err(|e| HttpError::Io(e.kind()))?;
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
    Dns(edge_nal_embassy::DnsError),
    Http(edge_http::io::Error<edge_nal_embassy::TcpError>),
    Io(embedded_io_async::ErrorKind),
}
