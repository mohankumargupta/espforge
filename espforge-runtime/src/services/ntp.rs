//! `NtpClient`: an outgoing SNTP client over an `embassy-net` stack.
//!
//! The protocol itself is [`sntpc`]'s: it builds the 48-byte SNTPv4 request,
//! checks the reply, and derives the server time plus the round-trip delay and
//! clock offset. What this module supplies is the two traits that crate asks
//! for — a UDP socket ([`NtpUdpSocket`] over [`UdpSocket`]) and a timestamp
//! source ([`NtpTimestampGenerator`] over [`Instant`]) — and the DNS lookup
//! that turns the configured host name into a socket address.
//!
//! There is no RTC on the board, so the timestamp source counts from boot:
//! round-trip delay stays correct, and the server's own transmit timestamp is
//! what becomes the reported Unix time. `sntpc` reconstructs the NTP era
//! around that boot-relative pivot, which is right while the real Unix time is
//! within half an era (2^31 seconds) of 1970 — that is, through 2038.

use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use embassy_net::Stack;
use embassy_net::dns::DnsQueryType;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_time::{Duration, Instant, with_timeout};
use sntpc::{NtpContext, NtpTimestampGenerator, NtpUdpSocket, get_time};

/// Bytes of receive buffer one SNTP datagram needs.
const RX_BUFFER: usize = 256;
/// Bytes of transmit buffer one SNTP request needs.
const TX_BUFFER: usize = 256;

/// What `<ntp>` declared on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpConfig {
    /// Host name or IP address of the NTP server (e.g. `"pool.ntp.org"`).
    pub server: &'static str,
    /// UDP port (the standard NTP port is 123).
    pub port: u16,
    /// DNS lookup and UDP exchange timeout, in milliseconds.
    pub timeout_ms: u64,
}

impl Default for NtpConfig {
    fn default() -> Self {
        Self {
            server: "pool.ntp.org",
            port: 123,
            timeout_ms: 5000,
        }
    }
}

/// Why a time query failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtpError {
    /// The server host name did not resolve to an IPv4 address.
    Dns,
    /// The UDP socket could not be bound, sent, or received from.
    Socket,
    /// The query did not finish within the configured timeout.
    Timeout,
    /// The server answered with something the SNTP parser rejects.
    Protocol,
}

/// The server time a successful query reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpTimestamp {
    /// Seconds since the Unix epoch (1970-01-01 00:00:00 UTC).
    pub unix_seconds: u64,
    /// Sub-second fraction in microseconds (0..=999_999).
    pub microseconds: u32,
    /// Estimated round-trip network delay in microseconds.
    pub roundtrip_delay_us: u32,
    /// Estimated server clock offset in microseconds, relative to the board's
    /// monotonic clock (which starts at boot, since there is no RTC).
    pub offset_us: i64,
}

/// A handle to the board's SNTP client. `Copy`, so it can sit on `Services`.
#[derive(Clone, Copy)]
pub struct NtpClient {
    stack: Stack<'static>,
    config: NtpConfig,
}

impl NtpClient {
    pub fn new(stack: Stack<'static>, config: NtpConfig) -> Self {
        Self { stack, config }
    }

    /// Query the configured server for the current network time.
    pub async fn get_time(&self) -> Result<NtpTimestamp, NtpError> {
        self.get_time_from(self.config.server, self.config.port)
            .await
    }

    /// Query `server` on `port`, regardless of what the board configured.
    pub async fn get_time_from(&self, server: &str, port: u16) -> Result<NtpTimestamp, NtpError> {
        let timeout = Duration::from_millis(self.config.timeout_ms);
        match with_timeout(timeout, self.query_host(server, port)).await {
            Ok(result) => result,
            Err(_) => Err(NtpError::Timeout),
        }
    }

    /// DNS lookup plus one exchange, so the one timeout covers both.
    async fn query_host(&self, server: &str, port: u16) -> Result<NtpTimestamp, NtpError> {
        let addr = self.resolve(server, port).await?;
        self.query(addr).await
    }

