use core::fmt;
use core::cell::RefCell;

use edge_http::io::client::Connection;

use espforge_platform::embassy_net::Stack;
use heapless::String;

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Close(Option<u16>),
    Ping,
    Pong,
}

// ── Error type ─────────────────────────────────────────────────────────────────

pub enum WebSocketError {
    DnsResolutionFailed,
    ConnectionFailed,
    HandshakeFailed,
    SendFailed,
    ReceiveFailed,
    InvalidUri,
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

// ── WebSocketResources ─────────────────────────────────────────────────────────
// No TLS buffers needed: esp-mbedtls manages its own heap-allocated buffers.

pub struct WebSocketResources {
    pub(crate) rx_buf: Option<[u8; 1536]>,
    pub(crate) tx_buf: Option<[u8; 1536]>,
    pub(crate) payload_buf: Option<[u8; 1536]>,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf:      Some([0u8; 1536]),
            tx_buf:      Some([0u8; 1536]),
            payload_buf: Some([0u8; 1536]),
        }
    }

    /// Alias kept for source-compatibility — wss:// no longer needs caller-allocated
    /// TLS buffers; this is now identical to `new()`.
    pub const fn new_with_tls() -> Self {
        Self::new()
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

// ── Internal network helpers ────────────────────────────────────────────────────

struct DnsError;

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DNS error") }
}
impl fmt::Debug for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DnsError") }
}
impl embedded_io_async::Error for DnsError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

struct NetError;

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Network error") }
}
impl fmt::Debug for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "NetError") }
}
impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

use embedded_io_async::{ErrorType, Read as AsyncRead, Write as AsyncWrite};
use edge_nal::{AddrType, Dns, TcpConnect, Close};

struct MyTcpSocket {
    socket: espforge_platform::embassy_net::TcpSocket<'static>,
}

impl ErrorType for MyTcpSocket {
    type Error = NetError;
}

impl embedded_io_async::Readable for MyTcpSocket {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

struct MyTcpSocketRead<'a> {
    reader: espforge_platform::embassy_net::tcp::ReadHalf<'a>,
}
impl ErrorType for MyTcpSocketRead<'_> { type Error = NetError; }
impl AsyncRead for MyTcpSocketRead<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.reader.read(buf).await.map_err(|_| NetError)
    }
}

struct MyTcpSocketWrite<'a> {
    writer: espforge_platform::embassy_net::tcp::WriteHalf<'a>,
}
impl ErrorType for MyTcpSocketWrite<'_> { type Error = NetError; }
impl AsyncWrite for MyTcpSocketWrite<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.writer.write(buf).await.map_err(|_| NetError)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.writer.flush().await.map_err(|_| NetError)
    }
}

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

impl edge_nal::Readable for MyTcpSocket {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
impl Close for MyTcpSocket {
    async fn close(&mut self, _what: edge_nal::io::Close) -> Result<(), Self::Error> {
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
    stack: Stack<'static>,
    rx_buf: RefCell<[u8; 1536]>,
    tx_buf: RefCell<[u8; 1536]>,
}

impl NetworkAdapter {
    fn new(stack: Stack<'static>, rx: [u8; 1536], tx: [u8; 1536]) -> Self {
        Self { stack, rx_buf: RefCell::new(rx), tx_buf: RefCell::new(tx) }
    }
}

impl Dns for NetworkAdapter {
    type Error = DnsError;

    async fn get_host_by_name(
        &self,
        host: &str,
        _addr_type: AddrType,
    ) -> Result<core::net::IpAddr, Self::Error> {
        use core::net::{IpAddr, Ipv4Addr};
        let addrs = self.stack
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
        _result: &mut [core::net::IpAddr],
    ) -> Result<usize, Self::Error> {
        Err(DnsError)
    }
}

impl TcpConnect for NetworkAdapter {
    type Error = NetError;
    type Socket<'m> = MyTcpSocket where Self: 'm;

