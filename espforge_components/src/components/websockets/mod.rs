#![allow(dead_code)]

use core::fmt;
use core::cell::RefCell;
use core::net::{IpAddr, Ipv4Addr};

use edge_http::io::client::Connection;
use edge_http::ws::{
    upgrade_request_headers, is_upgrade_accepted,
    MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN,
};
use edge_nal::{AddrType, Close, TcpConnect};
use edge_ws::{FrameHeader, FrameType};
use embedded_io_async::{ErrorType, Read as AsyncRead, Write as AsyncWrite};
use espforge_platform::embassy_net::Stack;
use heapless::String;
use rand_core::{CryptoRng, RngCore};

use embedded_tls as tls;

// ── Public message type ────────────────────────────────────────────────────────

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close(Option<u16>),
}

// ── Error type ─────────────────────────────────────────────────────────────────
#[derive(Debug)]
pub enum WebSocketError {
    DnsResolutionFailed,
    ConnectionFailed,
    HandshakeFailed,
    SendFailed,
    ReceiveFailed,
    InvalidUri,
    TlsBuffersMissing,
    TlsError,
    ProtocolError,
    UnexpectedFrame,
}


impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DnsResolutionFailed => write!(f, "DNS resolution failed"),
            Self::ConnectionFailed    => write!(f, "Connection failed"),
            Self::HandshakeFailed     => write!(f, "WebSocket handshake failed"),
            Self::SendFailed          => write!(f, "Send failed"),
            Self::ReceiveFailed       => write!(f, "Receive failed"),
            Self::InvalidUri          => write!(f, "Invalid WebSocket URI"),
            Self::TlsBuffersMissing   => write!(f, "TLS buffers required for wss://"),
            Self::TlsError            => write!(f, "TLS error"),
            Self::ProtocolError       => write!(f, "WebSocket protocol error"),
            Self::UnexpectedFrame     => write!(f, "Unexpected WebSocket frame type"),
        }
    }
}

// ── TLS buffers ────────────────────────────────────────────────────────────────

/// Read/write record buffers for a TLS connection.
/// The read buffer must be at least 16 640 bytes (max TLS record size).
pub struct TlsBuffers {
    pub read_buf:  [u8; 16640],
    pub write_buf: [u8; 4096],
}

impl TlsBuffers {
    pub const fn new() -> Self {
        Self {
            read_buf:  [0u8; 16640],
            write_buf: [0u8; 4096],
        }
    }
}

impl Default for TlsBuffers {
    fn default() -> Self {
        Self::new()
    }
}

// ── WebSocket resources ────────────────────────────────────────────────────────

pub struct WebSocketResources {
    pub rx_buf:      Option<[u8; 1536]>,
    pub tx_buf:      Option<[u8; 1536]>,
    pub payload_buf: Option<[u8; 1536]>,
    /// Only needed for `wss://` connections.
    pub tls_buffers: Option<TlsBuffers>,
}

impl WebSocketResources {
    /// Plain `ws://` resources — no TLS buffers.
    pub const fn new() -> Self {
        Self {
            rx_buf:      Some([0u8; 1536]),
            tx_buf:      Some([0u8; 1536]),
            payload_buf: Some([0u8; 1536]),
            tls_buffers: None,
        }
    }

    /// `wss://` resources — includes TLS record buffers.
    pub const fn new_with_tls() -> Self {
        Self {
            rx_buf:      Some([0u8; 1536]),
            tx_buf:      Some([0u8; 1536]),
            payload_buf: Some([0u8; 1536]),
            tls_buffers: Some(TlsBuffers::new()),
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

// ── Error stubs ────────────────────────────────────────────────────────────────

struct DnsError;

impl core::error::Error for DnsError {}
impl core::error::Error for NetError {}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DNS error")
    }
}
impl fmt::Debug for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DnsError")
    }
}
impl embedded_io_async::Error for DnsError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

