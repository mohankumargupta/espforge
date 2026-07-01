pub use edge_nal_tls::mbedtls::Tls;

use core::ffi::CStr;
use core::fmt;
use core::net::SocketAddr;

use edge_http::io::client::Connection;
use edge_http::ws::{MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN};
use edge_nal::{AddrType, Dns};
use edge_nal_embassy::{Dns as EmbassyDns, Tcp, TcpBuffers, TcpSocket};
use edge_nal_tls::mbedtls::{AuthMode, ClientSessionConfig, TlsReference};
use edge_nal_tls::{TlsConnector, TlsSocket};
use edge_ws::{FrameHeader, FrameType};

use embedded_io_async::{Read, Write};

use espforge_platform::embassy_net::Stack;

use heapless::String;
use rand_core::{TryCryptoRng, TryRng};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
const IO_BUFFER_SIZE: usize = 4096;
const FRAME_BUFFER_SIZE: usize = 2048;
const HOSTNAME_SIZE: usize = 128;

// -----------------------------------------------------------------------------
// Public Types
// -----------------------------------------------------------------------------
pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close,
}

#[derive(Debug)]
pub enum WebSocketError {
    InvalidUri,
    InvalidResponse,
    DnsFailed,
    ConnectFailed,
    HandshakeFailed,
    NotConnected,
    TlsError,
    Io,
    Protocol,
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebSocketError::InvalidUri => write!(f, "invalid WebSocket URI"),
            WebSocketError::InvalidResponse => write!(f, "invalid WebSocket upgrade response"),
            WebSocketError::DnsFailed => write!(f, "WebSocket DNS lookup failed"),
            WebSocketError::ConnectFailed => write!(f, "WebSocket TCP connection failed"),
            WebSocketError::HandshakeFailed => write!(f, "WebSocket upgrade handshake failed"),
            WebSocketError::NotConnected => write!(f, "WebSocket is not connected"),
            WebSocketError::TlsError => write!(f, "WebSocket TLS setup failed"),
            WebSocketError::Io => write!(f, "WebSocket I/O error"),
            WebSocketError::Protocol => write!(f, "WebSocket protocol error"),
        }
    }
}

// -----------------------------------------------------------------------------
// RNG Helper
// -----------------------------------------------------------------------------
pub struct TlsRng {
    rng: espforge_platform::rng::Rng,
}

impl TlsRng {
    pub fn new(rng: espforge_platform::rng::Rng) -> Self {
        Self { rng }
    }
}

impl TryRng for TlsRng {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.rng.random_u32())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let hi = self.rng.random_u32() as u64;
        let lo = self.rng.random_u32() as u64;
        Ok((hi << 32) | lo)
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        self.rng.fill_bytes(dst);
        Ok(())
    }
}
impl TryCryptoRng for TlsRng {}

// -----------------------------------------------------------------------------
// Static Storage Resources
// -----------------------------------------------------------------------------
pub struct WebSocketResources {
    pub io_buffer: [u8; IO_BUFFER_SIZE],
    pub frame_buffer: [u8; FRAME_BUFFER_SIZE],
    pub hostname: [u8; HOSTNAME_SIZE],
    pub tcp_buffers: TcpBuffers<1, IO_BUFFER_SIZE, IO_BUFFER_SIZE>,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            io_buffer: [0; IO_BUFFER_SIZE],
            frame_buffer: [0; FRAME_BUFFER_SIZE],
            hostname: [0; HOSTNAME_SIZE],
            tcp_buffers: TcpBuffers::new(),
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

struct Uri {
    secure: bool,
    host: String<64>,
    port: u16,
    path: String<128>,
}

fn server_name_cstr<'b>(
    host: &str,
    buf: &'b mut [u8; HOSTNAME_SIZE],
) -> Result<&'b CStr, WebSocketError> {
    if host.len() >= buf.len() {
        return Err(WebSocketError::InvalidUri);
    }
    buf.fill(0);
    buf[..host.len()].copy_from_slice(host.as_bytes());
    CStr::from_bytes_until_nul(buf).map_err(|_| WebSocketError::InvalidUri)
}

// -----------------------------------------------------------------------------
// Concrete Transport Enum
// -----------------------------------------------------------------------------
pub enum Transport<'a> {
    Plain(TcpSocket<'a>),
    Tls(TlsSocket<'a, TcpSocket<'a>>),
}

// -----------------------------------------------------------------------------
// Frame I/O helpers (shared by both transports so we don't duplicate match arms)
// -----------------------------------------------------------------------------
async fn send_frame_on<W: Write>(
    socket: &mut W,
    header: &FrameHeader,
    payload: &[u8],
) -> Result<(), WebSocketError> {
    header
        .send(&mut *socket)
        .await
        .map_err(|_| WebSocketError::Io)?;
    header
        .send_payload(&mut *socket, payload)
        .await
        .map_err(|_| WebSocketError::Io)?;
    Ok(())
}

