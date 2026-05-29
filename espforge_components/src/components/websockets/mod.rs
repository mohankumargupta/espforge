use core::fmt::Debug;
use edge_http::io::client::Connection;
use edge_http::ws::{MAX_BASE64_KEY_LEN, NONCE_LEN};
use edge_nal::{AddrType, Dns, TcpConnect};
use edge_ws::{FrameHeader, FrameType};
use embassy_net::Stack;
use espforge_platform::embassy_net;
use heapless::String;

#[derive(Debug)]
pub enum WebSocketError {
    ConnectionFailed,
    HandshakeFailed,
    SendFailed,
    ReceiveFailed,
    InvalidFrame,
    DnsError,
}

impl core::fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WebSocketError::ConnectionFailed => write!(f, "WebSocket connection failed"),
            WebSocketError::HandshakeFailed => write!(f, "WebSocket handshake failed"),
            WebSocketError::SendFailed => write!(f, "WebSocket send failed"),
            WebSocketError::ReceiveFailed => write!(f, "WebSocket receive failed"),
            WebSocketError::InvalidFrame => write!(f, "Invalid WebSocket frame"),
            WebSocketError::DnsError => write!(f, "DNS resolution failed"),
        }
    }
}

pub struct WebSocketResources {
    pub rx_buf: [u8; 4096],
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; 4096],
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close,
}

pub struct WebSocketClient<'a> {
    stack: Stack<'static>,
    resources: &'a mut WebSocketResources,
    uri: String<256>,
}

impl<'a> WebSocketClient<'a> {
    pub fn new(stack: Stack<'static>, resources: &'a mut WebSocketResources, uri: &str) -> Self {
        let mut s: String<256> = String::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            resources,
            uri: s,
        }
    }

    pub async fn run<F, Fut>(&mut self, mut handler: F) -> Result<(), WebSocketError>
    where
        F: FnMut(Message<'_>) -> Fut,
        Fut: core::future::Future<Output = ()>,
    {
        let (host, port, path) = self.parse_uri()?;
        let stack = edge_nal_embassy::Stack::new(self.stack);

        let ip = stack
            .get_host_by_name(host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| WebSocketError::DnsError)?;

        let socket_addr = core::net::SocketAddr::new(ip, port);

        let mut conn: Connection<'_, _, 512> =
            Connection::new(&mut self.resources.rx_buf, &stack, socket_addr);

        let mut nonce = [0u8; NONCE_LEN];
        // In a real app, you should use a proper RNG
        for (i, b) in nonce.iter_mut().enumerate() {
            *b = i as u8;
        }

        let mut key_base64 = [0u8; MAX_BASE64_KEY_LEN];
        let key_len = edge_http::ws::encode_key(&nonce, &mut key_base64);
        let key_str = core::str::from_utf8(&key_base64[..key_len]).map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_request(
            edge_http::Method::Get,
            path.as_str(),
            &[
                ("Host", host.as_str()),
                ("Upgrade", "websocket"),
                ("Connection", "Upgrade"),
                ("Sec-WebSocket-Key", key_str),
                ("Sec-WebSocket-Version", "13"),
            ],
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        let response = conn.initiate_response().await.map_err(|_| WebSocketError::HandshakeFailed)?;

        if response.status != 101 {
            return Err(WebSocketError::HandshakeFailed);
        }

        let mut socket = conn.unbind();

        let mut frame_buf = [0u8; 4096];

        loop {
            let mut header = FrameHeader::recv(&mut socket).await.map_err(|_| WebSocketError::ReceiveFailed)?;
            let payload_len = header.payload_len as usize;

            if payload_len > frame_buf.len() {
                return Err(WebSocketError::InvalidFrame);
            }

            socket.read_exact(&mut frame_buf[..payload_len]).await.map_err(|_| WebSocketError::ReceiveFailed)?;
            header.mask(&mut frame_buf[..payload_len]);

            match header.frame_type {
                FrameType::Text => {
                    if let Ok(text) = core::str::from_utf8(&frame_buf[..payload_len]) {
                        handler(Message::Text(text)).await;
                    }
                }
                FrameType::Binary => {
                    handler(Message::Binary(&frame_buf[..payload_len])).await;
                }
                FrameType::Ping => {
                    handler(Message::Ping).await;
                    let pong = FrameHeader::new(FrameType::Pong, payload_len);
                    pong.send(&mut socket).await.map_err(|_| WebSocketError::SendFailed)?;
                    socket.write_all(&frame_buf[..payload_len]).await.map_err(|_| WebSocketError::SendFailed)?;
                }
                FrameType::Pong => {
                    handler(Message::Pong).await;
                }
                FrameType::Close => {
                    handler(Message::Close).await;
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn parse_uri(&self) -> Result<(String<64>, u16, String<128>), WebSocketError> {
        let uri_str = self.uri.as_str();
        let uri = uri_str
            .trim_start_matches("ws://")
            .trim_start_matches("wss://");

        let (host_port, path) = if let Some(idx) = uri.find('/') {
            (&uri[..idx], &uri[idx..])
        } else {
            (uri, "/")
        };

        let (host, port) = if let Some(idx) = host_port.find(':') {
            let h = &host_port[..idx];
            let p: u16 = host_port[idx + 1..].parse().unwrap_or(80);
            (h, p)
        } else {
            (host_port, 80u16)
        };

        let mut host_s: String<64> = String::new();
        let _ = host_s.push_str(host);

        let mut path_s: String<128> = String::new();
        let _ = path_s.push_str(path);

        Ok((host_s, port, path_s))
    }
}
