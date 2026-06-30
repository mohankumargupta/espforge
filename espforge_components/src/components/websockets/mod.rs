extern crate alloc;

use core::fmt;
use espforge_platform::embassy_net::Stack;
use heapless::String;
use core::net::SocketAddr;
use edge_http::io::client::Connection;
use edge_http::ws::{MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN};
use edge_nal::{AddrType, Dns, TcpConnect};
use edge_nal_embassy::{Dns as EmbassyDns, Tcp, TcpBuffers, TcpConnection};
use edge_nal_tls::mbedtls::{AuthMode, ClientSessionConfig, TlsConnection};
use edge_nal_tls::TlsConnector;
use edge_ws::{FrameHeader, FrameType};
use embedded_io_async::{Read, Write};

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close,
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
    TlsError,
    ProtocolError,
    UnexpectedFrame,
    NotConnected,
    DnsFailed,
    HostTooLong,
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DnsFailed => write!(f, "DNS lookup failed"),
            Self::DnsResolutionFailed => write!(f, "DNS resolution failed"),
            Self::ConnectionFailed => write!(f, "Connection failed"),
            Self::HandshakeFailed => write!(f, "WebSocket handshake failed"),
            Self::SendFailed => write!(f, "Send failed"),
            Self::ReceiveFailed => write!(f, "Receive failed"),
            Self::InvalidUri => write!(f, "Invalid WebSocket URI"),
            Self::TlsError => write!(f, "TLS error"),
            Self::ProtocolError => write!(f, "WebSocket protocol error"),
            Self::UnexpectedFrame => write!(f, "Unexpected WebSocket frame type"),
            Self::NotConnected => write!(f, "Client is not connected"),
            Self::HostTooLong => write!(f, "Host name too long"),
        }
    }
}

// ── TLS RNG Wrapper ────────────────────────────────────────────────────────────
pub struct TlsRng(pub espforge_platform::rng::Rng);

impl rand_core::TryRng for TlsRng {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.0.random_u32())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let hi = self.0.random_u32() as u64;
        let lo = self.0.random_u32() as u64;
        Ok((hi << 32) | lo)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        self.0.fill_bytes(dest);
        Ok(())
    }
}

impl rand_core::TryCryptoRng for TlsRng {}

// ── WebSocketResources ─────────────────────────────────────────────────────────

pub struct WebSocketResources {
    pub io_buf: [u8; 1536],
    pub tcp_buffers: TcpBuffers<1, 1536, 1536>,
    pub server_name_buf: [u8; 129],
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            io_buf: [0u8; 1536],
            tcp_buffers: TcpBuffers::new(),
            server_name_buf: [0u8; 129],
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

fn server_name_cstr<'a>(host: &str, buf: &'a mut [u8; 129]) -> Result<&'a core::ffi::CStr, WebSocketError> {
    if host.len() >= buf.len() {
        return Err(WebSocketError::HostTooLong);
    }
    buf.fill(0);
    buf[..host.len()].copy_from_slice(host.as_bytes());
    core::ffi::CStr::from_bytes_until_nul(buf).map_err(|_| WebSocketError::InvalidUri)
}
// ── WebSocketClient ─────────────────────────────────────────────────────────────

pub struct WebSocketClient<'a> {
    socket: Option<TcpConnection<'static, 1, 1536, 1536>>,
    tls_socket: Option<TlsConnection<'a, TcpConnection<'static, 1, 1536, 1536>>>,
    resources: &'static mut WebSocketResources,
    tls: Option<mbedtls_rs::TlsReference<'a>>,
}

