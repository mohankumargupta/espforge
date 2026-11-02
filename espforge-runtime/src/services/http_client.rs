//! `HttpClient`: an outgoing HTTP/1.1 client over an `embassy-net` stack.
//!
//! Built on [`edge-http`], which frames the request and parses the response,
//! and [`edge_nal_embassy`], which bridges the stack to edge-http's socket
//! traits. The whole exchange runs inside one timeout. The connection buffer
//! is a single static, so an app calls `get`/`post` one at a time — which is
//! what the generated `app::forever` loop does anyway.

use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use edge_http::Method;
use edge_http::io::client::Connection;
use edge_nal::{AddrType, Dns, WithTimeoutError, with_timeout};
use edge_nal_embassy::{Dns as EmbassyDns, Tcp, TcpBuffers};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_io_async::{Read, Write};

/// The largest connection buffer the client will use. The declared
/// `rx_buffer_size` sizes it below this; the cap keeps the static bounded.
const CONN_BUFFER_CAP: usize = 4096;

/// The connection buffer and the TCP socket-buffer pool, shared by every
/// client: only one client connection is in flight at a time, and the pool is
/// not `Sync` on its own, so both live behind one async-aware `Mutex` that the
/// exchange holds across its awaits.
struct Shared {
    connection: [u8; CONN_BUFFER_CAP],
    pool: TcpBuffers<1, 1024, 2048>,
}

static SHARED: Mutex<CriticalSectionRawMutex, Shared> = Mutex::new(Shared {
    connection: [0; CONN_BUFFER_CAP],
    pool: TcpBuffers::new(),
});

/// What `<http_client>` declared on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpClientConfig {
    /// Bytes of connection buffer: the request and the response headers/body
    /// are framed through this. Capped at `CONN_BUFFER_CAP`.
    pub rx_buffer_size: usize,
    /// Largest request body the client will send.
    pub tx_buffer_size: usize,
    /// Connection, request and response timeout, in milliseconds.
    pub timeout_ms: u64,
}

/// Why an HTTP request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    /// The URL is not `http://host[:port]/path`.
    BadUrl,
    /// DNS could not resolve the host.
    Dns,
    /// edge-http refused to frame the request, or the socket failed.
    Request,
    /// edge-http refused to parse the response.
    Response,
    /// The socket failed while reading the response.
    Read,
    /// The socket failed while writing the request body.
    Write,
    /// The exchange did not finish inside `timeout_ms`.
    Timeout,
    /// The request body is larger than `tx_buffer_size`.
    BodyTooLarge,
}

/// A handle to the board's HTTP client. `Copy`, so it can sit on `Services`.
#[derive(Clone, Copy)]
pub struct HttpClient {
    stack: Stack<'static>,
    config: HttpClientConfig,
}

impl HttpClient {
    pub fn new(stack: Stack<'static>, config: HttpClientConfig) -> Self {
        Self { stack, config }
    }

    /// GET `url`, writing the response body into `rx_buf`. Returns the bytes
    /// written.
    pub async fn get<'a>(&self, url: &str, rx_buf: &'a mut [u8]) -> Result<&'a [u8], HttpError> {
        self.exchange(Method::Get, url, None, rx_buf).await
    }

    /// POST `payload` to `url` with `content_type`, writing the response body
    /// into `rx_buf`. Returns the bytes written.
    pub async fn post<'a>(
        &self,
        url: &str,
        content_type: &str,
        payload: &[u8],
        rx_buf: &'a mut [u8],
    ) -> Result<&'a [u8], HttpError> {
        if payload.len() > self.config.tx_buffer_size {
            return Err(HttpError::BodyTooLarge);
        }
        self.exchange(Method::Post, url, Some((content_type, payload)), rx_buf)
            .await
    }

    async fn exchange<'a>(
        &self,
        method: Method,
        url: &str,
        body: Option<(&str, &[u8])>,
        rx_buf: &'a mut [u8],
    ) -> Result<&'a [u8], HttpError> {
        let (host, port, path) = parse_url(url, 80)?;
        let addr = self.resolve(host, port).await?;

        let conn_len = self.config.rx_buffer_size.clamp(1, CONN_BUFFER_CAP);
        let mut guard = SHARED.lock().await;
        let Shared { connection, pool } = &mut *guard;
        let buf: &mut [u8] = &mut connection[..conn_len];
        let tcp = Tcp::new(self.stack, pool);
        let timeout = self.config.timeout_ms.min(u32::MAX as u64) as u32;

        match with_timeout(
            timeout,
            request(&tcp, addr, method, host, path, body, buf, rx_buf),
        )
        .await
        {
            Ok(body) => Ok(body),
            Err(WithTimeoutError::Timeout) => Err(HttpError::Timeout),
            Err(WithTimeoutError::Error(e)) => Err(e),
        }
    }

    async fn resolve(&self, host: &str, port: u16) -> Result<SocketAddr, HttpError> {
        // A literal address needs no DNS.
        if let Ok(ip) = host.parse::<Ipv4Addr>() {
            return Ok(SocketAddr::new(IpAddr::V4(ip), port));
        }
        let dns = EmbassyDns::new(self.stack);
        match dns.get_host_by_name(host, AddrType::IPv4).await {
            Ok(ip) => Ok(SocketAddr::new(ip, port)),
            Err(_) => Err(HttpError::Dns),
        }
    }
}

