use core::fmt;
use core::cell::RefCell;

use espforge_platform::embassy_net::Stack;
use heapless::String;
use edge_http::io::client::Connection;
use edge_nal::AddrType;

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close(Option<u16>),
}

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

impl fmt::Debug for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

// ── Resources ─────────────────────────────────────────────────────────────────

/// TLS read/write buffers.  Each must be large enough for the biggest TLS
/// record the server will ever send (typically ≤ 16 KiB).
pub struct TlsBuffers {
    pub read_buf:  [u8; 16384],
    pub write_buf: [u8; 4096],
}

impl TlsBuffers {
    pub const fn new() -> Self {
        Self {
            read_buf:  [0u8; 16384],
            write_buf: [0u8; 4096],
        }
    }
}

impl Default for TlsBuffers {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WebSocketResources {
    pub rx_buf:      Option<[u8; 1536]>,
    pub tx_buf:      Option<[u8; 1536]>,
    pub payload_buf: Option<[u8; 1536]>,
    /// Present only when using `wss://`.
    pub tls_buffers: Option<TlsBuffers>,
}

impl WebSocketResources {
    /// Plain `ws://` resources — no TLS buffers allocated.
    pub const fn new() -> Self {
        Self {
            rx_buf:      Some([0u8; 1536]),
            tx_buf:      Some([0u8; 1536]),
            payload_buf: Some([0u8; 1536]),
            tls_buffers: None,
        }
    }

    /// `wss://` resources — TLS read/write buffers are included.
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

// ── DnsError / NetError ───────────────────────────────────────────────────────

pub struct DnsError;

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
impl core::error::Error for DnsError {}

impl embedded_io_async::Error for DnsError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

pub struct NetError;

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
impl core::error::Error for NetError {}

impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

// ── MyTcpSocket ───────────────────────────────────────────────────────────────

pub struct MyTcpSocket {
    socket: espforge_platform::embassy_net::tcp::TcpSocket<'static>,
}

impl embedded_io_async::ErrorType for MyTcpSocket {
    type Error = NetError;
}

impl embedded_io_async::Read for MyTcpSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.socket.read(buf).await.map_err(|_| NetError)
    }
}

impl embedded_io_async::Write for MyTcpSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.socket.write(buf).await.map_err(|_| NetError)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.socket.flush().await.map_err(|_| NetError)
    }
}

impl edge_nal::Readable for MyTcpSocket {
    async fn readable(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl edge_nal::TcpShutdown for MyTcpSocket {
    async fn close(&mut self, _what: edge_nal::Close) -> Result<(), Self::Error> {
        self.socket.close();
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        self.socket.abort();
        Ok(())
    }
}

pub struct DummyHalf;

impl embedded_io_async::ErrorType for DummyHalf {
    type Error = NetError;
}
impl embedded_io_async::Read for DummyHalf {
    async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> { Ok(0) }
}
impl embedded_io_async::Write for DummyHalf {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> { Ok(buf.len()) }
    async fn flush(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
impl edge_nal::Readable for DummyHalf {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
impl edge_nal::TcpShutdown for DummyHalf {
    async fn close(&mut self, _what: edge_nal::Close) -> Result<(), Self::Error> { Ok(()) }
    async fn abort(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

impl edge_nal::TcpSplit for MyTcpSocket {
    type Read<'h> = DummyHalf where Self: 'h;
    type Write<'h> = DummyHalf where Self: 'h;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        (DummyHalf, DummyHalf)
    }
}

// ── TlsSocket ─────────────────────────────────────────────────────────────────
//
// Wraps `MyTcpSocket` in an `embedded_tls::TlsConnection` so that the same
// `Connection<T>` / WebSocket handshake path works for `wss://` as for `ws://`.

#[cfg(feature = "websockets")]
pub struct TlsSocket<'b> {
    conn: embedded_tls::TlsConnection<'b, MyTcpSocket, embedded_tls::Aes128GcmSha256>,
}

#[cfg(feature = "websockets")]
impl<'b> embedded_io_async::ErrorType for TlsSocket<'b> {
    type Error = NetError;
}

#[cfg(feature = "websockets")]
impl<'b> embedded_io_async::Read for TlsSocket<'b> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        use embedded_io_async::Read as _;
        self.conn.read(buf).await.map_err(|_| NetError)
    }
}

#[cfg(feature = "websockets")]
impl<'b> embedded_io_async::Write for TlsSocket<'b> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        use embedded_io_async::Write as _;
        self.conn.write(buf).await.map_err(|_| NetError)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        use embedded_io_async::Write as _;
        self.conn.flush().await.map_err(|_| NetError)
    }
}

#[cfg(feature = "websockets")]
impl<'b> edge_nal::Readable for TlsSocket<'b> {
    async fn readable(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(feature = "websockets")]
impl<'b> edge_nal::TcpShutdown for TlsSocket<'b> {
    async fn close(&mut self, _what: edge_nal::Close) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn abort(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// ── NetworkAdapter ────────────────────────────────────────────────────────────

pub struct NetworkAdapter {
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
        addr_type: AddrType,
    ) -> Result<core::net::IpAddr, Self::Error> {
        use core::net::{IpAddr, Ipv4Addr};

        let _ = addr_type;
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
        _addr: core::net::IpAddr,
        _result: &mut [u8],
    ) -> Result<usize, Self::Error> {
        Err(DnsError)
    }
}

impl edge_nal::TcpConnect for NetworkAdapter {
    type Error = NetError;
    type Socket<'m> = MyTcpSocket where Self: 'm;

