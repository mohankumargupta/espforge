extern crate alloc;

use core::fmt;
use core::cell::RefCell;

// Fix: TcpSocket is in embassy_net::tcp, not embassy_net root
use espforge_platform::embassy_net::tcp::TcpSocket;
use espforge_platform::embassy_net::Stack;
use heapless::String;

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close,
}

// ── Error types ────────────────────────────────────────────────────────────────

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

// ── WebSocketResources ─────────────────────────────────────────────────────────

pub struct WebSocketResources {
    pub rx_buf:      Option<[u8; 1536]>,
    pub tx_buf:      Option<[u8; 1536]>,
    pub payload_buf: Option<[u8; 1536]>,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf:      Some([0u8; 1536]),
            tx_buf:      Some([0u8; 1536]),
            payload_buf: Some([0u8; 1536]),
        }
    }

    /// Alias kept for source-compatibility with wss:// path.
    pub const fn new_with_tls() -> Self { Self::new() }
}

impl Default for WebSocketResources {
    fn default() -> Self { Self::new() }
}

// ── Internal error types ────────────────────────────────────────────────────────
//
// embedded-io 0.7.x requires that types implementing embedded_io_async::Error
// also implement core::error::Error (stabilised in Rust 1.81 for no_std).

struct DnsError;

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DNS error") }
}
impl fmt::Debug for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DnsError") }
}
impl core::error::Error for DnsError {}
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
impl core::error::Error for NetError {}
impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

// ── MyTcpSocket ────────────────────────────────────────────────────────────────
//
// Wraps embassy_net::tcp::TcpSocket and implements the edge-nal traits required
// by TcpConnect::Socket:
//   Read + Write + TcpSplit + TcpShutdown + Readable   (all with Error = NetError)

use embedded_io_async::{ErrorType, Read as AsyncRead, Write as AsyncWrite};
use edge_nal::{TcpConnect, TcpSplit, TcpShutdown, Dns, AddrType};
// Fix: Readable is in edge_nal root, not edge_nal::io
use edge_nal::Readable;

struct MyTcpSocket {
    // Fix: embassy_net::tcp::TcpSocket, not embassy_net::TcpSocket
    socket: TcpSocket<'static>,
}

impl ErrorType for MyTcpSocket {
    type Error = NetError;
}

// Fix: Readable is edge_nal::Readable (supertrait is ErrorType, no extra type needed)
impl Readable for MyTcpSocket {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

// ── Read/Write halves for TcpSplit ─────────────────────────────────────────────
//
// embassy_net's TcpSocket::split() returns anonymous types we can't name.
// We use raw pointers to give them stable names and satisfy TcpSplit's bounds.

struct MyTcpSocketRead<'a> {
    // Fix: embassy_net::tcp::TcpSocket
    socket: *mut TcpSocket<'static>,
    _phantom: core::marker::PhantomData<&'a mut TcpSocket<'static>>,
}

struct MyTcpSocketWrite<'a> {
    socket: *mut TcpSocket<'static>,
    _phantom: core::marker::PhantomData<&'a mut TcpSocket<'static>>,
}

impl ErrorType for MyTcpSocketRead<'_> { type Error = NetError; }

impl AsyncRead for MyTcpSocketRead<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // Safety: TcpSplit guarantees that only one half accesses the socket at a time.
        unsafe { &mut *self.socket }.read(buf).await.map_err(|_| NetError)
    }
}

// Fix: Readable must be implemented for the Read half to satisfy TcpSplit::Read's bound
impl Readable for MyTcpSocketRead<'_> {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

impl ErrorType for MyTcpSocketWrite<'_> { type Error = NetError; }

impl AsyncWrite for MyTcpSocketWrite<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        unsafe { &mut *self.socket }.write(buf).await.map_err(|_| NetError)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        unsafe { &mut *self.socket }.flush().await.map_err(|_| NetError)
    }
}

