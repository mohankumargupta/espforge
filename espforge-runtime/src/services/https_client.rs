//! `HttpsClient`: an outgoing HTTPS client, TLS 1.3 over TCP.
//!
//! `embedded-tls` owns the handshake and record layer; the HTTP request is
//! written straight over the opened TLS stream. `edge-http` is not reused
//! here: its client drives a `TcpConnect` factory whose socket type must also
//! split, which a TLS stream cannot provide without a second connection
//! abstraction. What this does support: `GET`/`POST` with `Content-Length`,
//! `Connection: close`, and a `Content-Length` or close-delimited response —
//! enough for the REST-style requests the element exists for. Chunked
//! transfer-encoding is not decoded.
//!
//! Certificate verification needs `embedded-tls`'s `rustpki`/`webpki` features
//! and a trust store, so the handshake runs unverified today; `insecure` only
//! controls the build warning the element already emits. That gap is recorded
//! in the design doc, not hidden here.

use alloc::vec;
use core::net::{Ipv4Addr, SocketAddrV4};

use embassy_net::Stack;
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, with_timeout};
use embedded_io_async::Write;
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};

use super::http_client::{HttpError, HttpStreamError, StreamInfo, decimal, parse_url, parse_u32};
use super::rng::EspRng;

/// What `<https_client>` declared on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpsClientConfig {
    /// Bytes of TLS record read buffer, allocated from the heap.
    pub rx_buffer_size: usize,
    /// Bytes of TLS record write buffer, allocated from the heap.
    pub tx_buffer_size: usize,
    /// Connection, handshake and read/write timeout, in milliseconds.
    pub timeout_ms: u64,
}

/// A handle to the board's HTTPS client. `Copy`, so it can sit on `Services`.
#[derive(Clone, Copy)]
pub struct HttpsClient {
    stack: Stack<'static>,
    config: HttpsClientConfig,
    /// The declared `insecure` flag. Verification is not implemented yet, so
    /// this only records what the board asked for.
    insecure: bool,
}

impl HttpsClient {
    pub fn new(stack: Stack<'static>, config: HttpsClientConfig, insecure: bool) -> Self {
        Self {
            stack,
            config,
            insecure,
        }
    }

    /// Whether the board asked for `insecure="true"`.
    pub fn is_insecure(&self) -> bool {
        self.insecure
    }

