use core::ffi::CStr;
use core::fmt;
use core::net::SocketAddr;

use edge_http::io::client::Connection;
use edge_nal::{AddrType, Dns};
use edge_nal_embassy::{Dns as EmbassyDns, Tcp, TcpBuffers, TcpSocket};
use edge_nal_tls::mbedtls::{
    Certificate,
    ClientSessionConfig,
    TlsReference,
    TlsSocket,
    X509,
};
use edge_nal_tls::TlsConnector;
use edge_ws::{FrameHeader, FrameType};

use embedded_io_async::{Read, Write};

use espforge_platform::embassy_net::Stack;

use heapless::String;
use rand_core::{TryCryptoRng, TryRng};

//
// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const IO_BUFFER_SIZE: usize = 2048;
const FRAME_BUFFER_SIZE: usize = 2048;
const HOSTNAME_SIZE: usize = 128;

//
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

//
// -----------------------------------------------------------------------------
// RNG
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
        todo!()
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        todo!()
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        todo!()
    }
}

impl TryCryptoRng for TlsRng {}

//
// -----------------------------------------------------------------------------
// Resources
// -----------------------------------------------------------------------------

pub struct WebSocketResources {
    pub io_buffer: [u8; IO_BUFFER_SIZE],
    pub frame_buffer: [u8; FRAME_BUFFER_SIZE],
    pub hostname: [u8; HOSTNAME_SIZE],
    pub tcp_buffers: TcpBuffers<1, IO_BUFFER_SIZE, IO_BUFFER_SIZE>,
}

impl WebSocketResources {
    pub const fn new() -> Self {
        todo!()
    }
}

//
// -----------------------------------------------------------------------------
// URI
// -----------------------------------------------------------------------------

struct Uri {
    secure: bool,
    host: String<64>,
    port: u16,
    path: String<128>,
}

//
// -----------------------------------------------------------------------------
// Active Transport
// -----------------------------------------------------------------------------

enum Transport<'a> {
    Plain(
        TcpSocket<'a>,
    ),

    Tls(
        TlsSocket<'a, TcpSocket<'a>>,
    ),
}

//
// -----------------------------------------------------------------------------
// Client
// -----------------------------------------------------------------------------

pub struct WebSocketClient<'a> {
    stack: Stack<'static>,

    uri: Uri,

    transport: Option<Transport<'a>>,

    resources: &'static mut WebSocketResources,

    tls: Option<TlsReference<'a>>,
}

//
// -----------------------------------------------------------------------------
// Construction
// -----------------------------------------------------------------------------

impl<'a> WebSocketClient<'a> {
    pub fn new(
        stack: Stack<'static>,
        uri: &str,
        resources: &'static mut WebSocketResources,
        tls: Option<TlsReference<'a>>,
    ) -> Result<Self, WebSocketError> {
        todo!()
    }

    fn parse_uri(
        uri: &str,
    ) -> Result<Uri, WebSocketError> {
        todo!()
    }

    fn server_name(
        &mut self,
    ) -> Result<&CStr, WebSocketError> {
        todo!()
    }

    fn socket_addr(
        &self,
    ) -> Result<SocketAddr, WebSocketError> {
        todo!()
    }

    pub fn is_connected(
        &self,
    ) -> bool {
        self.transport.is_some()
    }

    pub fn is_secure(
        &self,
    ) -> bool {
        self.uri.secure
    }
}

//
// -----------------------------------------------------------------------------
// Connection
// -----------------------------------------------------------------------------

impl<'a> WebSocketClient<'a> {
    pub async fn connect(
        &mut self,
    ) -> Result<(), WebSocketError> {
        todo!()
    }

    async fn connect_plain(
        &mut self,
    ) -> Result<(), WebSocketError> {
        todo!()
    }

    async fn connect_tls(
        &mut self,
    ) -> Result<(), WebSocketError> {
        todo!()
    }
}

//
// -----------------------------------------------------------------------------
// Sending
// -----------------------------------------------------------------------------

impl<'a> WebSocketClient<'a> {
    pub async fn send_text(
        &mut self,
        text: &str,
    ) -> Result<(), WebSocketError> {
        todo!()
    }

    pub async fn send_binary(
        &mut self,
        data: &[u8],
    ) -> Result<(), WebSocketError> {
        todo!()
    }

    async fn send_frame(
        &mut self,
        frame: FrameType,
        payload: &[u8],
    ) -> Result<(), WebSocketError> {
        todo!()
    }
}

//
// -----------------------------------------------------------------------------
// Receiving
// -----------------------------------------------------------------------------

impl<'a> WebSocketClient<'a> {
    pub async fn receive<'b>(
        &mut self,
        buffer: &'b mut [u8],
    ) -> Result<Message<'b>, WebSocketError> {
        todo!()
    }

    async fn recv_frame<'b>(
        &mut self,
        buffer: &'b mut [u8],
    ) -> Result<(FrameType, &'b [u8]), WebSocketError> {
        todo!()
    }
}

//
// -----------------------------------------------------------------------------
// Closing
// -----------------------------------------------------------------------------

impl<'a> WebSocketClient<'a> {
    pub async fn close(
        &mut self,
    ) -> Result<(), WebSocketError> {
        todo!()
    }
}

//
// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn make_nonce() -> [u8; 16] {
    todo!()
}

fn make_mask() -> u32 {
    todo!()
}

