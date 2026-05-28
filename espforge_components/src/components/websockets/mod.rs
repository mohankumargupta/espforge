use crate::components::http::HttpResources;
use crate::components::http::HttpClient;
use espforge_platform::embassy_net::{Stack, tcp::client::{TcpClient, TcpClientState}};

/// WebSocket frame opcodes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpCode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl OpCode {
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

/// WebSocket close codes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseCode {
    NormalClosure = 1000,
    GoingAway = 1001,
    ProtocolError = 1002,
    UnsupportedData = 1003,
    NoStatus = 1005,
    AbnormalClosure = 1006,
    InvalidData = 1007,
    PolicyViolation = 1008,
    TooLarge = 1009,
    MandatoryExt = 1010,
    InternalError = 1011,
    ServiceRestart = 1012,
    TryAgainLater = 1013,
    BadGateway = 1014,
    TlsError = 1015,
}

impl CloseCode {
    pub fn to_u16(&self) -> u16 {
        *self as u16
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

/// WebSocket resources for connection state
pub struct WebSocketResources {
    pub http_resources: HttpResources,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            http_resources: HttpResources::new(),
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

/// WebSocket error types
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
            WebSocketError::ClosedByServer(code) => write!(f, "WebSocket closed by server: {}", code),
        }
    }
}

/// Incoming WebSocket message
#[derive(Debug)]
pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Close(Option<u16>),
    Pong,
}

/// WebSocket client for ESP32
pub struct WebSocketClient<'a> {
    stack: Stack<'static>,
    resources: &'a mut WebSocketResources,
    uri: heapless::String<256>,
}

impl<'a> WebSocketClient<'a> {
    /// Create a new WebSocket client
    pub fn new(stack: Stack<'static>, resources: &'a mut WebSocketResources, uri: &str) -> Self {
        Self {
            stack,
            resources,
            uri: heapless::String::from(uri).unwrap_or_default(),
        }
    }

    /// Check if the socket is connected
    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    /// Perform WebSocket handshake
    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        // Parse URI
        let (host, port, path) = self.parse_uri()?;

        // Create HTTP upgrade request
        let request = self.create_handshake_request(host, path);

        // Connect via TCP
        let mut tcp_state = TcpClientState::<1, 4096, 4096>::new();
        let mut tcp_client = TcpClient::new(self.stack, &mut tcp_state);

        // Connect to server
        tcp_client
            .connect(
                embassy_net::SocketAddr::new(
                    embassy_net::Ipv4Address::new(0, 0, 0, 0), // Will be resolved via DNS
                    port,
                ),
            )
            .await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        // Send handshake
        let bytes = request.as_bytes();
        tcp_client
            .write_all(bytes)
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        // Read handshake response
        let mut rx_buf = [0u8; 1024];
        let n = tcp_client
            .read(&mut rx_buf)
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        // Validate handshake response
        if !self.validate_handshake_response(&rx_buf[..n]) {
            return Err(WebSocketError::HandshakeFailed);
        }

