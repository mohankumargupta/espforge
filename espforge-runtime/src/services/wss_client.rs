//! `WssClient`: an outgoing `wss://` client, WebSocket over TLS 1.3.
//!
//! Identical to [`WsClient`](super::ws_client::WsClient) except that the TCP
//! socket is wrapped in an `embedded-tls` connection first: the same RFC 6455
//! handshake and framing run over the TLS stream, and the session type is the
//! shared [`WsSession`](super::ws_client::WsSession).
//!
//! The socket's buffers *and* the TLS record buffers come from the caller's
//! `buffer`, so a session allocates nothing on the heap. The layout is
//! `rx | tx | record read | record write`, each sized from the board's
//! `rx_buffer_size`/`tx_buffer_size` (floored at what a TLS record needs).
//!
//! Certificate verification needs `embedded-tls`'s `rustpki`/`webpki` features
//! and a trust store, so the handshake runs unverified today; `insecure` only
//! records what the board asked for and the element already warns about it.

use embassy_net::Stack;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, with_timeout};
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};

use super::rng::EspRng;
use super::ws_client::{WsSession, handshake_upgrade, resolve};
use super::ws_proto::{WsError, parse_ws_url};

/// Bytes of TCP receive buffer a WebSocket socket needs.
const SOCKET_RX: usize = 1024;
/// Bytes of TCP transmit buffer a WebSocket socket needs.
const SOCKET_TX: usize = 1024;
/// A TLS record read buffer below this cannot hold the handshake.
const MIN_RECORD_READ: usize = 4096;
/// A TLS record write buffer below this cannot frame the handshake.
const MIN_RECORD_WRITE: usize = 2048;

/// What `<wss_client>` declared on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WssClientConfig {
    /// Bytes of TLS record read buffer carved from the caller's `buffer`.
    pub rx_buffer_size: usize,
    /// Bytes of TLS record write buffer carved from the caller's `buffer`.
    pub tx_buffer_size: usize,
    /// Connection, TLS handshake and WebSocket upgrade timeout, in
    /// milliseconds.
    pub timeout_ms: u64,
}

/// A handle to the board's secure WebSocket client. `Copy`, so it can sit on
/// `Services`.
#[derive(Clone, Copy)]
pub struct WssClient {
    stack: Stack<'static>,
    config: WssClientConfig,
    /// The declared `insecure` flag. Verification is not implemented yet, so
    /// this only records what the board asked for.
    insecure: bool,
}

impl WssClient {
    pub fn new(stack: Stack<'static>, config: WssClientConfig, insecure: bool) -> Self {
        Self {
            stack,
            config,
            insecure,
        }
    }

    /// Whether the board asked for `insecure="true"`.
    pub fn is_insecure(&self) -> bool {
        self.insecure
    }

    /// How many bytes `buffer` must have: the socket's own buffers plus the two
    /// TLS record buffers, each floored at what TLS needs.
    pub fn required_buffer(&self) -> usize {
        SOCKET_RX
            + SOCKET_TX
            + self.config.rx_buffer_size.max(MIN_RECORD_READ)
            + self.config.tx_buffer_size.max(MIN_RECORD_WRITE)
    }

    /// Open a TLS session and upgrade it to a WebSocket. `buffer` holds the
    /// socket and TLS record buffers for as long as the session lives; see
    /// [`WssClient::required_buffer`].
    pub async fn connect<'a>(
        &self,
        url: &str,
        buffer: &'a mut [u8],
    ) -> Result<WsSession<TlsConnection<'a, TcpSocket<'a>, Aes128GcmSha256>>, WsError> {
        let (host, port, path) = parse_ws_url(url, 443, true)?;
        let timeout = Duration::from_millis(self.config.timeout_ms);
        match with_timeout(timeout, self.handshake(host, port, path, buffer)).await {
            Ok(result) => result,
            Err(_) => Err(WsError::Timeout),
        }
    }

    async fn handshake<'a>(
        &self,
        host: &str,
        port: u16,
        path: &str,
        buffer: &'a mut [u8],
    ) -> Result<WsSession<TlsConnection<'a, TcpSocket<'a>, Aes128GcmSha256>>, WsError> {
        let addr = resolve(self.stack, host, port).await?;

        let record_read = self.config.rx_buffer_size.max(MIN_RECORD_READ);
        let record_write = self.config.tx_buffer_size.max(MIN_RECORD_WRITE);
        if buffer.len() < SOCKET_RX + SOCKET_TX + record_read + record_write {
            return Err(WsError::Socket);
        }

        let (socket_rx, rest) = buffer.split_at_mut(SOCKET_RX);
        let (socket_tx, rest) = rest.split_at_mut(SOCKET_TX);
        let (record_read, record_write) = rest.split_at_mut(record_read);

        let mut socket = TcpSocket::new(self.stack, socket_rx, socket_tx);
        socket.connect(addr).await.map_err(|_| WsError::Connect)?;

        let config = TlsConfig::new().with_server_name(host);
        let mut tls = TlsConnection::<_, Aes128GcmSha256>::new(socket, record_read, record_write);
        tls.open(TlsContext::new(
            &config,
            UnsecureProvider::new::<Aes128GcmSha256>(EspRng::new()),
        ))
        .await
        .map_err(|_| WsError::Connect)?;

        let mut rng = EspRng::new();
        handshake_upgrade(&mut tls, host, port, path, 443, &mut rng).await?;

        Ok(WsSession::new(tls, rng, self.config.tx_buffer_size))
    }
}