impl<'a> WebSocketClient<'a> {
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
            resources,
            tls,
        }
    }

    pub fn has_tls(&self) -> bool {
        self.tls.is_some()
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
            None => (host_port, if is_wss { 443 } else { 80 }),
        };

        let mut host: String<64> = String::new();
        let mut path: String<64> = String::new();
        host.push_str(host_raw)
            .map_err(|_| WebSocketError::InvalidUri)?;
        path.push_str(path_raw)
            .map_err(|_| WebSocketError::InvalidUri)?;

        Ok((host, port, path, is_wss))
    }

    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        self.socket = None;
        self.tls_socket = None;

        let (host, port, path, is_wss) = self.parse_uri()?;

        if is_wss && self.tls.is_none() {
            return Err(WebSocketError::TlsError);
        }

        let dns = EmbassyDns::new(self.stack);
        let ip = dns
            .get_host_by_name(host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsFailed)?;

        let addr = SocketAddr::new(ip, port);
        let tcp = Tcp::new(self.stack, &self.resources.tcp_buffers);

        let mut nonce = [0_u8; NONCE_LEN];        

        unsafe { espforge_platform::rng::Rng::new() }.fill_bytes(&mut nonce);

        let host_str = host.as_str();
        let path_str = path.as_str();

        if is_wss {
            let tls_ref = self.tls.ok_or(WebSocketError::TlsError)?;
            let server_name = server_name_cstr(host_str, &mut self.resources.server_name_buf)?;
            let config = ClientSessionConfig {
                server_name: Some(server_name),
                auth_mode: AuthMode::None,
                ..ClientSessionConfig::new()
            };
            let connector = TlsConnector::new(tls_ref, tcp, &config);

            let mut conn = Connection::<_, 16>::new(&mut self.resources.io_buf, &connector, addr);

            let mut buf = [0_u8; MAX_BASE64_KEY_LEN];
            conn.initiate_ws_upgrade_request(Some(host_str), Some("foo.com"), path_str, None, &nonce, &mut buf)
                .await.map_err(|_| WebSocketError::HandshakeFailed)?;
            
            conn.initiate_response().await.map_err(|_| WebSocketError::HandshakeFailed)?;

            let mut buf2 = [0_u8; MAX_BASE64_KEY_RESPONSE_LEN];
            if !conn.is_ws_upgrade_accepted(&nonce, &mut buf2).map_err(|_| WebSocketError::HandshakeFailed)? {
                return Err(WebSocketError::HandshakeFailed);
            }

            conn.complete().await.map_err(|_| WebSocketError::HandshakeFailed)?;

            let (socket, _) = conn.release();
            self.tls_socket = Some(socket);
        } else {
            let mut conn = Connection::<_, 16>::new(&mut self.resources.io_buf, &tcp, addr);

            let mut buf = [0_u8; MAX_BASE64_KEY_LEN];
            conn.initiate_ws_upgrade_request(Some(host_str), Some("foo.com"), path_str, None, &nonce, &mut buf)
                .await.map_err(|_| WebSocketError::HandshakeFailed)?;
            
            conn.initiate_response().await.map_err(|_| WebSocketError::HandshakeFailed)?;

            let mut buf2 = [0_u8; MAX_BASE64_KEY_RESPONSE_LEN];
            if !conn.is_ws_upgrade_accepted(&nonce, &mut buf2).map_err(|_| WebSocketError::HandshakeFailed)? {
                return Err(WebSocketError::HandshakeFailed);
            }

            conn.complete().await.map_err(|_| WebSocketError::HandshakeFailed)?;

            let (socket, _) = conn.release();
            self.socket = Some(socket);

        }
        Ok(())
    }


    // async fn do_ws_upgrade_plain(
    //     &mut self,
    //     socket: &mut TcpSocket,
    //     host: &str,
    //     path: &str,
    //     nonce: &[u8],
    // ) -> Result<(), WebSocketError> {
    //     self.send_upgrade_request(socket, host, path, nonce).await?;
    //     self.read_upgrade_response(socket, nonce).await
    // }

    // async fn do_ws_upgrade_tls<S>(
    //     &mut self,
    //     session: &mut S,
    //     host: &str,
    //     path: &str,
    //     nonce: &[u8],
    // ) -> Result<(), WebSocketError>
    // where
    //     S: embedded_io_async::Read + embedded_io_async::Write,
    // {
    //     self.send_upgrade_request(session, host, path, nonce)
    //         .await?;
    //     self.read_upgrade_response(session, nonce).await
    // }

    // async fn send_upgrade_request<S>(
    //     &self,
    //     stream: &mut S,
    //     host: &str,
    //     path: &str,
    //     nonce: &[u8],
    // ) -> Result<(), WebSocketError>
    // where
    //     S: embedded_io_async::Write,
    // {
    //     let mut line: heapless::String<128> = heapless::String::new();
    //     core::fmt::write(&mut line, format_args!("GET {} HTTP/1.1\r\n", path))
    //         .map_err(|_| WebSocketError::SendFailed)?;
    //     let logger = espforge_platform::logger::Logger::new();
    //     logger.info("Sending:");
    //     logger.info(line.as_str());
    //     stream
    //         .write_all(line.as_bytes())
    //         .await
    //         .map_err(|_| WebSocketError::SendFailed)?;

    //     let headers = upgrade_request_headers(host, path, nonce);
    //     for (name, value) in headers.iter() {
    //         let mut hdr: heapless::String<128> = heapless::String::new();
    //         core::fmt::write(&mut hdr, format_args!("{}: {}\r\n", name, value))
    //             .map_err(|_| WebSocketError::SendFailed)?;
    //         logger.info("Headers:");
    //         logger.info(hdr.as_str());

    //         stream
    //             .write_all(hdr.as_bytes())
    //             .await
    //             .map_err(|_| WebSocketError::SendFailed)?;
    //     }

    //     stream
    //         .write_all(b"\r\n")
    //         .await
    //         .map_err(|_| WebSocketError::SendFailed)?;
    //     stream
    //         .flush()
    //         .await
    //         .map_err(|_| WebSocketError::SendFailed)?;
    //     Ok(())
    // }

    // async fn read_upgrade_response<S>(
    //     &self,
    //     stream: &mut S,
    //     nonce: &[u8],
    // ) -> Result<(), WebSocketError>
    // where
    //     S: embedded_io_async::Read,
    // {
    //     let mut resp_buf = [0u8; 1024];
    //     let mut total = 0usize;

    //     loop {
    //         let n = stream
    //             .read(&mut resp_buf[total..])
    //             .await
    //             .map_err(|_| WebSocketError::HandshakeFailed)?;
    //         total += n;
    //         if resp_buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
    //             break;
    //         }
    //         if total >= resp_buf.len() {
    //             break;
    //         }
    //     }

    //     let response = core::str::from_utf8(&resp_buf[..total])
    //         .map_err(|_| WebSocketError::HandshakeFailed)?;

    //     let logger = espforge_platform::logger::Logger::new();
    //     logger.info("Server response:");
    //     logger.info(response);

    //     let status: u16 = response
    //         .lines()
    //         .next()
    //         .and_then(|l| l.split_whitespace().nth(1))
    //         .and_then(|s| s.parse::<u16>().ok())
    //         .ok_or(WebSocketError::HandshakeFailed)?;

    //     if status != 101 {
    //         return Err(WebSocketError::HandshakeFailed);
    //     }

    //     let resp_headers: heapless::Vec<(&str, &str), 16> = response
    //         .lines()
    //         .skip(1)
    //         .filter_map(|line| {
    //             let mut parts = line.splitn(2, ':');
    //             let name = parts.next()?.trim();
    //             let value = parts.next()?.trim();
    //             Some((name, value))
    //         })
    //         .collect();

    //     if !is_upgrade_accepted(nonce, resp_headers.iter().copied()) {
    //         return Err(WebSocketError::HandshakeFailed);
    //     }
    //     Ok(())
    // }

    // ── send / receive ──────────────────────────────────────────────────────────

    // ── send / receive ──────────────────────────────────────────────────────────

    pub async fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: text.len() as u64,
            mask_key: Some(mask_key.into()),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(session, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(socket, text.as_bytes()).await.map_err(|_| WebSocketError::SendFailed)?;
        } else {
            return Err(WebSocketError::NotConnected);
        }
        Ok(())
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();
        
        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: data.len() as u64,
            mask_key: Some(mask_key.into()),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(session).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(session, data).await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
            header.send_payload(socket, data).await.map_err(|_| WebSocketError::SendFailed)?;
        } else {
            return Err(WebSocketError::NotConnected);
        }
        Ok(())
    }

    pub async fn receive<'b>(
        &mut self,
        buf: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {

        macro_rules! recv_from {
            ($stream:expr) => {{
                let header = FrameHeader::recv($stream)
                    .await
                    .map_err(|_| WebSocketError::ReceiveFailed)?;
                let len = header.payload_len as usize;
                if len > buf.len() {
                    return Err(WebSocketError::ReceiveFailed);
                }
                header
                    .recv_payload($stream, &mut buf[..len])
                    .await
                    .map_err(|_| WebSocketError::ReceiveFailed)?;
                match header.frame_type {
                    FrameType::Text(_) => {
                        let s = core::str::from_utf8(&buf[..len])
                            .map_err(|_| WebSocketError::ProtocolError)?;
                        Ok(Some(Message::Text(s)))
                    }
                    FrameType::Binary(_) => Ok(Some(Message::Binary(&buf[..len]))),
                    FrameType::Ping => Ok(Some(Message::Ping)),
                    FrameType::Pong => Ok(Some(Message::Pong)),
                    FrameType::Close => Ok(Some(Message::Close)),
                    _ => Err(WebSocketError::UnexpectedFrame),
                }
            }};
        }

        if let Some(session) = self.tls_socket.as_mut() {
            recv_from!(session)
        } else if let Some(socket) = self.socket.as_mut() {
            // FIX: Reborrow the raw socket into the macro call expansion
            recv_from!(socket)
        } else {
            Err(WebSocketError::NotConnected)
        }
    }

    pub async fn close(&mut self) -> Result<(), WebSocketError> {
        let mask_key = unsafe { espforge_platform::rng::Rng::new() }.random_u32();

        let header = FrameHeader {
            frame_type: FrameType::Close,
            payload_len: 0,
            mask_key: Some(mask_key.into()),
        };

        if let Some(session) = self.tls_socket.as_mut() {
            header.send(session).await.map_err(|_| WebSocketError::SendFailed)?;
        } else if let Some(socket) = self.socket.as_mut() {
            header.send(socket).await.map_err(|_| WebSocketError::SendFailed)?;
        } else {
            return Err(WebSocketError::NotConnected);
        }

        self.socket = None;
        self.tls_socket = None;

        Ok(())
     }
}


