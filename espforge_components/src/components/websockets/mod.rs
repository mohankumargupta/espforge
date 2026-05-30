use core::fmt;

use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use espforge_platform::embassy_net::Stack;

use heapless::String;

use edge_http::io::client::Connection;

use edge_http::ws::{MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN};

use edge_nal::{AddrType, Dns, TcpConnect};

use edge_ws::{FrameHeader, FrameType};

#[derive(Debug)]
pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close(Option<u16>),
}

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
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

// ── Resources ────────────────────────────────────────────────────────────────

pub struct WebSocketResources {
    pub rx_buf: [u8; 1536],
    pub tx_buf: [u8; 1536],
    pub payload_buf: [u8; 1536],
    pub has_tls: bool,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf: [8; 1536],
            tx_buf: [8; 1536],
            payload_buf: [8; 1536],
            has_tls: false,
        }
    }

    pub const fn new_with_tls() -> Self {
        Self {
            rx_buf: [8; 1536],
            tx_buf: [8; 1536],
            payload_buf: [8; 1536],
            has_tls: true,
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

// ── TCP Wrapper ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct DnsError;
impl core::fmt::Display for DnsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "DnsError") }
}
impl core::error::Error for DnsError {}
impl embedded_io_async::Error for DnsError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

#[derive(Debug)]
pub struct NetError;
impl core::fmt::Display for NetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "NetError") }
}
impl core::error::Error for NetError {}
impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

pub struct MyTcpSocket<'a>(pub espforge_platform::embassy_net::tcp::TcpSocket<'a>);

impl<'a> embedded_io_async::ErrorType for MyTcpSocket<'a> {
    type Error = NetError;
}

impl<'a> embedded_io_async::Read for MyTcpSocket<'a> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await.map_err(|_| NetError)
    }
}

impl<'a> embedded_io_async::Write for MyTcpSocket<'a> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await.map_err(|_| NetError)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await.map_err(|_| NetError)
    }
}

impl<'a> edge_nal::Readable for MyTcpSocket<'a> {
    async fn readable(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<'a> edge_nal::TcpShutdown for MyTcpSocket<'a> {
    async fn close(&mut self, _what: edge_nal::Close) -> Result<(), Self::Error> {
        self.0.close();
        Ok(())
    }
    async fn abort(&mut self) -> Result<(), Self::Error> {
        self.0.abort();
        Ok(())
    }
}

pub struct DummyHalf;
impl embedded_io_async::ErrorType for DummyHalf { type Error = NetError; }
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

impl<'a> edge_nal::TcpSplit for MyTcpSocket<'a> {
    type Read<'h> = DummyHalf where Self: 'h;
    type Write<'h> = DummyHalf where Self: 'h;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        (DummyHalf, DummyHalf)
    }
}

// ── Network Adapter ──────────────────────────────────────────────────────────

pub struct NetworkAdapter<'a> {
    stack: Stack<'static>,
    rx_buf: core::cell::RefCell<Option<&'a mut [u8]>>,
    tx_buf: core::cell::RefCell<Option<&'a mut [u8]>>,
}

impl<'a> Dns for NetworkAdapter<'a> {
    type Error = DnsError;

    async fn get_host_by_name(&self, host: &str, _addr_type: AddrType) -> Result<IpAddr, Self::Error> {
        let addrs = self.stack.dns_query(host, espforge_platform::embassy_net::dns::DnsQueryType::A)
            .await
            .map_err(|_| DnsError)?;
        if let Some(espforge_platform::embassy_net::IpAddress::Ipv4(v4)) = addrs.first() {
            let mut octets = 8; 4];
            octets.copy_from_slice(&v4.octets());
            Ok(IpAddr::V4(Ipv4Addr::from(octets)))
        } else {
            Err(DnsError)
        }
    }

    async fn get_host_by_address(&self, _addr: IpAddr, _result: &mut [u8]) -> Result<usize, Self::Error> {
        Err(DnsError)
    }
}

impl<'a> TcpConnect for NetworkAdapter<'a> {
    type Error = NetError;
    type Socket<'m> = MyTcpSocket<'a> where Self: 'm;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        let mut socket = espforge_platform::embassy_net::tcp::TcpSocket::new(
            self.stack,
            self.rx_buf.borrow_mut().take().unwrap(),
            self.tx_buf.borrow_mut().take().unwrap(),
        );
        let addr = espforge_platform::embassy_net::IpEndpoint::new(
            match remote.ip() {
                IpAddr::V4(v4) => espforge_platform::embassy_net::IpAddress::Ipv4(
                    espforge_platform::embassy_net::Ipv4Address::from_octets(v4.octets()),
                ),
                IpAddr::V6(_) => panic!("IPv6 not supported"),
            },
            remote.port(),
        );
        socket.connect(addr).await.map_err(|_| NetError)?;
        Ok(MyTcpSocket(socket))
    }
}

