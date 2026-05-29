use embedded_io_async::{Read, Write};
use espforge_platform::embassy_net;
use espforge_platform::embassy_net::Stack;
use heapless::Vec;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpCode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}


impl OpCode {
    #[allow(dead_code)]
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x0 => Some(OpCode::Continuation),
            0x1 => Some(OpCode::Text),
            0x2 => Some(OpCode::Binary),
            0x8 => Some(OpCode::Close),
            0x9 => Some(OpCode::Ping),
            0xA => Some(OpCode::Pong),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseCode {
    NormalClosure,
    GoingAway,
    ProtocolError,
    UnsupportedData,
    NoStatus,
    AbnormalClosure,
    InvalidData,
    PolicyViolation,
    TooLarge,
    MandatoryExt,
    InternalError,
    ServiceRestart,
    TryAgainLater,
    BadGateway,
    TlsError,
}

impl CloseCode {
    pub fn to_u16(&self) -> u16 {
        match self {
            CloseCode::NormalClosure => 1000,
            CloseCode::GoingAway => 1001,
            CloseCode::ProtocolError => 1002,
            CloseCode::UnsupportedData => 1003,
            CloseCode::NoStatus => 1005,
            CloseCode::AbnormalClosure => 1006,
            CloseCode::InvalidData => 1007,
            CloseCode::PolicyViolation => 1008,
            CloseCode::TooLarge => 1009,
            CloseCode::MandatoryExt => 1010,
            CloseCode::InternalError => 1011,
            CloseCode::ServiceRestart => 1012,
            CloseCode::TryAgainLater => 1013,
            CloseCode::BadGateway => 1014,
            CloseCode::TlsError => 1015,
        }
    }

    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            1000 => Some(CloseCode::NormalClosure),
            1001 => Some(CloseCode::GoingAway),
            1002 => Some(CloseCode::ProtocolError),
            1003 => Some(CloseCode::UnsupportedData),
            1005 => Some(CloseCode::NoStatus),
            1006 => Some(CloseCode::AbnormalClosure),
            1007 => Some(CloseCode::InvalidData),
            1008 => Some(CloseCode::PolicyViolation),
            1009 => Some(CloseCode::TooLarge),
            1010 => Some(CloseCode::MandatoryExt),
            1011 => Some(CloseCode::InternalError),
            1012 => Some(CloseCode::ServiceRestart),
            1013 => Some(CloseCode::TryAgainLater),
            1014 => Some(CloseCode::BadGateway),
            1015 => Some(CloseCode::TlsError),
            _ => None,
        }
    }
}

pub struct WebSocketResources {
    pub rx_buf: [u8; 4096],
    pub tx_buf: [u8; 4096],
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; 4096],
            tx_buf: [0u8; 4096],
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum WebSocketError {
    ConnectionFailed,
    HandshakeFailed,
    SendFailed,
    ReceiveFailed,
    InvalidFrame,
    ClosedByServer(u16),
}

impl core::fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WebSocketError::ConnectionFailed => write!(f, "WebSocket connection failed"),
            WebSocketError::HandshakeFailed => write!(f, "WebSocket handshake failed"),
            WebSocketError::SendFailed => write!(f, "WebSocket send failed"),
            WebSocketError::ReceiveFailed => write!(f, "WebSocket receive failed"),
            WebSocketError::InvalidFrame => write!(f, "Invalid WebSocket frame"),
            WebSocketError::ClosedByServer(code) => {
                write!(f, "WebSocket closed by server: {}", code)
            }
        }
    }
}

#[derive(Debug)]
pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Close(Option<u16>),
    Ping,
    Pong,
}

pub struct WebSocketClient<'a> {
    stack: Stack<'static>,
    resources: &'a mut WebSocketResources,
    uri: heapless::String<256>,
}

