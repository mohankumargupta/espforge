//! `WsClient`: an outgoing `ws://` client over an `embassy-net` stack.
//!
//! The lifecycle is two phases. First the RFC 6455 upgrade handshake: an HTTP
//! `GET` carrying the WebSocket headers, whose `Sec-WebSocket-Accept` the
//! server must echo back. [`edge_http::ws`] builds those headers and checks the
//! reply, so this code never hand-codes the `Sec-WebSocket-Key`/`Accept`
//! derivation. Then framing: [`edge_ws`] reads and writes the RFC 6455 frames,
//! and every outgoing client frame is masked with a fresh key from the RNG.
//!
//! A session holds its TCP socket for as long as the caller keeps it, and the
//! socket's own buffers are the caller's `buffer`. That is why `connect` borrows
//! it: the borrow is what makes the socket outlive the handshake. A session
//! needs at least `2 * 1024` bytes; the upgrade response scratch lives on the
//! task stack.

use core::net::Ipv4Addr;

use edge_http::ws::{
    MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN, is_upgrade_accepted,
    upgrade_request_headers,
};
use edge_ws::{FrameHeader, FrameType};
use embassy_net::Stack;
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, with_timeout};
use embedded_io_async::{Read, Write};
use rand_core::RngCore;

use super::rng::EspRng;
use super::ws_proto::{
    Headers, MessageType, WsError, WsMessage, host_header, parse_ws_url, status_code,
};

/// Bytes of TCP receive buffer a WebSocket socket needs.
const SOCKET_RX: usize = 1024;
/// Bytes of TCP transmit buffer a WebSocket socket needs.
const SOCKET_TX: usize = 1024;
/// The socket buffers together: the minimum `buffer` `connect` accepts.
const SOCKET_BUFFERS: usize = SOCKET_RX + SOCKET_TX;

/// The largest WebSocket upgrade response the handshake will read. Real
/// responses are a few hundred bytes; the cap bounds the stack scratch.
const RESPONSE_CAP: usize = 1024;

/// The largest `Host: host:port` header the handshake writes.
const HOST_CAP: usize = 96;

/// What `<ws_client>` declared on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsClientConfig {
    /// Bytes of receive buffer: the cap on one incoming frame's payload.
    pub rx_buffer_size: usize,
    /// Largest outgoing frame payload the client will send.
    pub tx_buffer_size: usize,
    /// Handshake connection and message timeout, in milliseconds.
    pub timeout_ms: u64,
}

/// A handle to the board's plain WebSocket client. `Copy`, so it can sit on
/// `Services`.
#[derive(Clone, Copy)]
pub struct WsClient {
    stack: Stack<'static>,
    config: WsClientConfig,
}

impl WsClient {
    pub fn new(stack: Stack<'static>, config: WsClientConfig) -> Self {
        Self { stack, config }
    }

    /// Open a WebSocket session to `url`, running the upgrade handshake over a
    /// fresh TCP connection. `buffer` holds the socket's own buffers for as
    /// long as the session lives (at least `2 * 1024` bytes).
    pub async fn connect<'a>(
        &self,
        url: &str,
        buffer: &'a mut [u8],
    ) -> Result<WsSession<TcpSocket<'a>>, WsError> {
        let (host, port, path) = parse_ws_url(url, 80, false)?;
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
    ) -> Result<WsSession<TcpSocket<'a>>, WsError> {
        let addr = resolve(self.stack, host, port).await?;

        if buffer.len() < SOCKET_BUFFERS {
            return Err(WsError::Socket);
        }
        let (socket_rx, rest) = buffer.split_at_mut(SOCKET_RX);
        let (socket_tx, _) = rest.split_at_mut(SOCKET_TX);
        let mut socket = TcpSocket::new(self.stack, socket_rx, socket_tx);
        socket.connect(addr).await.map_err(|_| WsError::Connect)?;

        let mut rng = EspRng::new();
        handshake_upgrade(&mut socket, host, port, path, 80, &mut rng).await?;

        Ok(WsSession::new(socket, rng, self.config.tx_buffer_size))
    }
}

/// An open WebSocket session over some byte stream: a plain TCP socket or a TLS
/// connection over one. Outgoing frames are masked, per RFC 6455.
pub struct WsSession<S: Read + Write> {
    socket: S,
    rng: EspRng,
    tx_buffer_size: usize,
}

impl<S: Read + Write> WsSession<S> {
    pub(crate) fn new(socket: S, rng: EspRng, tx_buffer_size: usize) -> Self {
        Self {
            socket,
            rng,
            tx_buffer_size,
        }
    }

    /// Send an unfragmented UTF-8 text frame.
    pub async fn send_text(&mut self, text: &str) -> Result<(), WsError> {
        self.send_frame(FrameType::Text(false), text.as_bytes())
            .await
    }

    /// Send an unfragmented binary frame.
    pub async fn send_binary(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send_frame(FrameType::Binary(false), data).await
    }

    /// Send a ping frame.
    pub async fn ping(&mut self, payload: &[u8]) -> Result<(), WsError> {
        self.send_frame(FrameType::Ping, payload).await
    }

    /// Send a close frame carrying `status_code`.
    pub async fn close(&mut self, status_code: u16) -> Result<(), WsError> {
        self.send_frame(FrameType::Close, &status_code.to_be_bytes())
            .await
    }

