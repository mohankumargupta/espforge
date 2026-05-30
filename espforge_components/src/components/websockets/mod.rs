use core::fmt;
use core::cell::RefCell;
use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use espforge_platform::embassy_net::Stack;
use heapless::String;
use edge_http::io::client::Connection;
use edge_nal::AddrType;
use edge_ws::{FrameHeader, FrameType};

// ── Public message type ───────────────────────────────────────────────────────

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close(Option<&'a str>),
}

// ── Errors ────────────────────────────────────────────────────────────────────

pub enum WebSocketError {
    DnsResolutionFailed,
    ConnectionFailed,
    HandshakeFailed,
    SendFailed,
    ReceiveFailed,
    InvalidUri,
    TlsBuffersMissing,
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
            Self::ProtocolError       => write!(f, "WebSocket protocol error"),
            Self::UnexpectedFrame     => write!(f, "Unexpected WebSocket frame type"),
        }
    }
}

// ── Resources ─────────────────────────────────────────────────────────────────

pub struct WebSocketResources {
    rx_buf: Option<[u8; 1536]>,
    tx_buf: Option<[u8; 1536]>,
    payload_buf: Option<[u8; 1536]>,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf: Some([0u8; 1536]),
            tx_buf: Some([0u8; 1536]),
            payload_buf: Some([0u8; 1536]),
        }
    }

    pub const fn new_with_tls() -> Self {
        Self::new()
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

// ── DnsError / NetError ───────────────────────────────────────────────────────
//
// embedded-io 0.7 requires Error: core::error::Error.
// In no_std we implement core::error::Error manually (it has no required methods).

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

// core::error::Error has no required methods in Rust ≥ 1.81 (stabilised for no_std).
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

// TcpSplit is required by edge-nal; we provide a no-op implementation since
// we always use the socket as a whole, never split.
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

// ── NetworkAdapter ────────────────────────────────────────────────────────────
//
// Holds the TCP socket buffers in RefCell so that TcpConnect::connect, which
// takes &self per the trait definition, can still access them mutably.

pub struct NetworkAdapter {
    stack: Stack<'static>,
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

impl edge_nal::TcpConnect for NetworkAdapter {
    type Error = NetError;
    type Socket<'m> = MyTcpSocket where Self: 'm;

    // Trait requires &self. Buffers are accessed via RefCell borrow_mut().
    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        let mut rx = self.rx_buf.borrow_mut();
        let mut tx = self.tx_buf.borrow_mut();

        let mut socket = espforge_platform::embassy_net::tcp::TcpSocket::new(
            self.stack,
            &mut *rx,
            &mut *tx,
        );

        let addr = match remote.ip() {
            IpAddr::V4(v4) => espforge_platform::embassy_net::IpEndpoint::new(
                espforge_platform::embassy_net::IpAddress::Ipv4(
                    espforge_platform::embassy_net::Ipv4Address::from_octets(v4.octets()),
                ),
                remote.port(),
            ),
            IpAddr::V6(_) => return Err(NetError),
        };

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
    stack: Stack<'static>,
    uri: String<128>,
    socket: Option<MyTcpSocket>,
    payload_buf: Option<[u8; 1536]>,
    rx_buf: Option<[u8; 1536]>,
    tx_buf: Option<[u8; 1536]>,
}

impl WebSocketClient {
    pub fn new(
        stack: Stack<'static>,
        uri: &str,
        resources: &'static mut WebSocketResources,
    ) -> Self {
        let mut s = String::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            uri: s,
            socket: None,
            payload_buf: resources.payload_buf.take(),
            rx_buf: resources.rx_buf.take(),
            tx_buf: resources.tx_buf.take(),
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
            None => (rest, "/"),
        };

        let (host_raw, port) = match host_port.find(':') {
            Some(i) => (
                &host_port[..i],
                host_port[i + 1..]
                    .parse::<u16>()
                    .map_err(|_| WebSocketError::InvalidUri)?,
            ),
            None => (host_port, if is_wss { 443u16 } else { 80u16 }),
        };

        let mut host = String::<64>::new();
        host.push_str(host_raw).map_err(|_| WebSocketError::InvalidUri)?;
        let mut path = String::<64>::new();
        path.push_str(path_raw).map_err(|_| WebSocketError::InvalidUri)?;

        Ok((host, port, path, is_wss))
    }

    /// Perform DNS resolution, TCP connect, and WebSocket upgrade handshake.
    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        let (host, port, path, is_wss) = self.parse_uri()?;

        if is_wss {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        let mut conn_buf = self.payload_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let rx = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        // Resolve DNS
        use edge_nal::Dns;
        let ip = adapter
            .get_host_by_name(host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = SocketAddr::new(ip, port);

        // Generate random nonce using the platform RNG wrapper (no esp_hal dep needed here)
        const NONCE_LEN: usize = 16;
        let mut nonce = [0u8; NONCE_LEN];
        // SAFETY: called once, peripheral released before next use
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        // edge-http 0.7: Connection::new(buf, socket, addr: SocketAddr)
        let mut conn = Connection::<_>::new(&mut conn_buf, &adapter, remote);

        // initiate_ws_upgrade_request has 6 parameters in edge-http 0.7:
        //   host, origin, path, extra_headers, nonce, headers_buf: &mut [u8; 28]
        let mut headers_buf = [0u8; 28];
        conn.initiate_ws_upgrade_request(
            Some(host.as_str()),
            None,
            path.as_str(),
            None,
            &nonce,
            &mut headers_buf,
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_response()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        // is_ws_upgrade_accepted takes &mut [u8; 33] in edge-http 0.7
        let mut resp_key_buf = [0u8; 33];
        let accepted = conn
            .is_ws_upgrade_accepted(&nonce, &mut resp_key_buf)
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        if !accepted {
            return Err(WebSocketError::HandshakeFailed);
        }

        conn.complete()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        // conn.release() returns (socket, &mut [u8]) — the slice borrows conn_buf.
        // We take the socket and drop everything else; payload_buf stays None,
        // which is fine since it was only needed for the HTTP upgrade phase.
        let (socket, _) = conn.release();
        self.socket = Some(socket);

        Ok(())
    }

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        // SAFETY: called once per send, peripheral released immediately
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key: Some(mask_key),
        };
        header.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
        header
            .send_payload(&mut *socket, text.as_bytes())
            .await
            .map_err(|_| WebSocketError::SendFailed)?;
        Ok(())
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key: Some(mask_key),
        };
        header.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
        header
            .send_payload(&mut *socket, data)
            .await
            .map_err(|_| WebSocketError::SendFailed)?;
        Ok(())
    }

    pub async fn receive<'b>(
        &mut self,
        buf: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::ReceiveFailed)?;

        let header = FrameHeader::recv(&mut *socket)
            .await
            .map_err(|_| WebSocketError::ReceiveFailed)?;

        // FIX: compute the length first so we only hold one borrow on `buf`
        let len = header.payload_len.min(buf.len() as u64) as usize;
        let payload = &mut buf[..len];

        header
            .recv_payload(&mut *socket, payload)
            .await
            .map_err(|_| WebSocketError::ReceiveFailed)?;

        match header.frame_type {
            FrameType::Text(_) => {
                let text = core::str::from_utf8(payload)
                    .map_err(|_| WebSocketError::ProtocolError)?;
                Ok(Some(Message::Text(text)))
            }
            FrameType::Binary(_) => Ok(Some(Message::Binary(payload))),
            FrameType::Close => Ok(Some(Message::Close(None))),
            FrameType::Ping => {
                let pong = FrameHeader {
                    frame_type: FrameType::Pong,
                    payload_len: payload.len() as u64,
                    mask_key: None,
                };
                pong.send(&mut *socket)
                    .await
                    .map_err(|_| WebSocketError::SendFailed)?;
                pong.send_payload(&mut *socket, payload)
                    .await
                    .map_err(|_| WebSocketError::SendFailed)?;
                Ok(Some(Message::Ping))
            }
            FrameType::Pong => Ok(Some(Message::Pong)),
            _ => Err(WebSocketError::UnexpectedFrame),
        }
    }
}