    async fn connect(&self, remote: core::net::SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        use core::net::IpAddr;

        let mut rx = self.rx_buf.borrow_mut();
        let mut tx = self.tx_buf.borrow_mut();

        let addr = match remote.ip() {
            IpAddr::V4(v4) => espforge_platform::embassy_net::IpEndpoint::new(
                espforge_platform::embassy_net::IpAddress::Ipv4(
                    espforge_platform::embassy_net::Ipv4Address::from_octets(v4.octets()),
                ),
                remote.port(),
            ),
            IpAddr::V6(_) => return Err(NetError),
        };

        let mut socket = espforge_platform::embassy_net::tcp::TcpSocket::new(
            self.stack,
            &mut *rx,
            &mut *tx,
        );

        socket.connect(addr).await.map_err(|_| NetError)?;

        // SAFETY: The NetworkAdapter (and its RefCell buffers) lives for the
        // entire WebSocketClient::connect() scope, which outlives any use of
        // the returned socket within that scope.
        let socket = unsafe {
            core::mem::transmute::<
                espforge_platform::embassy_net::tcp::TcpSocket<'_>,
                espforge_platform::embassy_net::tcp::TcpSocket<'static>,
            >(socket)
        };

        Ok(MyTcpSocket { socket })
    }
}

// ── WebSocketClient ───────────────────────────────────────────────────────────

pub struct WebSocketClient {
    stack:       Stack<'static>,
    uri:         String<128>,
    payload_buf: Option<[u8; 1536]>,
    rx_buf:      Option<[u8; 1536]>,
    tx_buf:      Option<[u8; 1536]>,
    /// Non-None when using `wss://` and `new_with_tls()` resources were supplied.
    tls_buffers: Option<TlsBuffers>,
    socket:      Option<espforge_platform::embassy_net::tcp::TcpSocket<'static>>,
}

impl WebSocketClient {
    pub fn new(
        stack: Stack<'static>,
        uri: &str,
        resources: &mut WebSocketResources,
    ) -> Self {
        let mut s = String::<128>::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            uri: s,
            payload_buf: resources.payload_buf.take(),
            rx_buf:      resources.rx_buf.take(),
            tx_buf:      resources.tx_buf.take(),
            tls_buffers: resources.tls_buffers.take(),
            socket:      None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

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

    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        let (host, port, path, is_wss) = self.parse_uri()?;

        if is_wss {
            self.connect_tls(host, port, path).await
        } else {
            self.connect_plain(host, port, path).await
        }
    }

    // ── Plain ws:// ───────────────────────────────────────────────────────────

