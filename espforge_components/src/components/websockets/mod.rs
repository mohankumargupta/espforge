extern crate alloc;

//use embedded_io_async::{Read, Write};
use core::fmt;
//use core::cell::RefCell;
use espforge_platform::embassy_net::Stack;
use heapless::String;
use embedded_io_async::{ErrorType, Read as AsyncRead, Write as AsyncWrite};
use edge_nal::{TcpSplit, TcpShutdown};

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close,
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
    NotConnected,
    DnsFailed,
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DnsFailed           => write!(f, "DNS lookup failed"),
            Self::DnsResolutionFailed => write!(f, "DNS resolution failed"),
            Self::ConnectionFailed    => write!(f, "Connection failed"),
            Self::HandshakeFailed     => write!(f, "WebSocket handshake failed"),
            Self::SendFailed          => write!(f, "Send failed"),
            Self::ReceiveFailed       => write!(f, "Receive failed"),
            Self::InvalidUri          => write!(f, "Invalid WebSocket URI"),
            Self::TlsError            => write!(f, "TLS error"),
            Self::ProtocolError       => write!(f, "WebSocket protocol error"),
            Self::UnexpectedFrame     => write!(f, "Unexpected WebSocket frame type"),
            Self::NotConnected        => write!(f, "Client is not connected"),
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

// struct DnsError;

// impl fmt::Display for DnsError {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DNS error") }
// }

// impl fmt::Debug for DnsError {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DnsError") }
// }

// impl core::error::Error for DnsError {}

// impl embedded_io_async::Error for DnsError {
//     fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
// }

#[derive(Debug)]
pub struct NetError;

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Network error") }
}

impl core::error::Error for NetError {}

impl embedded_io_async::Error for NetError {
    fn kind(&self) -> embedded_io_async::ErrorKind { embedded_io_async::ErrorKind::Other }
}

pub struct TcpSocket {
    socket: espforge_platform::embassy_net::tcp::TcpSocket<'static>,
}

impl ErrorType for TcpSocket {
    type Error = NetError;
}

impl edge_nal::Readable for TcpSocket {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

pub struct TcpSocketRead<'a> {
    socket: *mut espforge_platform::embassy_net::tcp::TcpSocket<'static>,
    _phantom: core::marker::PhantomData<&'a ()>,
}

pub struct TcpSocketWrite<'a> {
    socket: *mut espforge_platform::embassy_net::tcp::TcpSocket<'static>,
    _phantom: core::marker::PhantomData<&'a ()>,
}

impl ErrorType for TcpSocketRead<'_> { type Error = NetError; }
impl AsyncRead for TcpSocketRead<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        unsafe { &mut *self.socket }.read(buf).await.map_err(|_| NetError)
    }
}

impl edge_nal::Readable for TcpSocketRead<'_> {
    async fn readable(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

impl ErrorType for TcpSocketWrite<'_> { type Error = NetError; }
impl AsyncWrite for TcpSocketWrite<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        unsafe { &mut *self.socket }.write(buf).await.map_err(|_| NetError)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        unsafe { &mut *self.socket }.flush().await.map_err(|_| NetError)
    }
}

impl TcpSplit for TcpSocket {
    type Read<'a> = TcpSocketRead<'a> where Self: 'a;
    type Write<'a> = TcpSocketWrite<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        let ptr = &mut self.socket as *mut _;
        (
            TcpSocketRead  { socket: ptr, _phantom: core::marker::PhantomData },
            TcpSocketWrite { socket: ptr, _phantom: core::marker::PhantomData },
        )
    }
}

impl AsyncRead for TcpSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.socket.read(buf).await.map_err(|_| NetError)
    }
}

impl AsyncWrite for TcpSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.socket.write(buf).await.map_err(|_| NetError)
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.socket.flush().await.map_err(|_| NetError)
    }
}

impl TcpShutdown for TcpSocket {
    async fn close(&mut self, _behavior: edge_nal::Close) -> Result<(), Self::Error> {
        self.socket.close();
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        self.socket.abort();
        Ok(())
    }
}



// ── WebSocket upgrade helpers ───────────────────────────────────────────────────