struct NetError;

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Network error")
    }
}
impl fmt::Debug for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NetError")
    }
}
impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

// ── MyTcpSocket ────────────────────────────────────────────────────────────────

struct MyTcpSocket {
    socket: espforge_platform::embassy_net::tcp::TcpSocket<'static>,
}

impl ErrorType for MyTcpSocket {
    type Error = NetError;
}

// Implement Readable for the main socket
impl edge_nal::Readable for MyTcpSocket {
    async fn readable(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// Create a wrapper for the read half
struct MyTcpSocketRead<'a> {
    reader: espforge_platform::embassy_net::tcp::TcpReader<'a>,
}

impl<'a> embedded_io_async::ErrorType for MyTcpSocketRead<'a> {
    type Error = NetError;
}

impl<'a> embedded_io_async::Read for MyTcpSocketRead<'a> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.reader.read(buf).await.map_err(|_| NetError)
    }
}

impl<'a> edge_nal::Readable for MyTcpSocketRead<'a> {
    async fn readable(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// Create a wrapper for the write half
struct MyTcpSocketWrite<'a> {
    writer: espforge_platform::embassy_net::tcp::TcpWriter<'a>,
}

impl<'a> embedded_io_async::ErrorType for MyTcpSocketWrite<'a> {
    type Error = NetError;
}

impl<'a> embedded_io_async::Write for MyTcpSocketWrite<'a> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.writer.write(buf).await.map_err(|_| NetError)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.writer.flush().await.map_err(|_| NetError)
    }
}

// Implement TcpSplit to tie it all together
impl edge_nal::TcpSplit for MyTcpSocket {
    type Read<'a> = MyTcpSocketRead<'a> where Self: 'a;
    type Write<'a> = MyTcpSocketWrite<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        let (reader, writer) = self.socket.split();
        (MyTcpSocketRead { reader }, MyTcpSocketWrite { writer })
    }
}

impl AsyncRead for MyTcpSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.socket.read(buf).await.map_err(|_| NetError)
    }
}

impl AsyncWrite for MyTcpSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.socket.write(buf).await.map_err(|_| NetError)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.socket.flush().await.map_err(|_| NetError)
    }
}

impl edge_nal::TcpShutdown for MyTcpSocket {
    async fn close(&mut self, _what: Close) -> Result<(), Self::Error> {
        self.socket.close();
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        self.socket.abort();
        Ok(())
    }
}

// ── NetworkAdapter ─────────────────────────────────────────────────────────────

struct NetworkAdapter {
    stack:  Stack<'static>,
    rx_buf: RefCell<[u8; 1536]>,
    tx_buf: RefCell<[u8; 1536]>,
}

impl NetworkAdapter {
    fn new(stack: Stack<'static>, rx: [u8; 1536], tx: [u8; 1536]) -> Self {
        Self {
            stack,
            rx_buf: RefCell::new(rx),
            tx_buf: RefCell::new(tx),
        }
    }
}

impl edge_nal::Dns for NetworkAdapter {
    type Error = DnsError;

    async fn get_host_by_name(
        &self,
        host: &str,
        _addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        let addrs = self
            .stack
            .dns_query(host, espforge_platform::embassy_net::dns::DnsQueryType::A)
            .await
            .map_err(|_| DnsError)?;

        if let Some(espforge_platform::embassy_net::IpAddress::Ipv4(v4)) = addrs.first() {
            let mut octets = [0u8; 4];
            octets.copy_from_slice(&v4.octets());
            Ok(IpAddr::V4(Ipv4Addr::from(octets)))
        } else {
            Err(DnsError)
        }
    }

    async fn get_host_by_address(
        &self,
        _addr: IpAddr,
        _result: &mut [u8],
    ) -> Result<usize, Self::Error> {
        Err(DnsError)
    }
}

impl TcpConnect for NetworkAdapter {
    type Error      = NetError;
    type Socket<'m> = MyTcpSocket where Self: 'm;