// ── Client ───────────────────────────────────────────────────────────────────

pub struct WebSocketClient<'a> {
    stack: Stack<'static>,
    uri: String<128>,
    socket: Option<MyTcpSocket<'a>>,
    payload_buf: Option<&'a mut [u8]>,
    rx_buf: Option<&'a mut [u8]>,
    tx_buf: Option<&'a mut [u8]>,
    has_tls: bool,
}

impl<'a> WebSocketClient<'a> {
    pub fn new(
        stack: Stack<'static>,
        resources: &'a mut WebSocketResources,
        uri: &str,
    ) -> Self {
        let mut s = String::new();
        let _ = s.push_str(uri);

        Self {
            stack,
            uri: s,
            socket: None,
            payload_buf: Some(&mut resources.payload_buf),
            rx_buf: Some(&mut resources.rx_buf),
            tx_buf: Some(&mut resources.tx_buf),
            has_tls: resources.has_tls,
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
            Some(i) => {
                let p = host_port[i + 1..]
                    .parse()
                    .map_err(|_| WebSocketError::InvalidUri)?;
                (&host_port[..i], p)
            }
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

        if is_wss && !self.has_tls {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        let conn_buf = self.payload_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        // adapter lives only through the handshake block, so it drops before
        // self.payload_buf is reassigned below — preventing the lifetime conflict.
        let buf = {
            let adapter: NetworkAdapter<'a> = NetworkAdapter {
                stack: self.stack,
                rx_buf: core::cell::RefCell::new(self.rx_buf.take()),
                tx_buf: core::cell::RefCell::new(self.tx_buf.take()),
            };

            let mut conn: Connection<'_, NetworkAdapter<'_>, 64> =
                Connection::new(conn_buf, &adapter, SocketAddr::new(ip, port));

            let rng = espforge_platform::esp_hal::rng::Rng::new();
            let mut nonce = [0_u8; NONCE_LEN];
            for i in (0..NONCE_LEN).step_by(4) {
                let r = rng.random().to_ne_bytes();
                nonce[i..i + 4].copy_from_slice(&r);
            }

            let mut key_buf = [0_u8; MAX_BASE64_KEY_LEN];
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

            let mut resp_key_buf = [0_u8; MAX_BASE64_KEY_RESPONSE_LEN];
            if !conn
                .is_ws_upgrade_accepted(&nonce, &mut resp_key_buf)
                .map_err(|_| WebSocketError::HandshakeFailed)?
            {
                return Err(WebSocketError::HandshakeFailed);
            }

            conn.complete().await.map_err(|_| WebSocketError::HandshakeFailed)?;

            let (socket, buf) = conn.release();
            self.socket = Some(socket);
            buf
        }; // adapter dropped here

        self.payload_buf = Some(buf);
        Ok(())
    }

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let socket = self.socket.as_mut().ok_or(WebSocketError::SendFailed)?;
        let rng = espforge_platform::esp_hal::rng::Rng::new();

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
        let rng = espforge_platform::esp_hal::rng::Rng::new();

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

        let payload = header
            .recv_payload(&mut *socket, buf)
            .await
            .map_err(|_| WebSocketError::ReceiveFailed)?;

        match header.frame_type {
            FrameType::Text(_) => {
                let text = core::str::from_utf8(payload).map_err(|_| WebSocketError::ProtocolError)?;
                Ok(Some(Message::Text(text)))
            }
            FrameType::Binary(_) => Ok(Some(Message::Binary(payload))),
            FrameType::Close => Ok(Some(Message::Close(None))),
            FrameType::Ping => {
                let rng = espforge_platform::esp_hal::rng::Rng::new();
                let pong_header = FrameHeader {
                    frame_type: FrameType::Pong,
                    payload_len: payload.len() as u64,
                    mask_key: Some(rng.random()),
                };
                pong_header
                    .send(&mut *socket)
                    .await
                    .map_err(|_| WebSocketError::SendFailed)?;
                pong_header
                    .send_payload(&mut *socket, payload)
                    .await
                    .map_err(|_| WebSocketError::SendFailed)?;
                Ok(Some(Message::Ping))
            }
            FrameType::Pong => Ok(Some(Message::Pong)),
            _ => Err(WebSocketError::UnexpectedFrame),
        }
    }
}