    /// Receive one frame, writing its unmasked payload into `rx_buf` and
    /// returning it.
    pub async fn recv<'b>(&mut self, rx_buf: &'b mut [u8]) -> Result<WsMessage<'b>, WsError> {
        let header = FrameHeader::recv(&mut self.socket)
            .await
            .map_err(|_| WsError::FrameError)?;

        let payload_len = header.payload_len as usize;
        if payload_len > rx_buf.len() {
            return Err(WsError::PayloadTooLarge);
        }

        let payload = header
            .recv_payload(&mut self.socket, &mut rx_buf[..payload_len])
            .await
            .map_err(|_| WsError::FrameError)?;

        let message_type = match header.frame_type {
            FrameType::Text(_) => MessageType::Text,
            FrameType::Binary(_) => MessageType::Binary,
            FrameType::Ping => MessageType::Ping,
            FrameType::Pong => MessageType::Pong,
            FrameType::Close => MessageType::Close,
            FrameType::Continue(_) => return Err(WsError::FrameError),
        };

        Ok(WsMessage {
            message_type,
            payload,
        })
    }

    async fn send_frame(&mut self, frame_type: FrameType, payload: &[u8]) -> Result<(), WsError> {
        if payload.len() > self.tx_buffer_size {
            return Err(WsError::PayloadTooLarge);
        }

        let header = FrameHeader {
            frame_type,
            payload_len: payload.len() as u64,
            mask_key: Some(self.rng.next_u32()),
        };

        header
            .send(&mut self.socket)
            .await
            .map_err(|_| WsError::Socket)?;
        header
            .send_payload(&mut self.socket, payload)
            .await
            .map_err(|_| WsError::Socket)?;
        self.socket.flush().await.map_err(|_| WsError::Socket)?;
        Ok(())
    }
}

/// Write the upgrade request and check the reply, over any byte stream.
pub(crate) async fn handshake_upgrade<S: Read + Write>(
    socket: &mut S,
    host: &str,
    port: u16,
    path: &str,
    default_port: u16,
    rng: &mut EspRng,
) -> Result<(), WsError> {
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce);

    let mut key_buf = [0u8; MAX_BASE64_KEY_LEN];
    let mut host_buf = [0u8; HOST_CAP];
    let host_header = host_header(host, port, default_port, &mut host_buf);
    let headers = upgrade_request_headers(Some(host_header), None, None, &nonce, &mut key_buf);

    write_upgrade_request(socket, path, &headers).await?;

    let mut response_buf = [0u8; RESPONSE_CAP];
    let response_len = read_response(socket, &mut response_buf).await?;
    let response =
        core::str::from_utf8(&response_buf[..response_len]).map_err(|_| WsError::UpgradeFailed)?;
    let code = status_code(response).ok_or(WsError::UpgradeFailed)?;

    let mut accept_buf = [0u8; MAX_BASE64_KEY_RESPONSE_LEN];
    if !is_upgrade_accepted(code, Headers::new(response), &nonce, &mut accept_buf) {
        return Err(WsError::UpgradeFailed);
    }

    Ok(())
}

/// Write the request line and upgrade headers, skipping the empty pairs
/// [`upgrade_request_headers`] emits for an absent `Origin`.
async fn write_upgrade_request<S: Write>(
    socket: &mut S,
    path: &str,
    headers: &[(&str, &str)],
) -> Result<(), WsError> {
    for piece in [
        b"GET ".as_slice(),
        path.as_bytes(),
        b" HTTP/1.1\r\n".as_slice(),
    ] {
        socket.write_all(piece).await.map_err(|_| WsError::Socket)?;
    }
    for (name, value) in headers {
        if name.is_empty() {
            continue;
        }
        for piece in [
            name.as_bytes(),
            b": ".as_slice(),
            value.as_bytes(),
            b"\r\n".as_slice(),
        ] {
            socket.write_all(piece).await.map_err(|_| WsError::Socket)?;
        }
    }
    socket
        .write_all(b"\r\n")
        .await
        .map_err(|_| WsError::Socket)?;
    socket.flush().await.map_err(|_| WsError::Socket)?;
    Ok(())
}

/// Read one byte at a time until the blank line that ends the response
/// headers. Byte-by-byte on purpose: a server may coalesce the `101` response
/// with the first frame in one segment, and nothing may be read past the
/// headers or that frame's bytes would be lost.
async fn read_response<S: Read>(socket: &mut S, buf: &mut [u8]) -> Result<usize, WsError> {
    let mut filled = 0;
    let mut byte = [0u8; 1];
    while filled < buf.len() {
        let read = socket.read(&mut byte).await.map_err(|_| WsError::Socket)?;
        if read == 0 {
            return Err(WsError::UpgradeFailed);
        }
        buf[filled] = byte[0];
        filled += 1;
        if filled >= 4 && &buf[filled - 4..filled] == b"\r\n\r\n" {
            return Ok(filled);
        }
    }
    Err(WsError::UpgradeFailed)
}

/// Resolve `host` to an address: a literal needs no DNS. `embassy-net`'s
/// `TcpSocket::connect` takes an `IpEndpoint`, which `SocketAddrV4` provides.
pub(crate) async fn resolve(
    stack: Stack<'static>,
    host: &str,
    port: u16,
) -> Result<core::net::SocketAddrV4, WsError> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(core::net::SocketAddrV4::new(ip, port));
    }
    match stack.dns_query(host, DnsQueryType::A).await {
        Ok(addresses) => match addresses.first() {
            Some(embassy_net::IpAddress::Ipv4(v4)) => Ok(core::net::SocketAddrV4::new(*v4, port)),
            _ => Err(WsError::Dns),
        },
        Err(_) => Err(WsError::Dns),
    }
}