        Ok(())
    }

    /// Send a text message
    pub async fn send_text(&mut self, data: &str) -> Result<(), WebSocketError> {
        self.send_frame(OpCode::Text, data.as_bytes()).await
    }

    /// Send a binary message
    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WebSocketError> {
        self.send_frame(OpCode::Binary, data).await
    }

    /// Send a ping frame
    pub async fn send_ping(&mut self) -> Result<(), WebSocketError> {
        self.send_frame(OpCode::Ping, &[]).await
    }

    /// Receive a message (blocking)
    pub async fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<Message<'_>>, WebSocketError> {
        // This is a simplified implementation
        // Full implementation would handle WebSocket framing
        let n = self.stack
            .read(buffer)
            .await
            .map_err(|_| WebSocketError::ReceiveFailed)?;

        if n == 0 {
            return Ok(None);
        }

        // Parse WebSocket frame
        self.parse_frame(&buffer[..n], buffer)
    }

    /// Close the WebSocket connection
    pub async fn close(&mut self, code: CloseCode) -> Result<(), WebSocketError> {
        let close_frame = self.create_close_frame(code);
        self.send_frame(OpCode::Close, close_frame.as_bytes()).await
    }

    fn parse_uri(&self) -> Result<(heapless::String<64>, u16, heapless::String<128>), WebSocketError> {
        let uri_str = self.uri.as_str();

        // Remove ws:// or wss:// prefix
        let uri = uri_str
            .trim_start_matches("ws://")
            .trim_start_matches("wss://");

        // Split host and path
        let (host_port, path) = if let Some(idx) = uri.find('/') {
            (&uri[..idx], &uri[idx..])
        } else {
            (uri, "/")
        };

        // Split host and port
        let (host, port) = if let Some(idx) = host_port.find(':') {
            let h = heapless::String::from(&host_port[..idx]).unwrap_or_default();
            let p: u16 = host_port[idx + 1..].parse().unwrap_or(80);
            (h, p)
        } else {
            let h = heapless::String::from(host_port).unwrap_or_default();
            (h, 80)
        };

        Ok((host, port, heapless::String::from(path).unwrap_or_default()))
    }

    fn create_handshake_request(&self, host: heapless::String<64>, path: heapless::String<128>) -> heapless::String<512> {
        let mut request = heapless::String::new();

        let _ = request.push_str("GET ");
        let _ = request.push_str(path.as_str());
        let _ = request.push_str(" HTTP/1.1\r\n");

        let _ = request.push_str("Host: ");
        let _ = request.push_str(host.as_str());
        let _ = request.push_str("\r\n");

        let _ = request.push_str("Upgrade: websocket\r\n");
        let _ = request.push_str("Connection: Upgrade\r\n");
        let _ = request.push_str("Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n");
        let _ = request.push_str("Sec-WebSocket-Version: 13\r\n");
        let _ = request.push_str("\r\n");

        request
    }

    fn validate_handshake_response(&self, response: &[u8]) -> bool {
        let response_str = match core::str::from_utf8(response) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Check for HTTP 101 Switching Protocols
        response_str.contains("HTTP/1.1 101") && response_str.contains("Upgrade: websocket")
    }

    async fn send_frame(&mut self, opcode: OpCode, data: &[u8]) -> Result<(), WebSocketError> {
        // This is a simplified implementation
        // Full implementation would create proper WebSocket frames with masking
        let mut frame = Vec::<u8, 2048>::new();

        // FIN bit + opcode
        let first_byte = 0x80 | (opcode as u8);
        frame.push(first_byte).ok();

        // Length and masking (client frames must be masked)
        let len = data.len();
        if len < 126 {
            frame.push(0x80 | (len as u8)).ok(); // Mask bit set
        } else if len < 65536 {
            frame.push(0x80 | 126).ok();
            frame.push((len >> 8) as u8).ok();
            frame.push((len & 0xFF) as u8).ok();
        } else {
            frame.push(0x80 | 127).ok();
            // 8-byte length
            for i in (0..8).rev() {
                frame.push((len >> (i * 8)) as u8).ok();
            }
        }

        // Masking key (4 bytes)
        let mask_key = [0x12, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask_key).ok();

        // Masked payload
        for (i, &byte) in data.iter().enumerate() {
            frame.push(byte ^ mask_key[i % 4]).ok();
        }

        // Send via TCP
        let mut tcp_state = TcpClientState::<1, 4096, 4096>::new();
        let mut tcp_client = TcpClient::new(self.stack, &mut tcp_state);

        tcp_client
            .write_all(&frame)
            .await
            .map_err(|_| WebSocketError::SendFailed)
    }

    fn parse_frame(&self, data: &[u8], _buffer: &mut [u8]) -> Result<Option<Message<'_>>, WebSocketError> {
        if data.is_empty() {
            return Ok(None);
        }

        let first_byte = data[0];
        let opcode = OpCode::from_u8(first_byte & 0x0F).ok_or(WebSocketError::InvalidFrame)?;

        match opcode {
            OpCode::Text => {
                let text = core::str::from_utf8(&data[2..]).ok().unwrap_or("");
                Ok(Some(Message::Text(text)))
            }
            OpCode::Binary => {
                Ok(Some(Message::Binary(&data[2..])))
            }
            OpCode::Close => {
                let close_code = if data.len() >= 4 {
                    let code = ((data[2] as u16) << 8) | (data[3] as u16);
                    Some(code)
                } else {
                    None
                };
                Ok(Some(Message::Close(close_code)))
            }
            OpCode::Pong => Ok(Some(Message::Pong)),
            _ => Ok(None),
        }
    }

    fn create_close_frame(&self, code: CloseCode) -> heapless::String<8> {
        let mut frame = heapless::String::new();
        let code_bytes = code.to_u16().to_be_bytes();
        let _ = frame.push(frame.len() as u8); // Dummy for now
        let _ = frame.push(code_bytes[0]);
        let _ = frame.push(code_bytes[1]);
        frame
    }
}