async fn recv_frame_on<'b, R: Read>(
    socket: &mut R,
    buffer: &'b mut [u8],
) -> Result<(FrameType, &'b [u8]), WebSocketError> {
    let header = FrameHeader::recv(&mut *socket)
        .await
        .map_err(|_| WebSocketError::Io)?;
    let payload = header
        .recv_payload(&mut *socket, buffer)
        .await
        .map_err(|_| WebSocketError::Io)?;
    Ok((header.frame_type, payload))
}

// -----------------------------------------------------------------------------
// Session (the decoupled active handle)
// -----------------------------------------------------------------------------
pub struct WebSocketSession<'a> {
    transport: Transport<'a>,
    frame_buffer: &'a mut [u8],
    rng: espforge_platform::rng::Rng,
}

impl<'a> WebSocketSession<'a> {
    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        self.send_frame(FrameType::Text(true), text.as_bytes())
            .await
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        self.send_frame(FrameType::Binary(true), data).await
    }

    async fn send_frame(
        &mut self,
        frame_type: FrameType,
        payload: &[u8],
    ) -> Result<(), WebSocketError> {
        // Client-to-server frames MUST be masked per RFC 6455 §5.1.
        let header = FrameHeader {
            frame_type,
            payload_len: payload.len() as u64,
            mask_key: Some(self.rng.random_u32()),
        };

        match &mut self.transport {
            Transport::Plain(socket) => send_frame_on(socket, &header, payload).await,
            Transport::Tls(socket) => send_frame_on(socket, &header, payload).await,
        }
    }

    /// Reads a single frame into `buffer` and returns it as a [`Message`].
    ///
    /// Note: this does not yet auto-reply to Ping frames or reassemble
    /// fragmented (`Continue`) frames.
    pub async fn receive<'b>(
        &mut self,
        buffer: &'b mut [u8],
    ) -> Result<Message<'b>, WebSocketError> {
        let (frame_type, payload) = self.recv_frame(buffer).await?;
        match frame_type {
            FrameType::Text(_) => {
                let text = core::str::from_utf8(payload).map_err(|_| WebSocketError::Protocol)?;
                Ok(Message::Text(text))
            }
            FrameType::Binary(_) => Ok(Message::Binary(payload)),
            FrameType::Ping => Ok(Message::Ping),
            FrameType::Pong => Ok(Message::Pong),
            FrameType::Close => Ok(Message::Close),
            FrameType::Continue(_) => Err(WebSocketError::Protocol),
        }
    }

    async fn recv_frame<'b>(
        &mut self,
        buffer: &'b mut [u8],
    ) -> Result<(FrameType, &'b [u8]), WebSocketError> {
        match &mut self.transport {
            Transport::Plain(socket) => recv_frame_on(socket, buffer).await,
            Transport::Tls(socket) => recv_frame_on(socket, buffer).await,
        }
    }

    /// Sends a Close frame and makes a best-effort attempt to drain the
    /// server's close acknowledgement so the connection tears down cleanly.
    pub async fn close(&mut self) -> Result<(), WebSocketError> {
        self.send_frame(FrameType::Close, &[]).await?;

        let frame_buffer: &mut [u8] = &mut *self.frame_buffer;
        let _ = match &mut self.transport {
            Transport::Plain(socket) => recv_frame_on(socket, frame_buffer).await,
            Transport::Tls(socket) => recv_frame_on(socket, frame_buffer).await,
        };

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// SessionContext – owns the transports for the duration of a session.
// -----------------------------------------------------------------------------
pub struct SessionContext<'a> {
    pub tls: Option<TlsConnector<'a, Tcp<'a>>>,
    pub plain_tcp: Option<Tcp<'a>>,
}

// -----------------------------------------------------------------------------
// Connector Factory
// -----------------------------------------------------------------------------
pub struct WebSocketConnector {
    stack: Stack<'static>,
    resources: &'static mut WebSocketResources,
}

impl WebSocketConnector {
    pub fn new(stack: Stack<'static>, resources: &'static mut WebSocketResources) -> Self {
        Self { stack, resources }
    }

    fn parse_uri(&self, uri_str: &str) -> Result<Uri, WebSocketError> {
        let (secure, rest) = if let Some(r) = uri_str.strip_prefix("wss://") {
            (true, r)
        } else if let Some(r) = uri_str.strip_prefix("ws://") {
            (false, r)
        } else {
            return Err(WebSocketError::InvalidUri);
        };

        if rest.is_empty() {
            return Err(WebSocketError::InvalidUri);
        }

        let (host_port, path_raw) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };

        if host_port.is_empty() {
            return Err(WebSocketError::InvalidUri);
        }