    async fn connect(
        &self,
        remote: core::net::SocketAddr,
    ) -> Result<Self::Socket<'_>, Self::Error> {
        let mut rx = self.rx_buf.borrow_mut();
        let mut tx = self.tx_buf.borrow_mut();

        let ip = match remote.ip() {
            IpAddr::V4(v4) => {
                espforge_platform::embassy_net::Ipv4Address::from_octets(v4.octets())
            }
            IpAddr::V6(_) => return Err(NetError),
        };

        let endpoint = espforge_platform::embassy_net::IpEndpoint::new(
            espforge_platform::embassy_net::IpAddress::Ipv4(ip),
            remote.port(),
        );

        // SAFETY: buffers are valid for the lifetime of this socket usage within this scope
        let mut socket = espforge_platform::embassy_net::tcp::TcpSocket::new(
            self.stack,
            unsafe { core::slice::from_raw_parts_mut(rx.as_mut_ptr(), rx.len()) },
            unsafe { core::slice::from_raw_parts_mut(tx.as_mut_ptr(), tx.len()) },
        );

        socket.connect(endpoint).await.map_err(|_| NetError)?;
        Ok(MyTcpSocket { socket })
    }
}

// ── EspRng — adapts Rng to rand_core traits ────────────────────────────────────

struct EspRng(espforge_platform::rng::Rng);

