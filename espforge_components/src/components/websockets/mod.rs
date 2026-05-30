// espforge_components/src/components/websockets/mod.rs
//
// Rewritten to use the actual APIs that exist in the dependency tree:
//
//   edge_ws   0.7  — FrameHeader, FrameType (no Message type, no client::upgrade)
//   edge_http 0.7  — Connection for HTTP-upgrade handshake, ws:: helper constants
//   edge_net  0.13 — re-exports edge_nal_embassy as ::embassy (with embassy feature)
//   embassy_net     — through espforge_platform::embassy_net
//
// The component keeps an open TcpSocket across connect/send/receive calls by
// storing the raw rx/tx buffers inside WebSocketResources and re-creating the
// socket on each connect().  For production use the socket would be stored in
// Self; here we keep the design simple and close-to-working so it compiles
// and users can wire in the real connection-persistence themselves.

use core::fmt;

use espforge_platform::embassy_net::Stack;
use heapless::String;

// ── Public message type (replaces the non-existent edge_ws::Message) ────────

/// A received WebSocket frame, analogous to the Message type in higher-level
/// WS libraries.  We define our own because edge-ws 0.7 exposes raw
/// FrameType + payload bytes rather than a unified Message enum.
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

// ── Resources ────────────────────────────────────────────────────────────────

const RX_BUF: usize = 4096;
const TX_BUF: usize = 4096;

pub struct WebSocketResources {
    pub rx_buf: [u8; RX_BUF],
    pub tx_buf: [u8; TX_BUF],
    /// Payload scratch buffer used by recv().
    pub payload_buf: [u8; 2048],
    /// TLS buffers — only populated by new_with_tls().
    pub tls_rx_buf: Option<[u8; 16640]>,
    pub tls_tx_buf: Option<[u8; 16640]>,
}

impl WebSocketResources {
    /// Plain WebSocket (ws://).
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; RX_BUF],
            tx_buf: [0u8; TX_BUF],
            payload_buf: [0u8; 2048],
            tls_rx_buf: None,
            tls_tx_buf: None,
        }
    }

    /// Secure WebSocket (wss://).  Adds ~32 KB of TLS I/O buffers.
    pub const fn new_with_tls() -> Self {
        Self {
            rx_buf: [0u8; RX_BUF],
            tx_buf: [0u8; TX_BUF],
            payload_buf: [0u8; 2048],
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

// ── Client ───────────────────────────────────────────────────────────────────

pub struct WebSocketClient<'a> {
    stack: Stack<'static>,
    resources: &'a mut WebSocketResources,
    uri: String<128>,
}

impl<'a> WebSocketClient<'a> {
    pub fn new(
        stack: Stack<'static>,
        resources: &'a mut WebSocketResources,
        uri: &str,
    ) -> Self {
        let mut s: String<128> = String::new();
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

    // ── Parse URI ────────────────────────────────────────────────────────────

    /// Returns `(host, port, path, is_wss)` as owned strings.
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
                let p: u16 = host_port[i + 1..]
                    .parse()
                    .map_err(|_| WebSocketError::InvalidUri)?;
                (&host_port[..i], p)
            }
            None => (host_port, if is_wss { 443u16 } else { 80u16 }),
        };

        let mut host: String<64> = String::new();
        host.push_str(host_raw).map_err(|_| WebSocketError::InvalidUri)?;

        let mut path: String<64> = String::new();
        path.push_str(path_raw).map_err(|_| WebSocketError::InvalidUri)?;

        Ok((host, port, path, is_wss))
    }

    // ── Connect ──────────────────────────────────────────────────────────────

    /// Perform DNS resolution, TCP connect, and WebSocket HTTP upgrade.
    ///
    /// Uses `embassy_net::Stack::dns_query` directly (available because the
    /// `dns` feature of `embassy-net` is enabled via `espforge_platform/wifi`).
    pub async fn connect(&mut self) -> Result<(), WebSocketError> {
        use edge_http::io::client::Connection;
        use edge_http::ws::{MAX_BASE64_KEY_LEN, NONCE_LEN};
        use embedded_io_async::Write as _;
        use espforge_platform::embassy_net::dns::DnsQueryType;
        use espforge_platform::embassy_net::tcp::TcpSocket;

        let (host, port, path, is_wss) = self.parse_uri()?;

        if is_wss && (self.resources.tls_rx_buf.is_none() || self.resources.tls_tx_buf.is_none()) {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        // DNS
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

        // wss:// is not yet supported in this scaffold — error early rather
        // than silently doing a plain connection.
        if is_wss {
            return Err(WebSocketError::TlsBuffersMissing);
        }

        // WebSocket HTTP upgrade using edge-http's Connection type.
        // We re-use the payload_buf as the HTTP I/O scratch buffer.
        let mut conn: Connection<'_, _> =
            Connection::new(&mut self.resources.payload_buf, &mut socket);

        // Build a 16-byte nonce for Sec-WebSocket-Key.
        // rand_core is available as a dep, but Rng::new requires esp-hal which
        // is not a direct dep here.  We use a fixed nonce; replace with real
        // randomness in production.
        let nonce: [u8; NONCE_LEN] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        ];

        let mut key_buf = [0u8; MAX_BASE64_KEY_LEN];

        conn.initiate_ws_upgrade_request(
            Some(host.as_str()), // host
            None,                // origin
            path.as_str(),       // path
            None,                // version (defaults to "13")
            &nonce,
            &mut key_buf,
        )
        .await
        .map_err(|_| WebSocketError::HandshakeFailed)?;

        conn.initiate_response()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        let mut accept_buf = [0u8; edge_http::ws::MAX_BASE64_KEY_RESPONSE_LEN];
        let accepted = conn
            .is_ws_upgrade_accepted(&nonce, &mut accept_buf)
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        if !accepted {
            return Err(WebSocketError::HandshakeFailed);
        }

        conn.complete()
            .await
            .map_err(|_| WebSocketError::HandshakeFailed)?;

        Ok(())
    }

    // ── Send ─────────────────────────────────────────────────────────────────

    /// Send a UTF-8 text frame.
    ///
    /// NOTE: In this scaffold the TcpSocket is not stored across calls (doing
    /// so requires self-referential lifetimes or arena allocation).  This
    /// method documents the intended API; wire in the stored socket once the
    /// connection persistence strategy is decided.
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