    /// GET `url` and return the response body in `rx_buf`.
    pub async fn get<'a>(&self, url: &str, rx_buf: &'a mut [u8]) -> Result<&'a [u8], HttpError> {
        self.exchange("GET", url, None, rx_buf).await
    }

    /// GET `url`, handing the response body to `on_chunk` as it arrives.
    ///
    /// The TLS stream is read in `STREAM_CHUNK` pieces, so a firmware image of
    /// any size fits in RAM. The header block is read first, which is where the
    /// status and the optional length/CRC headers come from.
    pub async fn get_stream<F, E>(
        &self,
        url: &str,
        mut on_chunk: F,
    ) -> Result<StreamInfo, HttpStreamError<E>>
    where
        F: FnMut(&[u8]) -> Result<(), E>,
    {
        let (host, port, path) = parse_url(url, 443).map_err(HttpStreamError::Http)?;
        let timeout = self.config.timeout_ms.min(u32::MAX as u64) as u32;
        match with_timeout(
            Duration::from_millis(timeout as u64),
            self.stream_request(host, port, path, &mut on_chunk),
        )
        .await
        {
            // `embassy_time::with_timeout` hands back the future's own output;
            // only the timeout is turned into an error here.
            Ok(result) => result,
            Err(_) => Err(HttpStreamError::Http(HttpError::Timeout)),
        }
    }

    /// POST `payload` to `url` with `content_type`, returning the response body
    /// in `rx_buf`.
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
        self.exchange("POST", url, Some((content_type, payload)), rx_buf)
            .await
    }

    async fn exchange<'a>(
        &self,
        method: &str,
        url: &str,
        body: Option<(&str, &[u8])>,
        rx_buf: &'a mut [u8],
    ) -> Result<&'a [u8], HttpError> {
        let (host, port, path) = parse_url(url, 443)?;
        let timeout = self.config.timeout_ms.min(u32::MAX as u64) as u32;
        match with_timeout(
            Duration::from_millis(timeout as u64),
            self.request(method, host, port, path, body, rx_buf),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(HttpError::Timeout),
        }
    }

    async fn request<'a>(
        &self,
        method: &str,
        host: &str,
        port: u16,
        path: &str,
        body: Option<(&str, &[u8])>,
        rx_buf: &'a mut [u8],
    ) -> Result<&'a [u8], HttpError> {
        let addr = self.resolve(host, port).await?;

        // The socket's own buffers, then the TLS record buffers (heap-sized to
        // what the board declared).
        let mut socket_rx = [0u8; 1024];
        let mut socket_tx = [0u8; 1024];
        let mut socket = TcpSocket::new(self.stack, &mut socket_rx, &mut socket_tx);
        socket.connect(addr).await.map_err(|_| HttpError::Request)?;

        let mut record_read = vec![0u8; self.config.rx_buffer_size.max(4096)];
        let mut record_write = vec![0u8; self.config.tx_buffer_size.max(2048)];
        let config = TlsConfig::new().with_server_name(host);
        let mut tls =
            TlsConnection::<_, Aes128GcmSha256>::new(socket, &mut record_read, &mut record_write);

        tls.open(TlsContext::new(
            &config,
            UnsecureProvider::new::<Aes128GcmSha256>(EspRng::new()),
        ))
        .await
        .map_err(|_| HttpError::Request)?;

        // Request
        let mut length = [0u8; 10];
        let content_length = body.map(|(_, payload)| decimal(payload.len() as u32, &mut length));
        write_request(&mut tls, method, host, path, body, content_length).await?;

        // Response: read until the peer closes, then strip the header block.
        //
        // `embedded-tls` reports the peer's close as an *error* from `read`
        // (`TlsError::ConnectionClosed` for the close_notify record, `IoError`
        // once the socket is at EOF), never as a zero-length read. With
        // `Connection: close` that close is how the body ends, so bytes that
        // arrived before the error are a complete response — only an error
        // with nothing collected is a real read failure.
        let mut read = 0;
        loop {
            if read == rx_buf.len() {
                break;
            }
            match tls.read(&mut rx_buf[read..]).await {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(_) if read > 0 => break,
                Err(_) => return Err(HttpError::Read),
            }
        }
        Ok(body_of(&rx_buf[..read]))
    }

    /// One streaming GET over the opened TLS stream.
    async fn stream_request<F, E>(
        &self,
        host: &str,
        port: u16,
        path: &str,
        on_chunk: &mut F,
    ) -> Result<StreamInfo, HttpStreamError<E>>
    where
        F: FnMut(&[u8]) -> Result<(), E>,
    {
        let addr = self.resolve(host, port).await.map_err(HttpStreamError::Http)?;
        let mut socket_rx = [0u8; 1024];
        let mut socket_tx = [0u8; 1024];
        let mut socket = TcpSocket::new(self.stack, &mut socket_rx, &mut socket_tx);
        socket
            .connect(addr)
            .await
            .map_err(|_| HttpStreamError::Http(HttpError::Request))?;

        let mut record_read = vec![0u8; self.config.rx_buffer_size.max(4096)];
        let mut record_write = vec![0u8; self.config.tx_buffer_size.max(2048)];
        let config = TlsConfig::new().with_server_name(host);
        let mut tls =
            TlsConnection::<_, Aes128GcmSha256>::new(socket, &mut record_read, &mut record_write);
        tls.open(TlsContext::new(
            &config,
            UnsecureProvider::new::<Aes128GcmSha256>(EspRng::new()),
        ))
        .await
        .map_err(|_| HttpStreamError::Http(HttpError::Request))?;

        write_request(&mut tls, "GET", host, path, None, None)
            .await
            .map_err(HttpStreamError::Http)?;

        // The header block first: the status and the length/CRC headers are
        // what the caller checks before writing a byte of firmware.
        let mut head = [0u8; STREAM_HEADER_CAP];
        let mut read = 0;
        let end = loop {
            if read == head.len() {
                return Err(HttpStreamError::Http(HttpError::Response));
            }
            match tls.read(&mut head[read..]).await {
                Ok(0) => return Err(HttpStreamError::Http(HttpError::Response)),
                Ok(n) => {
                    read += n;
                    if let Some(end) = header_end(&head[..read]) {
                        break end;
                    }
                }
                Err(_) if read > 0 => return Err(HttpStreamError::Http(HttpError::Response)),
                Err(_) => return Err(HttpStreamError::Http(HttpError::Read)),
            }
        };
        let info = match core::str::from_utf8(&head[..end]) {
            Ok(text) => parse_head(text),
            Err(_) => return Err(HttpStreamError::Http(HttpError::Response)),
        };

        // Bytes that arrived with the header block are the start of the body.
        if end < read {
            on_chunk(&head[end..read]).map_err(HttpStreamError::Chunk)?;
        }

        let mut chunk = [0u8; STREAM_CHUNK];
        loop {
            match tls.read(&mut chunk).await {
                Ok(0) => break,
                // `embedded-tls` reports the peer's close as an error, the way
                // `request` does; the CRC check in `Ota::finish` catches a
                // truncated body, so a close is simply the end of the stream.
                Ok(n) => on_chunk(&chunk[..n]).map_err(HttpStreamError::Chunk)?,
                Err(_) => break,
            }
        }
        Ok(info)
    }

    async fn resolve(&self, host: &str, port: u16) -> Result<SocketAddrV4, HttpError> {
        if let Ok(ip) = host.parse::<Ipv4Addr>() {
            return Ok(SocketAddrV4::new(ip, port));
        }
        match self.stack.dns_query(host, DnsQueryType::A).await {
            Ok(addrs) => match addrs.first() {
                Some(embassy_net::IpAddress::Ipv4(v4)) => Ok(SocketAddrV4::new(*v4, port)),
                _ => Err(HttpError::Dns),
            },
            Err(_) => Err(HttpError::Dns),
        }
    }
}