impl RngCore for EspRng {
    fn next_u32(&mut self) -> u32 {
        self.0.random_u32()
    }
    fn next_u64(&mut self) -> u64 {
        let lo = self.0.random_u32() as u64;
        let hi = self.0.random_u32() as u64;
        (hi << 32) | lo
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for EspRng {}

// ── WebSocketClient ────────────────────────────────────────────────────────────

pub struct WebSocketClient {
    stack:       Stack<'static>,
    uri:         String<128>,
    socket:      Option<MyTcpSocket>,
    payload_buf: Option<[u8; 1536]>,
    rx_buf:      Option<[u8; 1536]>,
    tx_buf:      Option<[u8; 1536]>,
    tls_buffers: Option<TlsBuffers>,
}

impl WebSocketClient {
    pub fn new(
        stack: Stack<'static>,
        uri: &str,
        resources: &mut WebSocketResources,
    ) -> Self {
        let mut s = String::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            uri: s,
            socket:      None,
            payload_buf: resources.payload_buf.take(),
            rx_buf:      resources.rx_buf.take(),
            tx_buf:      resources.tx_buf.take(),
            tls_buffers: resources.tls_buffers.take(),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    // ── URI parsing ────────────────────────────────────────────────────────────

    fn parse_uri(&self) -> Result<(String<64>, u16, String<64>, bool), WebSocketError> {
        let s = self.uri.as_str();

        let (is_wss, rest) = if let Some(r) = s.strip_prefix("wss://") {
            (true, r)
        } else if let Some(r) = s.strip_prefix("ws://") {
            (false, r)
        } else {
            return Err(WebSocketError::InvalidUri);
        };

        let (host_port, path_raw) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None    => (rest, "/"),
        };

        let (host_raw, port) = match host_port.find(':') {
            Some(i) => (
                &host_port[..i],
                host_port[i + 1..]
                    .parse::<u16>()
                    .map_err(|_| WebSocketError::InvalidUri)?,
            ),
            None => (host_port, if is_wss { 443 } else { 80 }),
        };

        let mut host = String::<64>::new();
        host.push_str(host_raw).map_err(|_| WebSocketError::InvalidUri)?;

        let mut path = String::<64>::new();
        path.push_str(path_raw).map_err(|_| WebSocketError::InvalidUri)?;

        Ok((host, port, path, is_wss))
    }

    // ── Public connect ─────────────────────────────────────────────────────────

    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        let (host, port, path, is_wss) = self.parse_uri()?;
        if is_wss {
            if self.tls_buffers.is_none() {
                return Err(WebSocketError::TlsBuffersMissing);
            }
            self.connect_tls(host, port, path).await
        } else {
            self.connect_plain(host, port, path).await
        }
    }

    // ── Plain ws:// ────────────────────────────────────────────────────────────

    async fn connect_plain(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        let conn_buf = self
            .payload_buf
            .as_mut()
            .ok_or(WebSocketError::ConnectionFailed)?;

        let rx = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        let ip = edge_nal::Dns::get_host_by_name(&adapter, host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = core::net::SocketAddr::new(ip, port);

        let mut nonce      = [0u8; NONCE_LEN];
        let mut key_buf    = [0u8; MAX_BASE64_KEY_LEN];
        let mut accept_buf = [0u8; MAX_BASE64_KEY_RESPONSE_LEN];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        let mut conn = Connection::<_, 32>::new(conn_buf, &adapter, remote);

        conn.initiate_ws_upgrade_request(
            Some(host.as_str()),
            None,
            path.as_str(),
            None,
            &nonce,
            &mut key_buf,
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_response()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let accepted = conn
            .is_ws_upgrade_accepted(&nonce, &mut accept_buf)
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        if !accepted {
            return Err(WebSocketError::HandshakeFailed);
        }

        let (raw_socket, _buf) = conn.release();
        self.socket = Some(raw_socket);
        Ok(())
    }

    // ── Secure wss:// ──────────────────────────────────────────────────────────

    async fn connect_tls(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        let rx = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        // Resolve host → IP
        let adapter = NetworkAdapter::new(self.stack, rx, tx);
        let ip = edge_nal::Dns::get_host_by_name(&adapter, host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = core::net::SocketAddr::new(ip, port);

        // Open plain TCP
        let tcp = TcpConnect::connect(&adapter, remote)
            .await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        // Wrap in TLS using embedded-tls 0.18 API
        let tls_bufs = self
            .tls_buffers
            .as_mut()
            .ok_or(WebSocketError::TlsBuffersMissing)?;

        let tls_config = tls::TlsConfig::new().with_server_name(host.as_str());
        let rng = EspRng(unsafe { espforge_platform::rng::Rng::new() });

        let mut tls_conn = tls::TlsConnection::<_, tls::Aes128GcmSha256>::new(
            tcp,
            &mut tls_bufs.read_buf,
            &mut tls_bufs.write_buf,
        );

        tls_conn
            .open(tls::TlsContext::new(
                &tls_config,
                tls::UnsecureProvider::new::<tls::Aes128GcmSha256>(rng),
            ))
            .await
            .map_err(|_| WebSocketError::TlsError)?;

        // WebSocket upgrade over the TLS stream
        let mut nonce      = [0u8; NONCE_LEN];
        let mut key_buf    = [0u8; MAX_BASE64_KEY_LEN];
        let mut accept_buf = [0u8; MAX_BASE64_KEY_RESPONSE_LEN];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        let headers = upgrade_request_headers(
            Some(host.as_str()),
            None,
            None,
            &nonce,
            &mut key_buf,
        );

        // Send "GET <path> HTTP/1.1\r\n"
        {
            let mut line = String::<128>::new();
            core::fmt::write(&mut line, format_args!("GET {} HTTP/1.1\r\n", path.as_str()))
                .map_err(|_| WebSocketError::HandshakeFailed)?;
            tls_conn
                .write_all(line.as_bytes())
                .await
                .map_err(|_| WebSocketError::HandshakeFailed)?;
        }

        // Send each header
        for (name, value) in headers.iter() {
            if name.is_empty() {
                continue;
            }
            let mut hdr = String::<128>::new();
            core::fmt::write(&mut hdr, format_args!("{}: {}\r\n", name, value))
                .map_err(|_| WebSocketError::HandshakeFailed)?;
            tls_conn
                .write_all(hdr.as_bytes())
                .await
                .map_err(|_| WebSocketError::HandshakeFailed)?;
        }

        // End of headers
        tls_conn
            .write_all(b"\r\n")
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        tls_conn
            .flush()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        // Read HTTP response until we see the end-of-headers marker
        let resp_buf = self
            .payload_buf
            .as_mut()
            .ok_or(WebSocketError::ConnectionFailed)?;

        let mut total = 0usize;
        loop {
            let n = tls_conn
                .read(&mut resp_buf[total..])
                .await
                .map_err(|_| WebSocketError::HandshakeFailed)?;
            if n == 0 {
                return Err(WebSocketError::HandshakeFailed);
            }
            total += n;
            if resp_buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
            if total >= resp_buf.len() {
                return Err(WebSocketError::HandshakeFailed);
            }
        }

        // Parse status code and headers from the raw response
        let resp_str = core::str::from_utf8(&resp_buf[..total])
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let mut lines = resp_str.lines();

        // Status line: "HTTP/1.1 101 Switching Protocols"
        let status_code = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(WebSocketError::HandshakeFailed)?;

        // Collect response headers as (name, value) pairs
        let resp_headers: heapless::Vec<(&str, &str), 16> = lines
            .filter_map(|line| {
                let mut parts = line.splitn(2, ':');
                let name  = parts.next()?.trim();
                let value = parts.next()?.trim();
                Some((name, value))
            })
            .collect();

        let accepted = is_upgrade_accepted(
            status_code,
            resp_headers.iter().copied(),
            &nonce,
            &mut accept_buf,
        );

        if !accepted {
            return Err(WebSocketError::HandshakeFailed);
        }

        // Close the TLS layer and recover the inner TCP socket.
        // embedded-tls close() sends a TLS close_notify and returns the socket.
        let inner_tcp = tls_conn
            .close()
            .await
            .map_err(|(_, _)| WebSocketError::TlsError)?;

        self.socket = Some(inner_tcp);
        Ok(())
    }

    // ── Send ───────────────────────────────────────────────────────────────────

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let socket   = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type:  FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key:    Some(mask_key),
        };

        header.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
        header.send_payload(&mut *socket, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
        socket.flush().await.map_err(|_| WebSocketError::SendFailed)
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        let socket   = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type:  FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key:    Some(mask_key),
        };

        header.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
        header.send_payload(&mut *socket, data).await.map_err(|_| WebSocketError::SendFailed)?;
        socket.flush().await.map_err(|_| WebSocketError::SendFailed)
    }

    // ── Receive ────────────────────────────────────────────────────────────────

    pub async fn receive<'b>(
        &mut self,
        buf: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::ReceiveFailed)?;

        let header = FrameHeader::recv(&mut *socket)
            .await
            .map_err(|_| WebSocketError::ReceiveFailed)?;

        let len     = header.payload_len.min(buf.len() as u64) as usize;
        let payload = &mut buf[..len];

        header
            .recv_payload(&mut *socket, payload)
            .await
            .map_err(|_| WebSocketError::ProtocolError)?;

        match header.frame_type {
            FrameType::Text(_) => {
                let text = core::str::from_utf8(payload)
                    .map_err(|_| WebSocketError::ProtocolError)?;
                Ok(Some(Message::Text(text)))
            }
            FrameType::Binary(_) => Ok(Some(Message::Binary(payload))),
            FrameType::Close     => Ok(Some(Message::Close(None))),
            FrameType::Ping => {
                let pong = FrameHeader {
                    frame_type:  FrameType::Pong,
                    payload_len: payload.len() as u64,
                    mask_key:    None,
                };
                pong.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
                pong.send_payload(&mut *socket, payload).await.map_err(|_| WebSocketError::SendFailed)?;
                Ok(Some(Message::Ping))
            }
            FrameType::Pong => Ok(Some(Message::Pong)),
            _               => Err(WebSocketError::UnexpectedFrame),
        }
    }
}