impl<'a> WebSocketClient<'a> {
    pub fn new(stack: Stack<'static>, resources: &'a mut WebSocketResources, uri: &str) -> Self {
        let mut s: heapless::String<256> = heapless::String::new();
        let _ = s.push_str(uri);
        Self {
            stack,
            resources,
            uri: s,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        let (host, port, path) = self.parse_uri()?;

        let mut request: heapless::String<512> = heapless::String::new();
        let _ = request.push_str("GET ");
        let _ = request.push_str(path.as_str());
        let _ = request.push_str(" HTTP/1.1\r\n");
        let _ = request.push_str("Host: ");
        let _ = request.push_str(host.as_str());
        let _ = request.push_str("\r\n");
        let _ = request.push_str("Upgrade: websocket\r\n");
        let _ = request.push_str("Connection: Upgrade\r\n");
        let _ = request.push_str("Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n");
        let _ = request.push_str("Sec-WebSocket-Version: 13\r\n\r\n");

        // Placeholder address — real implementation would resolve via DNS
        let addr = embassy_net::IpAddress::v4(0, 0, 0, 0);
        let endpoint = embassy_net::IpEndpoint::new(addr, port);

        let mut tcp_rx = [0u8; 4096];
        let mut tcp_tx = [0u8; 4096];
        let mut socket = embassy_net::tcp::TcpSocket::new(self.stack, &mut tcp_rx, &mut tcp_tx);

        socket
            .connect(endpoint)
            .await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        socket
            .write_all(request.as_bytes())
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let mut rx_buf = [0u8; 512];
        let n = socket
            .read(&mut rx_buf)
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let response = core::str::from_utf8(&rx_buf[..n]).unwrap_or("");
        if !response.contains("101") || !response.contains("websocket") {
            return Err(WebSocketError::HandshakeFailed);
        }

        Ok(())
    }

    pub async fn send_text(&mut self, data: &str) -> Result<(), WebSocketError> {
        self.send_frame(OpCode::Text, data.as_bytes()).await
    }

    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        self.send_frame(OpCode::Binary, data).await
    }

    pub async fn send_ping(&mut self) -> Result<(), WebSocketError> {
        self.send_frame(OpCode::Ping, &[]).await
    }

    pub async fn receive<'b>(
        &mut self,
        buffer: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        // Stub: persistent socket storage is needed for a full implementation.
        let _ = buffer;
        Ok(None)
    }

    pub async fn close(&mut self, code: CloseCode) -> Result<(), WebSocketError> {
        let bytes = code.to_u16().to_be_bytes();
        let mut payload: Vec<u8, 8> = Vec::new();
        let _ = payload.push(bytes[0]);
        let _ = payload.push(bytes[1]);
        self.send_frame(OpCode::Close, &payload).await
    }

    fn parse_uri(
        &self,
    ) -> Result<(heapless::String<64>, u16, heapless::String<128>), WebSocketError> {
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

        let mut host_s: heapless::String<64> = heapless::String::new();
        let _ = host_s.push_str(host);

        let mut path_s: heapless::String<128> = heapless::String::new();
        let _ = path_s.push_str(path);

        Ok((host_s, port, path_s))
    }

    async fn send_frame(&mut self, opcode: OpCode, data: &[u8]) -> Result<(), WebSocketError> {
        let mut frame: Vec<u8, 2048> = Vec::new();

        let opcode_byte = match opcode {
            OpCode::Continuation => 0x0u8,
            OpCode::Text => 0x1,
            OpCode::Binary => 0x2,
            OpCode::Close => 0x8,
            OpCode::Ping => 0x9,
            OpCode::Pong => 0xA,
        };
        let _ = frame.push(0x80 | opcode_byte);

        let len = data.len();
        let mask_key: [u8; 4] = [0x12, 0x34, 0x56, 0x78];

        if len < 126 {
            let _ = frame.push(0x80 | (len as u8));
        } else if len < 65536 {
            let _ = frame.push(0x80 | 126u8);
            let _ = frame.push((len >> 8) as u8);
            let _ = frame.push((len & 0xFF) as u8);
        } else {
            let _ = frame.push(0x80 | 127u8);
            for i in (0..8usize).rev() {
                let _ = frame.push((len >> (i * 8)) as u8);
            }
        }

        let _ = frame.extend_from_slice(&mask_key);

        for (i, &byte) in data.iter().enumerate() {
            let _ = frame.push(byte ^ mask_key[i % 4]);
        }

        // Stub: write `frame` to the stored socket in a full implementation.
        let _ = frame;
        Ok(())
    }

    fn parse_frame<'b>(
        &self,
        data: &'b [u8],
        _buffer: &mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        if data.len() < 2 {
            return Ok(None);
        }

        let first_byte = data[0];
        let opcode = OpCode::from_u8(first_byte & 0x0F).ok_or(WebSocketError::InvalidFrame)?;

        match opcode {
            OpCode::Text => {
                let text = core::str::from_utf8(&data[2..]).unwrap_or("");
                Ok(Some(Message::Text(text)))
            }
            OpCode::Binary => Ok(Some(Message::Binary(&data[2..]))),
            OpCode::Close => {
                let close_code = if data.len() >= 4 {
                    Some(u16::from_be_bytes([data[2], data[3]]))
                } else {
                    None
                };
                Ok(Some(Message::Close(close_code)))
            }
            OpCode::Ping => Ok(Some(Message::Ping)),
            OpCode::Pong => Ok(Some(Message::Pong)),
            _ => Ok(None),
        }
    }
}