    async fn query(&self, server: SocketAddr) -> Result<NtpTimestamp, NtpError> {
        // The socket borrows these buffers, so both live on this task's stack
        // and are released with the socket when the query returns.
        let mut rx_meta = [PacketMetadata::EMPTY; 2];
        let mut rx_buf = [0u8; RX_BUFFER];
        let mut tx_meta = [PacketMetadata::EMPTY; 2];
        let mut tx_buf = [0u8; TX_BUFFER];

        let mut socket = UdpSocket::new(
            self.stack,
            &mut rx_meta,
            &mut rx_buf,
            &mut tx_meta,
            &mut tx_buf,
        );
        socket.bind(0).map_err(|_| NtpError::Socket)?;

        let socket = EmbassyNtpSocket { socket };
        let context = NtpContext::new(EmbassyTimestampGen::new());
        let result = get_time(server, &socket, context)
            .await
            .map_err(map_error)?;

        Ok(NtpTimestamp {
            // `sntpc` already reconstructs the NTP era and returns Unix
            // seconds; only the fraction needs converting.
            unix_seconds: result.sec(),
            microseconds: sntpc::fraction_to_microseconds(result.sec_fraction()),
            roundtrip_delay_us: u32::try_from(result.roundtrip()).unwrap_or(u32::MAX),
            offset_us: result.offset(),
        })
    }

    /// The IPv4 address of `host`, or `host` itself when it already is one.
    async fn resolve(&self, host: &str, port: u16) -> Result<SocketAddr, NtpError> {
        if let Ok(ip) = host.parse::<Ipv4Addr>() {
            return Ok(SocketAddr::V4(SocketAddrV4::new(ip, port)));
        }

        match self.stack.dns_query(host, DnsQueryType::A).await {
            Ok(addrs) => match addrs.first() {
                Some(embassy_net::IpAddress::Ipv4(v4)) => {
                    Ok(SocketAddr::V4(SocketAddrV4::new(*v4, port)))
                }
                _ => Err(NtpError::Dns),
            },
            Err(_) => Err(NtpError::Dns),
        }
    }
}

fn map_error(error: sntpc::Error) -> NtpError {
    match error {
        sntpc::Error::AddressResolve => NtpError::Dns,
        sntpc::Error::Network => NtpError::Socket,
        _ => NtpError::Protocol,
    }
}

// ---------------------------------------------------------------------------
// NtpUdpSocket over embassy-net
// ---------------------------------------------------------------------------

struct EmbassyNtpSocket<'a> {
    socket: UdpSocket<'a>,
}

impl NtpUdpSocket for EmbassyNtpSocket<'_> {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> sntpc::Result<usize> {
        let SocketAddr::V4(v4) = addr else {
            // The resolver only produces IPv4 addresses, and the stack does
            // not carry the IPv6 protocol.
            return Err(sntpc::Error::Network);
        };
        let endpoint =
            embassy_net::IpEndpoint::new(embassy_net::IpAddress::Ipv4(*v4.ip()), v4.port());

        self.socket
            .send_to(buf, endpoint)
            .await
            .map(|()| buf.len())
            .map_err(|_| sntpc::Error::Network)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> sntpc::Result<(usize, SocketAddr)> {
        let (len, meta) = self
            .socket
            .recv_from(buf)
            .await
            .map_err(|_| sntpc::Error::Network)?;

        // The stack carries IPv4 only (`dhcpv4` without `proto-ipv6`), so
        // `IpAddress` has just this one variant.
        let embassy_net::IpAddress::Ipv4(v4) = meta.endpoint.addr;
        Ok((
            len,
            SocketAddr::V4(SocketAddrV4::new(v4, meta.endpoint.port)),
        ))
    }
}

// ---------------------------------------------------------------------------
// NtpTimestampGenerator over embassy-time
// ---------------------------------------------------------------------------

/// Monotonic timestamps counted from the start of the query. `sntpc` takes one
/// before sending and one after receiving, and the round-trip calculation only
/// uses their differences, so the missing wall clock costs nothing here.
#[derive(Clone, Copy)]
struct EmbassyTimestampGen {
    start: Instant,
}

impl EmbassyTimestampGen {
    fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl NtpTimestampGenerator for EmbassyTimestampGen {
    fn init(&mut self) {
        // `sntpc` calls this before *each* timestamp. Resetting `start` here
        // would make every timestamp zero and break the round-trip
        // calculation, so the epoch stays the one `new` chose.
    }

    fn timestamp_sec(&self) -> u64 {
        Instant::now().duration_since(self.start).as_secs()
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        (Instant::now().duration_since(self.start).as_micros() % 1_000_000) as u32
    }
}
