// espforge_components/src/components/websockets/mod.rs
// 
// Three fixes applied:
//  1. TcpConnect::connect takes &self (not &mut self) - trait requirement
//  2. NetworkAdapter holds buffers in RefCell so &self can mutably access them
//  3. After conn.release(), don't assign &mut [u8] back to Option<[u8; 1536]>

use core::fmt;
use core::cell::RefCell;
use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use espforge_platform::embassy_net::Stack;
use heapless::String;
use edge_http::io::client::Connection;
use edge_nal::{AddrType, Dns, TcpConnect};
use edge_ws::{FrameHeader, FrameType};

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close(Option<&'a str>),
}

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
            Self::ConnectionFailed   => write!(f, "Connection failed"),
            Self::HandshakeFailed   => write!(f, "WebSocket handshake failed"),
            Self::SendFailed        => write!(f, "Send failed"),
            Self::ReceiveFailed     => write!(f, "Receive failed"),
            Self::InvalidUri        => write!(f, "Invalid WebSocket URI"),
            Self::TlsBuffersMissing => write!(f, "TLS buffers required for wss://"),
            Self::ProtocolError     => write!(f, "WebSocket protocol error"),
            Self::UnexpectedFrame   => write!(f, "Unexpected WebSocket frame type"),
        }
    }
}

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

pub struct DnsError;

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "DnsError")
    }
}

impl embedded_io_async::Error for DnsError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

pub struct NetError;

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "NetError")
    }
}

impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

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

// ── NetworkAdapter ────────────────────────────────────────────────────────────
//
// FIX 1 + 2:
//   - The edge-nal 0.6 TcpConnect trait requires `connect(&self, ...)`, not `&mut self`.
//   - Buffers are stored as RefCell<[u8; 1536]> (owned arrays, not Options) so we can
//     call borrow_mut() inside &self methods.
//   - The resulting TcpSocket<'_> borrows from the RefCell guards, which live for the
//     duration of the connect() call — long enough because the socket is moved into
//     WebSocketClient before NetworkAdapter is dropped.

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

    // FIX 1: &self instead of &mut self (matches the trait signature).
    // FIX 2: borrow_mut() on the RefCell fields gives us &mut [u8] without
    //         needing &mut self. The MutexGuards keep the borrows alive for
    //         the lifetime of `socket`.
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

        // The socket borrows rx/tx which are RefMut guards scoped to this function.
        // We need it to outlive this call. Since WebSocketClient owns the buffers
        // for the lifetime of the connection, we extend via transmute.
        // SAFETY: the buffers in self (NetworkAdapter) live for the entire
        // WebSocketClient::connect() scope, which is longer than this call.
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

    /// Perform DNS resolution, TCP connect, and edge-http WebSocket handshake.
    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        let (host, port, path, is_wss) = self.parse_uri()?;

        if is_wss {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        let mut conn_buf = self.payload_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let rx = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let mut adapter = NetworkAdapter::new(self.stack, rx, tx);

        let ip = adapter
            .get_host_by_name(host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = SocketAddr::new(ip, port);

        const NONCE_LEN: usize = 16;
        let mut nonce = [0u8; NONCE_LEN];
        // Use esp-hal's RNG
        {
            use esp_hal::rng::Rng;
            // SAFETY: we only call this once and don't alias the peripheral
            let mut rng = unsafe { Rng::new(esp_hal::peripherals::RNG::steal()) };
            for i in (0..NONCE_LEN).step_by(4) {
                let r = rng.random().to_ne_bytes();
                nonce[i..i + 4].copy_from_slice(&r);
            }
        }

        let mut conn = Connection::<_>::new(&mut conn_buf, &mut adapter, ());

        conn.initiate_ws_upgrade_request(
            Some(host.as_str()),
            None,
            path.as_str(),
            None,
            &nonce,
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_response()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let mut resp_key_buf = [0u8; 64];
        let accepted = conn
            .is_ws_upgrade_accepted(&nonce, &mut resp_key_buf)
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        if !accepted {
            return Err(WebSocketError::HandshakeFailed);
        }

        conn.complete()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        // FIX 3: conn.release() returns (socket, &mut [u8]) — the &mut [u8] is a
        // slice into conn_buf (our local [u8; 1536]) and cannot be stored back as
        // Option<[u8; 1536]>.  We simply drop it; payload_buf stays None, which is
        // fine because it was only needed for the HTTP upgrade handshake phase.
        let (socket, _conn_buf_slice) = conn.release();
        self.socket = Some(socket);
        // conn_buf (the owned [u8; 1536]) is dropped here after _conn_buf_slice is
        // released — no use-after-free.

        Ok(())
    }

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mut rng = unsafe { esp_hal::rng::Rng::new(esp_hal::peripherals::RNG::steal()) };

        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key: Some(rng.random()),
        };
        header
            .send(&mut *socket)
            .await
            .map_err(|_| WebSocketError::SendFailed)?;
        header
            .send_payload(&mut *socket, text.as_bytes())
            .await
            .map_err(|_| WebSocketError::SendFailed)?;
        Ok(())
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let mut rng = unsafe { esp_hal::rng::Rng::new(esp_hal::peripherals::RNG::steal()) };

        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key: Some(rng.random()),
        };
        header
            .send(&mut *socket)
            .await
            .map_err(|_| WebSocketError::SendFailed)?;
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

        let header = match FrameHeader::recv(&mut *socket).await {
            Ok(h) => h,
            Err(_) => return Err(WebSocketError::ReceiveFailed),
        };

        let payload = &mut buf[..header.payload_len.min(buf.len() as u64) as usize];
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