fn upgrade_request_headers<'a>(
    host: &'a str,
    _path: &'a str,
    nonce: &'a [u8],
) -> heapless::Vec<(&'static str, heapless::String<64>), 8> {
    use base64::Engine;
    let mut key_buf = [0u8; 28];
    let mut v: heapless::Vec<(&'static str, heapless::String<64>), 8> = heapless::Vec::new();

    let _ = base64::engine::general_purpose::STANDARD
        .encode_slice(nonce, &mut key_buf)
        .ok();
    let key_str = core::str::from_utf8(&key_buf).unwrap_or("");
    let mut key_heapless: heapless::String<64> = heapless::String::new();
    let _ = key_heapless.push_str(key_str);

    let mut host_heapless: heapless::String<64> = heapless::String::new();
    let _ = host_heapless.push_str(host);

    let _ = v.push(("Host", host_heapless));
    let _ = v.push(("Upgrade", { let mut s = heapless::String::new(); let _ = s.push_str("websocket"); s }));
    let _ = v.push(("Connection", { let mut s = heapless::String::new(); let _ = s.push_str("Upgrade"); s }));
    let _ = v.push(("Sec-WebSocket-Key", key_heapless));
    let _ = v.push(("Sec-WebSocket-Version", { let mut s = heapless::String::new(); let _ = s.push_str("13"); s }));
    v
}

fn is_upgrade_accepted<'a>(
    nonce: &[u8],
    headers: impl Iterator<Item = (&'a str, &'a str)>,
) -> bool {
    use base64::Engine;
    use sha1::{Sha1, Digest};

    let mut key_b64 = [0u8; 28];
    let _ = base64::engine::general_purpose::STANDARD.encode_slice(nonce, &mut key_b64);

    let mut hasher = Sha1::new();
    hasher.update(&key_b64);
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let expected = hasher.finalize();

    let mut expected_b64 = [0u8; 28];
    let _ = base64::engine::general_purpose::STANDARD.encode_slice(&expected, &mut expected_b64);

    for (name, value) in headers {
        if name.eq_ignore_ascii_case("sec-websocket-accept") {
            return value.as_bytes() == &expected_b64[..];
        }
    }
    false
}

// ── WebSocketClient ─────────────────────────────────────────────────────────────

pub struct WebSocketClient<'a>
{
    stack:       Stack<'static>,
    uri:         String<128>,
    socket:      Option<TcpSocket>,
    tls_socket:  Option<alloc::boxed::Box<mbedtls_rs::Session<'a, TcpSocket>>>, 
    rx_buf:      Option<&'static mut [u8; 1536]>,
    tx_buf:      Option<&'static mut [u8; 1536]>,
    tls:         Option<mbedtls_rs::TlsReference<'a>>,

}