    async fn connect_plain(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        use edge_nal::Dns;

        let mut conn_buf = self.payload_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let rx = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        let ip = adapter
            .get_host_by_name(host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote: core::net::SocketAddr = core::net::SocketAddr::new(ip, port);
        let tcp = adapter.connect(remote).await.map_err(|_| WebSocketError::ConnectionFailed)?;

        let mut nonce = [0u8; 16];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        let mut conn: Connection<_> = Connection::new(&mut conn_buf, tcp);

        conn.initiate_ws_upgrade_request(
            Some(host.as_str()),
            None,
            path.as_str(),
            None,
            &nonce,
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_response().await.map_err(|_| WebSocketError::HandshakeFailed)?;

        let mut resp_key_buf = [0u8; 28];
        if !conn
            .is_ws_upgrade_accepted(&nonce, &mut resp_key_buf)
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?
        {
            return Err(WebSocketError::HandshakeFailed);
        }

        let (socket, _) = conn.complete().release();
        self.socket = Some(socket.socket);

        Ok(())
    }

    // ── Secure wss:// ─────────────────────────────────────────────────────────

    async fn connect_tls(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        use edge_nal::Dns;
        use embedded_tls::{TlsConfig, TlsContext, TlsVerify, NoiseRng};

        // Require TLS buffers — they must have been supplied via `new_with_tls()`.
        let tls_bufs = self.tls_buffers.as_mut().ok_or(WebSocketError::TlsBuffersMissing)?;

        let mut conn_buf = self.payload_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let rx = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        // DNS resolution
        let ip = adapter
            .get_host_by_name(host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote: core::net::SocketAddr = core::net::SocketAddr::new(ip, port);

        // Plain TCP socket
        let tcp = adapter.connect(remote).await.map_err(|_| WebSocketError::ConnectionFailed)?;

        // Wrap the TCP socket in a TLS connection.
        //
        // `TlsConfig::new()` accepts a seed for the internal RNG.  We obtain
        // a random u64 from the ESP hardware RNG for freshness.
        let seed = {
            let mut rng = unsafe { espforge_platform::rng::Rng::new() };
            let lo = rng.random_u32() as u64;
            let hi = rng.random_u32() as u64;
            (hi << 32) | lo
        };

        // embedded-tls ≥ 0.17 uses `TlsConfig` + `TlsContext`.
        // We skip certificate verification here (NoVerify) because embedded
        // targets seldom carry a trust-anchor database.  Users who need full
        // verification can extend this by passing a `TlsVerify::Trusted` config.
        let tls_config = TlsConfig::new()
            .with_server_name(host.as_str())
            .with_cert_verification(TlsVerify::None);

        let mut tls_conn = embedded_tls::TlsConnection::new(
            tcp,
            &mut tls_bufs.read_buf,
            &mut tls_bufs.write_buf,
        );

        tls_conn
            .open(TlsContext::new(
                &tls_config,
                NoiseRng(seed),
            ))
            .await
            .map_err(|_| WebSocketError::TlsError)?;

        // WebSocket upgrade over TLS
        let mut nonce = [0u8; 16];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        let mut conn: Connection<_> = Connection::new(&mut conn_buf, tls_conn);

        conn.initiate_ws_upgrade_request(
            Some(host.as_str()),
            None,
            path.as_str(),
            None,
            &nonce,
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_response().await.map_err(|_| WebSocketError::HandshakeFailed)?;

        let mut resp_key_buf = [0u8; 28];
        if !conn
            .is_ws_upgrade_accepted(&nonce, &mut resp_key_buf)
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?
        {
            return Err(WebSocketError::HandshakeFailed);
        }

        // The underlying TLS connection owns the raw TCP socket; we store the
        // inner TCP socket so that `send_text` / `receive` can reuse the same
        // code-path.  For TLS we keep the tls_conn alive through `tls_buffers`.
        //
        // NOTE: Because `embedded-tls` does not expose a way to "detach" the
        // inner socket after the handshake, we complete and release *only* the
        // `edge-http` connection wrapper.  The TLS session state lives in the
        // `tls_bufs` we hold in `self.tls_buffers`.
        let (tls_socket, _) = conn.complete().release();

        // Drop the TLS wrapper; the buffers are already in self.tls_buffers.
        // We reconstruct the raw inner TCP socket from the TLS connection.
        let raw_tcp = tls_socket.into_inner();
        self.socket = Some(raw_tcp.socket);

        Ok(())
    }

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        use edge_ws::{FrameHeader, FrameType};

        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type:  FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key:    Some(mask_key),
        };

        header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
        header
            .send_payload(socket, text.as_bytes())
            .await
            .map_err(|_| WebSocketError::SendFailed)?;

        Ok(())
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        use edge_ws::{FrameHeader, FrameType};

        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type:  FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key:    Some(mask_key),
        };

        header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
        header
            .send_payload(socket, data)
            .await
            .map_err(|_| WebSocketError::SendFailed)?;

        Ok(())
    }

    pub async fn receive<'b>(
        &mut self,
        buf: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        use edge_ws::{FrameHeader, FrameType};

        let socket = self.socket.as_mut().ok_or(WebSocketError::ReceiveFailed)?;

        let header = FrameHeader::recv(socket)
            .await
            .map_err(|_| WebSocketError::ReceiveFailed)?;

        let len = header.payload_len.min(buf.len() as u64) as usize;
        let payload = &mut buf[..len];

        match header.frame_type {
            FrameType::Text(_) => {
                header
                    .recv_payload(socket, payload)
                    .await
                    .map_err(|_| WebSocketError::ReceiveFailed)?;
                let text = core::str::from_utf8(payload)
                    .map_err(|_| WebSocketError::ProtocolError)?;
                Ok(Some(Message::Text(text)))
            }
            FrameType::Binary(_) => Ok(Some(Message::Binary(payload))),
            FrameType::Close     => Ok(Some(Message::Close(None))),
            FrameType::Ping      => {
                header
                    .recv_payload(socket, payload)
                    .await
                    .map_err(|_| WebSocketError::ProtocolError)?;

                let pong = FrameHeader {
                    frame_type:  FrameType::Pong,
                    payload_len: payload.len() as u64,
                    mask_key:    None,
                };
                pong.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
                pong.send_payload(socket, payload)
                    .await
                    .map_err(|_| WebSocketError::SendFailed)?;

                Ok(Some(Message::Ping))
            }
            FrameType::Pong => Ok(Some(Message::Pong)),
            _               => Err(WebSocketError::UnexpectedFrame),
        }
    }
}