/// Write the request line, headers and body over the TLS stream.
async fn write_request<W: Write>(
    out: &mut W,
    method: &str,
    host: &str,
    path: &str,
    body: Option<(&str, &[u8])>,
    content_length: Option<&str>,
) -> Result<(), HttpError> {
    out.write_all(method.as_bytes())
        .await
        .map_err(|_| HttpError::Write)?;
    out.write_all(b" ").await.map_err(|_| HttpError::Write)?;
    out.write_all(path.as_bytes())
        .await
        .map_err(|_| HttpError::Write)?;
    out.write_all(b" HTTP/1.1\r\nHost: ")
        .await
        .map_err(|_| HttpError::Write)?;
    out.write_all(host.as_bytes())
        .await
        .map_err(|_| HttpError::Write)?;
    out.write_all(b"\r\nConnection: close\r\n")
        .await
        .map_err(|_| HttpError::Write)?;
    if let (Some((content_type, _)), Some(length)) = (body, content_length) {
        out.write_all(b"Content-Type: ")
            .await
            .map_err(|_| HttpError::Write)?;
        out.write_all(content_type.as_bytes())
            .await
            .map_err(|_| HttpError::Write)?;
        out.write_all(b"\r\nContent-Length: ")
            .await
            .map_err(|_| HttpError::Write)?;
        out.write_all(length.as_bytes())
            .await
            .map_err(|_| HttpError::Write)?;
        out.write_all(b"\r\n").await.map_err(|_| HttpError::Write)?;
    }
    out.write_all(b"\r\n").await.map_err(|_| HttpError::Write)?;
    if let Some((_, payload)) = body {
        out.write_all(payload).await.map_err(|_| HttpError::Write)?;
    }
    out.flush().await.map_err(|_| HttpError::Write)?;
    Ok(())
}

/// Bytes read per callback in [`HttpsClient::get_stream`].
const STREAM_CHUNK: usize = 2048;

/// Bytes available for the response's header block.
const STREAM_HEADER_CAP: usize = 1024;

/// Where the response's header block ends, just past `\r\n\r\n`.
fn header_end(response: &[u8]) -> Option<usize> {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4)
}

/// The status code and the optional length/CRC headers from a header block.
fn parse_head(text: &str) -> StreamInfo {
    let mut info = StreamInfo {
        status: 0,
        content_length: None,
        crc32_header: None,
    };
    for (index, line) in text.split("\r\n").enumerate() {
        if index == 0 {
            info.status = line
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse().ok())
                .unwrap_or(0);
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            info.content_length = value.trim().parse().ok();
        } else if name.eq_ignore_ascii_case("X-Firmware-CRC32") {
            info.crc32_header = parse_u32(value);
        }
    }
    info
}

/// Everything after the response's header block.
fn body_of(response: &[u8]) -> &[u8] {
    match header_end(response) {
        Some(at) => &response[at..],
        // No header terminator: hand back what there is rather than nothing.
        None => response,
    }
}