impl<'a> WebSocketClient<'a>
{
    pub fn new(
        stack: Stack<'static>,
        uri: &str,
        resources: &'static mut WebSocketResources,
        tls: Option<mbedtls_rs::TlsReference<'a>>,
    ) -> Self {
        let mut s: String<128> = String::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            uri: s,
            socket: None,
            tls_socket: None,
            rx_buf: resources.rx_buf.as_mut().map(|b| b as &'static mut _),
            tx_buf: resources.tx_buf.as_mut().map(|b| b as &'static mut _),
            tls,
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
        // 1. Parse the host domain and target port out of the client URI configuration
        let (host, port, path, is_wss) = self.parse_uri()?;
        
         if is_wss && self.tls.is_none() {
            return Err(WebSocketError::TlsError);
        }
        
        // 1. Resolve DNS natively with Embassy
        let addrs = self.stack
            .dns_query(&host, espforge_platform::embassy_net::dns::DnsQueryType::A)
            .await
            .map_err(|_| WebSocketError::DnsFailed)?;
            
        let remote_ip = *addrs.first().ok_or(WebSocketError::DnsFailed)?;
        let endpoint = espforge_platform::embassy_net::IpEndpoint::new(remote_ip, port);

        // 2. Extract static buffers safely
        let rx_buf = self.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
        let tx_buf = self.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

        // 3. Establish the raw TCP socket
        let mut emb_socket = espforge_platform::embassy_net::tcp::TcpSocket::new(
            self.stack, 
            rx_buf, 
            tx_buf
        );
        
        emb_socket.connect(endpoint).await.map_err(|_| WebSocketError::ConnectionFailed)?;
        let mut raw_socket = TcpSocket { socket: emb_socket };

        // 4. Generate upgrade nonce
         let nonce = [0u8; 16];

        // 5. Multiplex WSS vs WS
        if is_wss {
            let ctx = tls_ctx.ok_or(WebSocketError::TlsError)?;
            let session_config = mbedtls_rs::SessionConfig::Client(Default::default()); 
            
            let session = mbedtls_rs::Session::new(ctx, raw_socket, &session_config)
                .map_err(|_| WebSocketError::TlsError)?;
            
            let mut boxed_session = alloc::boxed::Box::new(session);
            boxed_session.connect().await.map_err(|_| WebSocketError::TlsError)?;

            self.do_ws_upgrade_tls(&mut *boxed_session, &host, &path, &nonce).await?;
            self.tls_socket = Some(boxed_session);
        } else {
            self.do_ws_upgrade_plain(&mut raw_socket, &host, &path, &nonce).await?;
            self.socket = Some(raw_socket);
        }
         Ok(())
}



//     async fn connect_plain(
//         &mut self,
//         host: String<64>,
//         port: u16,
//         path: String<64>,
//     ) -> Result<(), WebSocketError> {
//         let rx = self.resources.rx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;
//         let tx = self.resources.tx_buf.take().ok_or(WebSocketError::ConnectionFailed)?;

//         let adapter = NetworkAdapter::new(self.stack, rx, tx);

//         let ip = edge_nal::Dns::get_host_by_name(&adapter, host.as_str(), AddrType::IPv4)
//             .await
//             .map_err(|_| WebSocketError::DnsResolutionFailed)?;

//         let remote: core::net::SocketAddr = core::net::SocketAddr::new(ip, port);
//         let mut socket = adapter.connect(remote).await.map_err(|_| WebSocketError::ConnectionFailed)?;

//         let mut nonce = [0u8; 16];
//         unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

//         self.do_ws_upgrade_plain(&mut socket, &host, &path, &nonce).await?;
//         self.socket = Some(socket);
//         Ok(())
//     }

// // FIX: Remove the generic type parameter <T> and accept TcpSocket directly
//     pub async fn connect_tls(
//         &mut self,
//         raw_socket: TcpSocket, 
//         tls_ctx: mbedtls_rs::TlsReference<'a>,
//     ) -> Result<(), WebSocketError> {
//         // FIX: Construct the variant using its default inner configuration
//         let session_config = mbedtls_rs::SessionConfig::Client(Default::default()); 
        
//         // Instantiate the session
//         let session = mbedtls_rs::Session::new(
//             tls_ctx,
//             raw_socket,
//             &session_config,
//         )
//         .map_err(|_| WebSocketError::TlsError)?;

//         // Wrap the session object up inside our target Box type
//         let mut boxed_session = alloc::boxed::Box::new(session);

//         // Establish the encrypted TLS session handshake
//         boxed_session.connect().await.map_err(|_| WebSocketError::TlsError)?;

//         // Safely store the active session inside the client state context
//         // This will now match Box<Session<'_, TcpSocket>> perfectly!
//         self.tls_socket = Some(boxed_session);
//         Ok(())
//     }
    

    async fn do_ws_upgrade_plain(
        &mut self,
        socket: &mut TcpSocket,
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
        let mut line: heapless::String<128> = heapless::String::new();
        core::fmt::write(&mut line, format_args!("GET {} HTTP/1.1\r\n", path))
            .map_err(|_| WebSocketError::SendFailed)?;
        stream.write_all(line.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;

        let headers = upgrade_request_headers(host, path, nonce);
        for (name, value) in headers.iter() {
            let mut hdr: heapless::String<128> = heapless::String::new();
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
        nonce: &[u8],
    ) -> Result<(), WebSocketError>
    where
        S: embedded_io_async::Read,
    {
        let mut resp_buf = [0u8; 1024];
        let mut total = 0usize;

        loop {
            let n = stream.read(&mut resp_buf[total..]).await.map_err(|_| WebSocketError::HandshakeFailed)?;
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

// ── send / receive ──────────────────────────────────────────────────────────

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();
        use edge_ws::{FrameHeader, FrameType};

        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key: Some(mask_key),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(&mut **session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut **session, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
            (**session).flush().await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            // FIX: Reborrow via &mut *socket so ownership doesn't permanently move on step 1
            header.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut *socket, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
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
            frame_type: FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key: Some(mask_key),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(&mut **session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut **session, data).await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            // FIX: Reborrow via &mut *socket here as well
            header.send(&mut *socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(&mut *socket, data).await.map_err(|_| WebSocketError::SendFailed)?;
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
                let header = FrameHeader::recv($stream).await.map_err(|_| WebSocketError::ReceiveFailed)?;
                let len = header.payload_len as usize;
                if len > buf.len() { return Err(WebSocketError::ReceiveFailed); }
                header.recv_payload($stream, &mut buf[..len]).await.map_err(|_| WebSocketError::ReceiveFailed)?;
                match header.frame_type {
                    FrameType::Text(_) => {
                        let s = core::str::from_utf8(&buf[..len]).map_err(|_| WebSocketError::ProtocolError)?;
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
            // FIX: Reborrow the raw socket into the macro call expansion
            recv_from!(&mut *socket)
        } else {
            Err(WebSocketError::ReceiveFailed)
        }
    }

}