    async fn connect(
        &self,
        remote: core::net::SocketAddr,
    ) -> Result<Self::Socket<'static>, Self::Error> {
        use core::net::IpAddr;

        let mut rx = self.rx_buf.borrow_mut();
        let mut tx = self.tx_buf.borrow_mut();

        let ip = match remote.ip() {
            IpAddr::V4(v4) => {
                espforge_platform::embassy_net::Ipv4Address::from_octets(v4.octets())
            }
            IpAddr::V6(_) => return Err(NetError),
        };
        let endpoint = (ip, remote.port());

        let mut socket = espforge_platform::embassy_net::TcpSocket::new(
            self.stack,
            unsafe { core::slice::from_raw_parts_mut(rx.as_mut_ptr(), rx.len()) },
            unsafe { core::slice::from_raw_parts_mut(tx.as_mut_ptr(), tx.len()) },
        );
        socket.connect(endpoint).await.map_err(|_| NetError)?;
        Ok(MyTcpSocket { socket })
    }
}

// ── WebSocket upgrade helpers ───────────────────────────────────────────────────

fn upgrade_request_headers<'a>(
    host: &'a str,
    path: &'a str,
    nonce: &'a [u8; 16],
) -> heapless::Vec<(&'a str, &'a str), 8> {
    use base64::Engine;
    let mut key_buf = [0u8; 24];
    base64::engine::general_purpose::STANDARD
        .encode_slice(nonce, &mut key_buf)
        .ok();
    let key_str = core::str::from_utf8(&key_buf).unwrap_or("");

    let mut v = heapless::Vec::new();
    let _ = v.push(("Host", host));
    let _ = v.push(("Upgrade", "websocket"));
    let _ = v.push(("Connection", "Upgrade"));
    let _ = v.push(("Sec-WebSocket-Key", key_str));
    let _ = v.push(("Sec-WebSocket-Version", "13"));
    v
}

fn is_upgrade_accepted<'a>(
    nonce: &[u8; 16],
    headers: impl Iterator<Item = (&'a str, &'a str)>,
) -> bool {
    use base64::Engine;
    use sha1::{Digest, Sha1};

    let mut key_b64 = [0u8; 24];
    base64::engine::general_purpose::STANDARD
        .encode_slice(nonce, &mut key_b64)
        .ok();

    let mut hasher = Sha1::new();
    hasher.update(&key_b64);
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let expected = hasher.finalize();
    let mut expected_b64 = [0u8; 28];
    base64::engine::general_purpose::STANDARD
        .encode_slice(&expected, &mut expected_b64)
        .ok();

    for (name, value) in headers {
        if name.eq_ignore_ascii_case("sec-websocket-accept") {
            return value.as_bytes() == &expected_b64[..];
        }
    }
    false
}

// ── WebSocketClient ─────────────────────────────────────────────────────────────

pub struct WebSocketClient {
    stack: Stack<'static>,
    uri: String<128>,
    resources: WebSocketResources,
    // Plain TCP path
    socket: Option<MyTcpSocket>,
    // TLS path — the session wraps the TCP socket and implements Read+Write.
    // We box it so the type size is bounded regardless of the generic T inside Session.
    #[cfg(feature = "websockets")]
    tls_socket: Option<alloc::boxed::Box<dyn TlsSocket>>,
}

// Trait-object wrapper so we can hold either a plain or TLS write path.
#[cfg(feature = "websockets")]
trait TlsSocket: embedded_io_async::Read + embedded_io_async::Write {}
#[cfg(feature = "websockets")]
impl<T: embedded_io_async::Read + embedded_io_async::Write> TlsSocket for T {}