impl TcpSplit for MyTcpSocket {
    type Read<'a>  = MyTcpSocketRead<'a>  where Self: 'a;
    type Write<'a> = MyTcpSocketWrite<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        let ptr: *mut TcpSocket<'static> = &mut self.socket;
        (
            MyTcpSocketRead  { socket: ptr, _phantom: core::marker::PhantomData },
            MyTcpSocketWrite { socket: ptr, _phantom: core::marker::PhantomData },
        )
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

// Fix: TcpShutdown supertrait is ErrorType, so do NOT declare `type Error`.
// The method signature uses edge_nal::Close (the enum), not edge_nal::TcpClose.
impl TcpShutdown for MyTcpSocket {
    async fn close(&mut self, _what: edge_nal::Close) -> Result<(), Self::Error> {
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

    // Fix: trait signature is &mut [u8], not &mut [IpAddr]
    async fn get_host_by_address(
        &self,
        _addr: core::net::IpAddr,
        _result: &mut [u8],
    ) -> Result<usize, Self::Error> {
        Err(DnsError)
    }
}

impl TcpConnect for NetworkAdapter {
    type Error = NetError;
    // Fix: MyTcpSocket now implements TcpShutdown + Readable, satisfying all bounds
    type Socket<'m> = MyTcpSocket where Self: 'm;

    async fn connect(
        &self,
        remote: core::net::SocketAddr,
    ) -> Result<Self::Socket<'_>, Self::Error> {
        use core::net::IpAddr;

        let mut rx = self.rx_buf.borrow_mut();
        let mut tx = self.tx_buf.borrow_mut();

        let ip = match remote.ip() {
            IpAddr::V4(v4) =>
                espforge_platform::embassy_net::Ipv4Address::from_octets(v4.octets()),
            IpAddr::V6(_) => return Err(NetError),
        };

        // Fix: TcpSocket::new is in embassy_net::tcp
        let mut socket = TcpSocket::new(
            self.stack,
            unsafe { core::slice::from_raw_parts_mut(rx.as_mut_ptr(), rx.len()) },
            unsafe { core::slice::from_raw_parts_mut(tx.as_mut_ptr(), tx.len()) },
        );

        socket.connect((ip, remote.port())).await.map_err(|_| NetError)?;
        Ok(MyTcpSocket { socket })
    }
}

// ── WebSocket upgrade helpers ───────────────────────────────────────────────────

fn upgrade_request_headers<'a>(
    host: &'a str,
    _path: &'a str,
    nonce: &'a [u8],
    key_buf: &'a mut [u8; 28],
) -> heapless::Vec<(&'static str, &'a str), 8> {
    use base64::Engine;
    let mut v: heapless::Vec<(&'static str, &'a str), 8> = heapless::Vec::new();
    let n = base64::engine::general_purpose::STANDARD
        .encode_slice(nonce, key_buf)
        .unwrap_or(0);
    let key_str = core::str::from_utf8(&key_buf[..n]).unwrap_or("");
    // Note: we can't store &str slices from local stack easily; caller provides storage
    let _ = v.push(("Host", host));
    let _ = v.push(("Sec-WebSocket-Key", key_str));
    v
}

fn is_upgrade_accepted<'a>(
    nonce: &[u8],
    headers: impl Iterator<Item = (&'a str, &'a str)>,
) -> bool {
    use base64::Engine;
    use sha1::{Sha1, Digest};

    let mut key_b64 = [0u8; 28];
    let n = base64::engine::general_purpose::STANDARD
        .encode_slice(nonce, &mut key_b64)
        .unwrap_or(0);

    let mut hasher = Sha1::new();
    hasher.update(&key_b64[..n]);
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let expected = hasher.finalize();

    let mut expected_b64 = [0u8; 28];
    let m = base64::engine::general_purpose::STANDARD
        .encode_slice(&expected, &mut expected_b64)
        .unwrap_or(0);

    for (name, value) in headers {
        if name.eq_ignore_ascii_case("sec-websocket-accept") {
            return value.as_bytes() == &expected_b64[..m];
        }
    }
    false
}

// ── TLS trait object ────────────────────────────────────────────────────────────
//
// Fix: dyn TlsSocket needs the associated Error type specified because TlsSocket
// has a supertrait (ErrorType) with an associated type.
// We use a concrete error type (embedded_io_async::ErrorKind is not right either).
// The cleanest approach is to pick a concrete implementor and box it with a fixed
// error type, or define the trait alias with a concrete Error bound.

// We define TlsSocket with a specific error type to make it object-safe:
trait TlsSocket: embedded_io_async::Read<Error = TlsError>
               + embedded_io_async::Write<Error = TlsError> {}

// A concrete error type for the TLS socket path
#[derive(Debug)]
struct TlsError;
impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "TLS I/O error") }
}
impl core::error::Error for TlsError {}
impl embedded_io_async::Error for TlsError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

// ── WebSocketClient ─────────────────────────────────────────────────────────────

