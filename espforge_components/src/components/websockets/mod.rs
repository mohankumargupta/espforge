use core::fmt;

use espforge_platform::embassy_net::Stack;
use heapless::String;

#[derive(Debug)]
pub enum Message<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
    Ping,
    Pong,
    Close,
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
            Self::ConnectionFailed    => write!(f, "Connection failed"),
            Self::HandshakeFailed     => write!(f, "WebSocket handshake failed"),
            Self::SendFailed          => write!(f, "Send failed"),
            Self::ReceiveFailed       => write!(f, "Receive failed"),
            Self::InvalidUri          => write!(f, "Invalid WebSocket URI"),
            Self::TlsBuffersMissing   => write!(f, "TLS buffers required for wss://"),
            Self::ProtocolError       => write!(f, "WebSocket protocol error"),
            Self::UnexpectedFrame     => write!(f, "Unexpected WebSocket frame type"),
        }
    }
}

pub struct WebSocketResources {
    pub rx_buf: [u8; 4096],
    pub tx_buf: [u8; 4096],
    pub payload_buf: [u8; 4096],
    pub tls_rx_buf: Option<[u8; 16640]>,
    pub tls_tx_buf: Option<[u8; 16640]>,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; 4096],
            tx_buf: [0u8; 4096],
            payload_buf: [0u8; 4096],
            tls_rx_buf: None,
            tls_tx_buf: None,
        }
    }

    pub const fn new_with_tls() -> Self {
        Self {
            rx_buf: [0u8; 4096],
            tx_buf: [0u8; 4096],
            payload_buf: [0u8; 4096],
            tls_rx_buf: Some([0u8; 16640]),
            tls_tx_buf: Some([0u8; 16640]),
        }
    }
}

impl Default for WebSocketResources {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WebSocketClient<'a> {
    stack: Stack<'static>,
    uri: String<128>,
    resources: &'a mut WebSocketResources,
}

impl<'a> WebSocketClient<'a> {
    /// Argument order matches what the espforge codegen emits:
    /// `WebSocketClient::new(stack, resources, uri)`
    pub fn new(
        stack: Stack<'static>,
        resources: &'a mut WebSocketResources,
        uri: &str,
    ) -> Self {
        let mut s = String::new();
        let _ = s.push_str(uri);
        Self { stack, uri: s, resources }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    // ── Parse URI ────────────────────────────────────────────────────────────

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

    // ── Connect ──────────────────────────────────────────────────────────────

    /// Perform DNS resolution and TCP connect.
    ///
    /// NOTE: The WebSocket HTTP upgrade handshake is not yet implemented —
    /// connection is established at the TCP level only.
    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        use espforge_platform::embassy_net::dns::DnsQueryType;
        use espforge_platform::embassy_net::tcp::TcpSocket;

        let (host, port, _path, is_wss) = self.parse_uri()?;

        if is_wss && (self.resources.tls_rx_buf.is_none() || self.resources.tls_tx_buf.is_none()) {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        // DNS resolution
        let addrs = self
            .stack
            .dns_query(host.as_str(), DnsQueryType::A)
            .await
            .map_err(|_| WebSocketError::DnsResolutionFailed)?;

        let ip = addrs.first().copied().ok_or(WebSocketError::DnsResolutionFailed)?;

        let endpoint = espforge_platform::embassy_net::IpEndpoint::new(ip.into(), port);

        // TCP connect
        let mut socket = TcpSocket::new(
            self.stack,
            &mut self.resources.rx_buf,
            &mut self.resources.tx_buf,
        );

        socket
            .connect(endpoint)
            .await
            .map_err(|_| WebSocketError::ConnectionFailed)?;

        if is_wss {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        // TODO: perform the WebSocket HTTP upgrade handshake over `socket`
        // once a compatible library is integrated.

        Ok(())
    }

    // ── Send ─────────────────────────────────────────────────────────────────

    pub async fn send_text(&mut self, _text: &str) -> Result<(), WebSocketError> {
        Err(WebSocketError::SendFailed)
    }

    pub async fn send_binary(&mut self, _data: &[u8]) -> Result<(), WebSocketError> {
        Err(WebSocketError::SendFailed)
    }

    // ── Receive ──────────────────────────────────────────────────────────────

    pub async fn receive<'b>(
        &mut self,
        _buf: &'b mut [u8],
    ) -> Result<Option<Message<'b>>, WebSocketError> {
        Ok(None)
    }
}