impl WebSocketClient {
    pub fn new(
        stack: Stack<'static>,
        uri: &str,
        resources: &'static mut WebSocketResources,
    ) -> Self {
        let mut s: String<128> = String::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            uri: s,
            resources: WebSocketResources {
                rx_buf:      resources.rx_buf.take(),
                tx_buf:      resources.tx_buf.take(),
                payload_buf: resources.payload_buf.take(),
            },
            socket: None,
            #[cfg(feature = "websockets")]
            tls_socket: None,
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
                host_port[i+1..].parse::<u16>().map_err(|_| WebSocketError::InvalidUri)?,
            ),
            None => (host_port, if is_wss { 443 } else { 80 }),
        };

        let mut host: String<64> = String::new();
        let mut path: String<64> = String::new();
        host.push_str(host_raw).map_err(|_| WebSocketError::InvalidUri)?;
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

    async fn connect_plain(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        let rx = self.resources.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.resources.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        let ip = edge_nal::Dns::get_host_by_name(&adapter, host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = core::net::SocketAddr::new(ip, port);
        let mut socket = adapter.connect(remote).await.map_err(|_| WebSocketError::ConnectionFailed)?;

        // Perform WebSocket upgrade
        let mut nonce = [0u8; 16];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        // Build and send upgrade request manually
        self.do_ws_upgrade_plain(&mut socket, &host, &path, &nonce).await?;

        self.socket = Some(socket);
        Ok(())
    }

    /// Connect over TLS using esp-mbedtls.
    ///
    /// The esp-mbedtls `Session::new()` call takes ownership of the plain TCP socket
    /// and returns a session that implements `embedded_io_async::Read + Write`.
    async fn connect_tls(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        use mbedtls_rs::{asynch::Session, Certificates, Mode, TlsVersion};

        let rx = self.resources.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.resources.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        let ip = edge_nal::Dns::get_host_by_name(&adapter, host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = core::net::SocketAddr::new(ip, port);
        let plain_socket = adapter
            .connect(remote)
            .await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        // Wrap the TCP socket in an esp-mbedtls TLS session.
        // Certificates::new() uses the built-in Mozilla root store compiled into
        // the esp-mbedtls binary blob.
        let mut session = Session::new(
            plain_socket.socket,
            host.as_str(),
            Mode::Client,
            TlsVersion::Tls1_3,
            Certificates::new(),
        )
        .map_err(|_| WebSocketError::TlsError)?;

        session.connect().await.map_err(|_| WebSocketError::TlsError)?;

        // Perform the WebSocket upgrade over the TLS session.
        let mut nonce = [0u8; 16];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        self.do_ws_upgrade_tls(&mut session, &host, &path, &nonce)
            .await?;

        // Store session behind a Box<dyn …> so the outer struct stays Sized.
        self.tls_socket = Some(alloc::boxed::Box::new(session));
        Ok(())
    }

    // ── upgrade helpers ─────────────────────────────────────────────────────────

    async fn do_ws_upgrade_plain(
        &self,
        socket: &mut MyTcpSocket,
        host: &str,
        path: &str,
        nonce: &[u8; 16],
    ) -> Result<(), WebSocketError> {
        self.send_upgrade_request(socket, host, path, nonce).await?;
        self.read_upgrade_response(socket, nonce).await
    }

    async fn do_ws_upgrade_tls<S>(
        &self,
        session: &mut S,
        host: &str,
        path: &str,
        nonce: &[u8; 16],
    ) -> Result<(), WebSocketError>
    where
        S: embedded_io_async::Read + embedded_io_async::Write,
    {
        self.send_upgrade_request(session, host, path, nonce).await?;
        self.read_upgrade_response(session, nonce).await
    }

    async fn send_upgrade_request<S>(
        &self,
        stream: &mut S,
        host: &str,
        path: &str,
        nonce: &[u8; 16],
    ) -> Result<(), WebSocketError>
    where
        S: embedded_io_async::Write,
    {
        use embedded_io_async::Write;

        let mut line: String<128> = String::new();
        core::fmt::write(&mut line, format_args!("GET {} HTTP/1.1\r\n", path))
            .map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(line.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;

        let headers = upgrade_request_headers(host, path, nonce);
        for (name, value) in headers.iter() {
            let mut hdr: String<128> = String::new();
            core::fmt::write(&mut hdr, format_args!("{}: {}\r\n", name, value))
                .map_err(|_| WebSocketError::SendFailed)?;
            stream.write_all(hdr.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
        }
        stream.write_all(b"\r\n").await.map_err(|_| WebSocketError::SendFailed)?;
        stream.flush().await.map_err(|_| WebSocketError::SendFailed)?;
        Ok(())
    }

    async fn read_upgrade_response<S>(
        &self,
        stream: &mut S,
        nonce: &[u8; 16],
    ) -> Result<(), WebSocketError>
    where
        S: embedded_io_async::Read,
    {
        use embedded_io_async::Read;

        let mut resp_buf = [0u8; 1024];
        let mut total = 0usize;
        loop {
            let n = stream.read(&mut resp_buf[total..]).await.map_err(|_| WebSocketError::HandshakeFailed)?;
            total += n;
            if resp_buf[..total].windows(4).any(|w| w == b"\r\n\r\n") { break; }
            if total >= resp_buf.len() { break; }
        }

        let resp_str = core::str::from_utf8(&resp_buf[..total])
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let status = resp_str
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(WebSocketError::HandshakeFailed)?;

        if status != 101 {
            return Err(WebSocketError::HandshakeFailed);
        }

        let resp_headers: heapless::Vec<(&str, &str), 32> = resp_str
            .lines()
            .skip(1)
            .filter_map(|line| {
                let mut parts = line.splitn(2, ':');
                let name  = parts.next()?.trim();
                let value = parts.next()?.trim();
                Some((name, value))
            })
            .collect();

        if !is_upgrade_accepted(nonce, resp_headers.iter().copied()) {
            return Err(WebSocketError::HandshakeFailed);
        }

        Ok(())
    }

    // ── send / receive ──────────────────────────────────────────────────────────

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        use edge_ws::FrameHeader;
        use edge_ws::FrameType;
        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key: Some(mask_key),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(&mut **session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut **session, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
            use embedded_io_async::Write;
            (**session).flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(socket, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
            use embedded_io_async::Write;
            socket.flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else {
            return Err(WebSocketError::SendFailed);
        }
        Ok(())
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        use edge_ws::FrameHeader;
        use edge_ws::FrameType;
        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key: Some(mask_key),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(&mut **session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut **session, data).await.map_err(|_| WebSocketError::SendFailed)?;
            use embedded_io_async::Write;
            (**session).flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(socket, data).await.map_err(|_| WebSocketError::SendFailed)?;
            use embedded_io_async::Write;
            socket.flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else {
            return Err(WebSocketError::SendFailed);
        }
        Ok(())
    }

    pub async fn receive<'b>(
        &mut self,
        buf: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        use edge_ws::{FrameHeader, FrameType};

        macro_rules! recv_from {
            ($stream:expr) => {{
                let header = FrameHeader::recv($stream)
                    .await
                    .map_err(|_| WebSocketError::ReceiveFailed)?;

                let len     = header.payload_len.min(buf.len() as u64) as usize;
                let payload = &mut buf[..len];
                header
                    .recv_payload($stream, payload)
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
                    FrameType::Ping      => {
                        // Auto-pong
                        let pong = FrameHeader {
                            frame_type: FrameType::Pong,
                            payload_len: payload.len() as u64,
                            mask_key: None,
                        };
                        pong.send($stream).await.map_err(|_| WebSocketError::SendFailed)?;
                        pong.send_payload($stream, payload).await.map_err(|_| WebSocketError::SendFailed)?;
                        Ok(Some(Message::Ping))
                    }
                    FrameType::Pong      => Ok(Some(Message::Pong)),
                    _                    => Err(WebSocketError::UnexpectedFrame),
                }
            }};
        }

        if let Some(session) = self.tls_socket.as_mut() {
            recv_from!(&mut **session)
        } else if let Some(socket) = self.socket.as_mut() {
            recv_from!(socket)
        } else {
            Err(WebSocketError::ReceiveFailed)
        }
    }
}