pub struct WebSocketClient {
    stack:       Stack<'static>,
    uri:         String<128>,
    socket:      Option<MyTcpSocket>,
    tls_socket:  Option<alloc::boxed::Box<dyn TlsSocket>>,
    resources:   WebSocketResources,
}

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
            socket: None,
            tls_socket: None,
            resources: WebSocketResources {
                rx_buf:      resources.rx_buf.take(),
                tx_buf:      resources.tx_buf.take(),
                payload_buf: resources.payload_buf.take(),
            },
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
        let mut socket = adapter.connect(remote).await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        let mut nonce = [0u8; 16];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        self.do_ws_upgrade_plain(&mut socket, &host, &path, &nonce).await?;
        self.socket = Some(socket);
        Ok(())
    }

    /// Connect over TLS using esp-mbedtls (mbedtls-rs crate).
    async fn connect_tls(
        &mut self,
        host: String<64>,
        port: u16,
        path: String<64>,
    ) -> Result<(), WebSocketError> {
        // Fix: mbedtls_rs::Mode does not exist. The Session::new() API from
        // esp-mbedtls/mbedtls-rs takes the socket, TlsVersion, Certificate, and hostname.
        // There is no Mode parameter — client mode is implied by the call signature.
        use mbedtls_rs::{Session, Certificate, TlsVersion};

        let rx = self.resources.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx = self.resources.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let adapter = NetworkAdapter::new(self.stack, rx, tx);

        let ip = edge_nal::Dns::get_host_by_name(&adapter, host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let remote = core::net::SocketAddr::new(ip, port);
        let tcp_socket = adapter.connect(remote).await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        // Fix: Certificate (not Certificates). Session::new() takes the TCP socket,
        // TlsVersion, Certificate, and hostname string. No Mode argument.
        let mut session = Session::new(
            tcp_socket,
            TlsVersion::Tls1_3,
            Certificate::new(),
            host.as_str(),
        )
        .map_err(|_| WebSocketError::TlsError)?;

        session.connect().await.map_err(|_| WebSocketError::TlsError)?;

        let mut nonce = [0u8; 16];
        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        self.do_ws_upgrade_tls(&mut session, &host, &path, &nonce).await?;

        // Fix: Box<dyn TlsSocket> requires TlsSocket's Error to be fixed.
        // Session implements Read+Write — wrap it via an adapter that maps errors.
        // Store as boxed trait object.
        self.tls_socket = Some(alloc::boxed::Box::new(TlsSessionWrapper { session }));
        Ok(())
    }

    // ── upgrade helpers ─────────────────────────────────────────────────────────

    async fn do_ws_upgrade_plain(
        &mut self,
        socket: &mut MyTcpSocket,
        host: &str,
        path: &str,
        nonce: &[u8],
    ) -> Result<(), WebSocketError> {
        self.send_upgrade_request(socket, host, path, nonce).await?;
        self.read_upgrade_response(socket, nonce).await
    }

    async fn do_ws_upgrade_tls<S>(
        &mut self,
        session: &mut S,
        host: &str,
        path: &str,
        nonce: &[u8],
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
        nonce: &[u8],
    ) -> Result<(), WebSocketError>
    where
        S: embedded_io_async::Write,
    {
        use embedded_io_async::Write;
        use base64::Engine;

        // Build request line
        let mut line: heapless::String<128> = heapless::String::new();
        core::fmt::write(&mut line, format_args!("GET {} HTTP/1.1\r\n", path))
            .map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(line.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;

        // Host header
        let mut hdr: heapless::String<128> = heapless::String::new();
        core::fmt::write(&mut hdr, format_args!("Host: {}\r\n", host))
            .map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(hdr.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;

        // Upgrade headers
        stream.write_all(b"Upgrade: websocket\r\n").await.map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(b"Connection: Upgrade\r\n").await.map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(b"Sec-WebSocket-Version: 13\r\n").await.map_err(|_| WebSocketError::SendFailed)?;

        // Sec-WebSocket-Key
        let mut key_buf = [0u8; 28];
        let n = base64::engine::general_purpose::STANDARD
            .encode_slice(nonce, &mut key_buf)
            .unwrap_or(0);
        let key_str = core::str::from_utf8(&key_buf[..n]).unwrap_or("");
        let mut key_hdr: heapless::String<64> = heapless::String::new();
        core::fmt::write(&mut key_hdr, format_args!("Sec-WebSocket-Key: {}\r\n", key_str))
            .map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(key_hdr.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;

        stream.write_all(b"\r\n").await.map_err(|_| WebSocketError::SendFailed)?;
        stream.flush().await.map_err(|_| WebSocketError::SendFailed)?;
        Ok(())
    }

    async fn read_upgrade_response<S>(
        &self,
        stream: &mut S,
        nonce: &[u8],
    ) -> Result<(), WebSocketError>
    where
        S: embedded_io_async::Read,
    {
        use embedded_io_async::Read;
        let mut resp_buf = [0u8; 1024];
        let mut total = 0usize;

        loop {
            let n = stream.read(&mut resp_buf[total..]).await
                .map_err(|_| WebSocketError::HandshakeFailed)?;
            total += n;
            if resp_buf[..total].windows(4).any(|w| w == b"\r\n\r\n") { break; }
            if total >= resp_buf.len() { break; }
        }

        let response = core::str::from_utf8(&resp_buf[..total])
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let status: u16 = response
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(WebSocketError::HandshakeFailed)?;

        if status != 101 {
            return Err(WebSocketError::HandshakeFailed);
        }

        let resp_headers: heapless::Vec<(&str, &str), 16> = response
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
        use edge_ws::{FrameHeader, FrameType};
        let header = FrameHeader {
            frame_type:  FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key:    Some(mask_key),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(&mut **session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut **session, text.as_bytes()).await
                .map_err(|_| WebSocketError::SendFailed)?;
            (**session).flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(socket, text.as_bytes()).await
                .map_err(|_| WebSocketError::SendFailed)?;
            socket.flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else {
            return Err(WebSocketError::SendFailed);
        }
        Ok(())
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();
        use edge_ws::{FrameHeader, FrameType};
        let header = FrameHeader {
            frame_type:  FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key:    Some(mask_key),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(&mut **session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut **session, data).await
                .map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(socket, data).await
                .map_err(|_| WebSocketError::SendFailed)?;
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
                let header = FrameHeader::recv($stream).await
                    .map_err(|_| WebSocketError::ReceiveFailed)?;
                let len = header.payload_len as usize;
                if len > buf.len() { return Err(WebSocketError::ReceiveFailed); }
                header.recv_payload($stream, &mut buf[..len]).await
                    .map_err(|_| WebSocketError::ReceiveFailed)?;
                match header.frame_type {
                    FrameType::Text(_)   => {
                        let s = core::str::from_utf8(&buf[..len])
                            .map_err(|_| WebSocketError::ProtocolError)?;
                        Ok(Some(Message::Text(s)))
                    }
                    FrameType::Binary(_) => Ok(Some(Message::Binary(&buf[..len]))),
                    FrameType::Ping      => Ok(Some(Message::Ping)),
                    FrameType::Pong      => Ok(Some(Message::Pong)),
                    FrameType::Close     => Ok(Some(Message::Close)),
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

// ── TLS session wrapper ─────────────────────────────────────────────────────────
//
// mbedtls_rs::Session implements embedded_io_async::Read and Write with its own
// error type. We wrap it to map the error to our concrete TlsError so it fits
// in Box<dyn TlsSocket> (which requires Error = TlsError).

struct TlsSessionWrapper<T>
where
    T: embedded_io_async::Read + embedded_io_async::Write + TcpShutdown,
{
    session: mbedtls_rs::Session<'static, T>,
}

impl<T> embedded_io_async::ErrorType for TlsSessionWrapper<T>
where
    T: embedded_io_async::Read + embedded_io_async::Write + TcpShutdown,
{
    type Error = TlsError;
}

impl<T> embedded_io_async::Read for TlsSessionWrapper<T>
where
    T: embedded_io_async::Read + embedded_io_async::Write + TcpShutdown,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TlsError> {
        use embedded_io_async::Read;
        self.session.read(buf).await.map_err(|_| TlsError)
    }
}

impl<T> embedded_io_async::Write for TlsSessionWrapper<T>
where
    T: embedded_io_async::Read + embedded_io_async::Write + TcpShutdown,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, TlsError> {
        use embedded_io_async::Write;
        self.session.write(buf).await.map_err(|_| TlsError)
    }
    async fn flush(&mut self) -> Result<(), TlsError> {
        use embedded_io_async::Write;
        self.session.flush().await.map_err(|_| TlsError)
    }
}

impl<T> TlsSocket for TlsSessionWrapper<T>
where
    T: embedded_io_async::Read + embedded_io_async::Write + TcpShutdown,
{}