/// One request/response exchange. A free function because it borrows the
/// connection buffer and the socket for the whole exchange.
#[allow(clippy::too_many_arguments)]
async fn request<'a>(
    tcp: &Tcp<'_>,
    addr: SocketAddr,
    method: Method,
    host: &str,
    path: &str,
    body: Option<(&str, &[u8])>,
    buf: &mut [u8],
    rx_buf: &'a mut [u8],
) -> Result<&'a [u8], HttpError> {
    let mut connection: Connection<'_, Tcp<'_>> = Connection::new(buf, tcp, addr);

    // `Content-Length` needs the body length as text; `no_std` has no
    // `format!`, so write the digits into a stack buffer.
    let mut length = [0u8; 10];
    let content_length = body.map(|(_, payload)| decimal(payload.len() as u32, &mut length));

    let mut header_buf = [("", ""); 4];
    let headers = request_headers(host, body, content_length, &mut header_buf);
    connection
        .initiate_request(true, method, path, headers)
        .await
        .map_err(|_| HttpError::Request)?;

    if let Some((_, payload)) = body {
        connection
            .write_all(payload)
            .await
            .map_err(|_| HttpError::Write)?;
    }

    connection
        .initiate_response()
        .await
        .map_err(|_| HttpError::Response)?;

    let (_, response) = connection.split();
    let mut read = 0;
    loop {
        match response.read(&mut rx_buf[read..]).await {
            Ok(0) => break,
            Ok(n) => {
                read += n;
                if read == rx_buf.len() {
                    break;
                }
            }
            Err(_) => return Err(HttpError::Read),
        }
    }
    Ok(&rx_buf[..read])
}

/// The request headers edge-http wants: `Host`, `Connection: close` and, when
/// there is a body, `Content-Type`/`Content-Length`.
///
/// Headers that do not apply are left out rather than sent with an empty
/// value: edge-http parses the request's own headers, and an empty
/// `Content-Length` makes it panic in `BodyType::from_header`.
fn request_headers<'a, 'b>(
    host: &'a str,
    body: Option<(&'a str, &'a [u8])>,
    length: Option<&'a str>,
    buf: &'b mut [(&'a str, &'a str); 4],
) -> &'b [(&'a str, &'a str)] {
    let mut count = 0;
    buf[count] = ("Host", host);
    count += 1;
    buf[count] = ("Connection", "close");
    count += 1;
    if let (Some((content_type, _)), Some(length)) = (body, length) {
        buf[count] = ("Content-Type", content_type);
        count += 1;
        buf[count] = ("Content-Length", length);
        count += 1;
    }
    &buf[..count]
}

/// Split `http[s]://host[:port]/path` into its parts. `default_port` fills in
/// the port when the URL omits one.
pub(crate) fn parse_url(url: &str, default_port: u16) -> Result<(&str, u16, &str), HttpError> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or(HttpError::BadUrl)?;
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(HttpError::BadUrl);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            if host.is_empty() {
                return Err(HttpError::BadUrl);
            }
            let port = port.parse().map_err(|_| HttpError::BadUrl)?;
            Ok((host, port, path))
        }
        None => Ok((authority, default_port, path)),
    }
}

/// Write `value` in decimal into `buf`, returning the written slice.
pub(super) fn decimal(mut value: u32, buf: &mut [u8; 10]) -> &str {
    if value == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap_or("0");
    }
    let mut at = buf.len();
    while value > 0 && at > 0 {
        at -= 1;
        buf[at] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split_into_host_port_and_path() {
        assert_eq!(
            parse_url("http://example.com/api/v1", 80),
            Ok(("example.com", 80, "/api/v1"))
        );
        assert_eq!(
            parse_url("http://example.com:8080/x", 80),
            Ok(("example.com", 8080, "/x"))
        );
        assert_eq!(
            parse_url("https://example.com", 443),
            Ok(("example.com", 443, "/"))
        );
        assert_eq!(
            parse_url("http://10.0.0.1/status", 80),
            Ok(("10.0.0.1", 80, "/status"))
        );
        assert_eq!(parse_url("/status", 80), Err(HttpError::BadUrl));
        assert_eq!(parse_url("http://:80/x", 80), Err(HttpError::BadUrl));
    }

    #[test]
    fn get_headers_omit_the_body_headers() {
        let mut buf = [("", ""); 4];
        let headers = request_headers("example.com", None, None, &mut buf);
        assert_eq!(headers, [("Host", "example.com"), ("Connection", "close")]);
    }

    #[test]
    fn post_headers_carry_the_body_length() {
        let mut buf = [("", ""); 4];
        let payload = b"hi";
        let headers = request_headers(
            "example.com",
            Some(("text/plain", payload)),
            Some("2"),
            &mut buf,
        );
        assert_eq!(
            headers,
            [
                ("Host", "example.com"),
                ("Connection", "close"),
                ("Content-Type", "text/plain"),
                ("Content-Length", "2"),
            ]
        );
    }

    #[test]
    fn decimal_writes_digits_without_alloc() {
        let mut buf = [0u8; 10];
        assert_eq!(decimal(0, &mut buf), "0");
        assert_eq!(decimal(7, &mut buf), "7");
        assert_eq!(decimal(12345, &mut buf), "12345");
        assert_eq!(decimal(u32::MAX, &mut buf), "4294967295");
    }
}