        let (host_raw, port) = match host_port.rfind(':') {
            Some(idx) => {
                let port_str = &host_port[idx + 1..];
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| WebSocketError::InvalidUri)?;
                (&host_port[..idx], port)
            }
            None => (host_port, if secure { 443 } else { 80 }),
        };

        let mut host = String::new();
        host.push_str(host_raw)
            .map_err(|_| WebSocketError::InvalidUri)?;

        let mut path = String::new();
        path.push_str(path_raw)
            .map_err(|_| WebSocketError::InvalidUri)?;

        Ok(Uri {
            secure,
            host,
            port,
            path,
        })
    }

    /// Open a WebSocket connection.
    ///
    /// The caller must provide a `SessionContext` that will own the transport(s)
    /// for the whole lifetime of the returned `WebSocketSession`.  Both the
    /// session and the context share the same lifetime `'a`.
    pub async fn connect<'a>(
        &'a mut self,
        uri_str: &str,
        tls_ref: Option<TlsReference<'a>>,
        ctx: &'a mut SessionContext<'a>,
    ) -> Result<WebSocketSession<'a>, WebSocketError> {
        let uri = self.parse_uri(uri_str)?;

        let dns = EmbassyDns::new(self.stack);
        let ip = dns
            .get_host_by_name(uri.host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsFailed)?;
        let addr = SocketAddr::new(ip, uri.port);

        let rng = unsafe { espforge_platform::rng::Rng::new() };

        if uri.secure {
            let tls = tls_ref.ok_or(WebSocketError::TlsError)?;

            // hostname buffer is only borrowed for this block
            let server_name = server_name_cstr(uri.host.as_str(), &mut self.resources.hostname)?;
            let config = ClientSessionConfig {
                ca_chain: None,
                server_name: Some(server_name),
                auth_mode: AuthMode::None,
                ..Default::default()
            };
            //could try  ..ClientSessionConfig::new()

            let tcp = Tcp::new(self.stack, &self.resources.tcp_buffers);
            let tls_connector = TlsConnector::new(tls, tcp, &config);

            // Store the connector in the caller‑provided context – it lives for 'a
            ctx.tls = Some(tls_connector);
            let tls_conn = ctx.tls.as_ref().unwrap();

            let mut conn = Connection::new(&mut self.resources.io_buffer, tls_conn, addr);
            perform_handshake(&uri, &mut conn).await?;

            let (socket, _) = conn.release();
            Ok(WebSocketSession {
                transport: Transport::Tls(socket),
                frame_buffer: &mut self.resources.frame_buffer,
                rng,
            })
        } else {
            let tcp = Tcp::new(self.stack, &self.resources.tcp_buffers);

            // Store the plain Tcp in the context
            ctx.plain_tcp = Some(tcp);
            let tcp_ref = ctx.plain_tcp.as_ref().unwrap();

            let mut conn = Connection::new(&mut self.resources.io_buffer, tcp_ref, addr);
            perform_handshake(&uri, &mut conn).await?;

            let (socket, _) = conn.release();
            Ok(WebSocketSession {
                transport: Transport::Plain(socket),
                frame_buffer: &mut self.resources.frame_buffer,
                rng,
            })
        }
    }
}

// -----------------------------------------------------------------------------
// Free function – no borrow on self, eliminates E0502.
// -----------------------------------------------------------------------------
async fn perform_handshake<B>(uri: &Uri, conn: &mut Connection<'_, B>) -> Result<(), WebSocketError>
where
    B: edge_nal::TcpConnect,
{
espforge_platform::logger::Logger::new().info("ENTERED perform_handshake - build v2");

    let mut rng = TlsRng::new(unsafe { espforge_platform::rng::Rng::new() });
    let mut nonce = [0u8; NONCE_LEN];
    rng.try_fill_bytes(&mut nonce)
        .map_err(|_| WebSocketError::HandshakeFailed)?;

    let mut key_buf = [0_u8; MAX_BASE64_KEY_LEN];

    conn.initiate_ws_upgrade_request(
        Some(uri.host.as_str()),
        None,
        uri.path.as_str(),
        None,
        &nonce,
        &mut key_buf,
    )
    .await
    .map_err(|e| {
        espforge_platform::logger::Logger::new()
        .info(format_args!("ws upgrade request error: {:?}", e));    
        WebSocketError::HandshakeFailed
    })?;

    conn.initiate_response()
        .await
        .map_err(|e| {
        espforge_platform::logger::Logger::new()
            .info(format_args!("initiate_response error: {:?}", e));        
            WebSocketError::HandshakeFailed
        })?;

    let mut resp_buf = [0_u8; MAX_BASE64_KEY_RESPONSE_LEN];
    if !conn
        .is_ws_upgrade_accepted(&nonce, &mut resp_buf)
        .map_err(|e| {
            espforge_platform::logger::Logger::new()
            .info(format_args!("is_ws_upgrade_accepted error: {:?}", e));        
            WebSocketError::InvalidResponse
        })?
    {
        return Err(WebSocketError::HandshakeFailed);
    }

    conn.complete()
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;
    Ok(())
}
