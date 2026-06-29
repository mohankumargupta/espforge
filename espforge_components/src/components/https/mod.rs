use core::ffi::CStr;
use core::net::SocketAddr;
use edge_http::Method;
use edge_http::io::client::Connection;
use edge_nal::TcpConnect;
use edge_nal::{AddrType, Dns};
use edge_nal_embassy::Dns as EmbassyDns;
use edge_nal_embassy::{Pool, Tcp, TcpBuffers};
use edge_nal_tls::TlsConnector;
use edge_nal_tls::mbedtls::{AuthMode, ClientSessionConfig, Tls};
use embedded_io_async::Read;
use espforge_platform::embassy_net::Stack;
use heapless::{String, Vec};

#[derive(Debug)]
pub enum HttpsError {
    InvalidUrl,
    UnsupportedScheme,
    HostTooLong,
    PathTooLong,
    NotImplemented,
    DnsFailed,
    ConnectionFailed,
    TlsFailed,
    RequestFailed,
    ResponseFailed,
    ReadFailed,
}

impl core::fmt::Display for HttpsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HttpsError::InvalidUrl => write!(f, "invalid HTTPS URL"),
            HttpsError::UnsupportedScheme => write!(f, "URL must start with https://"),
            HttpsError::HostTooLong => write!(f, "HTTPS host is too long"),
            HttpsError::PathTooLong => write!(f, "HTTPS path is too long"),
            HttpsError::NotImplemented => write!(f, "HTTPS request is not implemented yet"),
            HttpsError::DnsFailed => write!(f, "HTTPS DNS lookup failed"),
            HttpsError::ConnectionFailed => write!(f, "HTTPS TCP connection failed"),
            HttpsError::TlsFailed => write!(f, "HTTPS TLS setup failed"),
            HttpsError::RequestFailed => write!(f, "HTTPS request failed"),
            HttpsError::ResponseFailed => write!(f, "HTTPS response failed"),
            HttpsError::ReadFailed => write!(f, "HTTPS response body read failed"),
        }
    }
}

pub struct HttpsResponse {
    pub status: u16,
    pub body: Vec<u8, 2048>,
    pub truncated: bool,
}

impl HttpsResponse {
    pub fn is_ok(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    pub fn text(&self) -> Option<&str> {
        core::str::from_utf8(&self.body).ok()
    }
}

struct ParsedHttpsUrl {
    host: String<128>,
    port: u16,
    path: String<256>,
}

fn parse_https_url(url: &str) -> Result<ParsedHttpsUrl, HttpsError> {
    let rest = url
        .strip_prefix("https://")
        .ok_or(HttpsError::UnsupportedScheme)?;

    if rest.is_empty() {
        return Err(HttpsError::InvalidUrl);
    }

    let (host_port, path_raw) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };

    if host_port.is_empty() {
        return Err(HttpsError::InvalidUrl);
    }

    let (host_raw, port) = match host_port.rfind(':') {
        Some(index) => {
            let host = &host_port[..index];
            let port = host_port[index + 1..]
                .parse::<u16>()
                .map_err(|_| HttpsError::InvalidUrl)?;
            (host, port)
        }
        None => (host_port, 443),
    };

    let mut host = String::new();
    host.push_str(host_raw)
        .map_err(|_| HttpsError::HostTooLong)?;

    let mut path = String::new();
    path.push_str(path_raw)
        .map_err(|_| HttpsError::PathTooLong)?;

    Ok(ParsedHttpsUrl { host, port, path })
}

pub struct HttpsResources {
    pub io_buf: [u8; 4096],
    pub tcp_buffers: TcpBuffers<1, 4096, 4096>,
    pub server_name_buf: [u8; 129],
}

impl HttpsResources {
    pub const fn new() -> Self {
        Self {
            io_buf: [0u8; 4096],
            tcp_buffers: TcpBuffers::new(),
            server_name_buf: [0u8; 129],
        }
    }
}

fn server_name_cstr<'a>(host: &str, buf: &'a mut [u8; 129]) -> Result<&'a CStr, HttpsError> {
    if host.len() >= buf.len() {
        return Err(HttpsError::HostTooLong);
    }

    buf.fill(0);
    buf[..host.len()].copy_from_slice(host.as_bytes());

    CStr::from_bytes_until_nul(buf).map_err(|_| HttpsError::InvalidUrl)
}

impl Default for HttpsResources {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HttpsTlsRng(pub espforge_platform::rng::Rng);

impl rand_core::TryRng for HttpsTlsRng {
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

impl rand_core::TryCryptoRng for HttpsTlsRng {}

pub struct HttpsClient {
    stack: Stack<'static>,
    resources: &'static mut HttpsResources,
}

impl HttpsClient {
    pub fn new(stack: Stack<'static>, resources: &'static mut HttpsResources) -> Self {
        Self { stack, resources }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    pub async fn get(&mut self, url: &str) -> Result<HttpsResponse, HttpsError> {
        let parsed = parse_https_url(url)?;

        let dns = EmbassyDns::new(self.stack);

        let ip = dns
            .get_host_by_name(parsed.host.as_str(), AddrType::IPv4)
            .await
            .map_err(|_| HttpsError::DnsFailed)?;

        let addr = SocketAddr::new(ip, parsed.port);

        let tcp = Tcp::new(self.stack, &self.resources.tcp_buffers);

        let server_name =
            server_name_cstr(parsed.host.as_str(), &mut self.resources.server_name_buf)?;

        let mut rng = HttpsTlsRng(unsafe { espforge_platform::rng::Rng::new() });

        let tls = unsafe { Tls::new_local_borrows(&mut rng) }.map_err(|_| HttpsError::TlsFailed)?;

        let config = ClientSessionConfig {
            ca_chain: None,
            server_name: Some(server_name),
            auth_mode: AuthMode::None,
            ..ClientSessionConfig::new()
        };

        let connector = TlsConnector::new(tls.reference(), tcp, &config);

        let host = parsed.host.as_str();
        let path = parsed.path.as_str();

        let mut conn = Connection::<_, 16>::new(&mut self.resources.io_buf, &connector, addr);

conn.initiate_request(false, Method::Get, path, &[("Host", host)])
    .await
    .map_err(|e| {
        espforge_platform::logger::Logger::new()
            .info(format_args!("initiate_request error: {:?}", e));
        HttpsError::RequestFailed
    })?;

        conn.initiate_response()
            .await
            .map_err(|_| HttpsError::ResponseFailed)?;

        let status = conn.headers().map_err(|_| HttpsError::ResponseFailed)?.code;

        let mut body = Vec::<u8, 2048>::new();
        let mut truncated = false;
        let mut chunk = [0u8; 512];

        loop {
            let len = conn
                .read(&mut chunk)
                .await
                .map_err(|_| HttpsError::ReadFailed)?;

            if len == 0 {
                break;
            }

            let remaining = body.capacity() - body.len();
            let copy_len = remaining.min(len);

            if copy_len > 0 {
                let _ = body.extend_from_slice(&chunk[..copy_len]);
            }

            if copy_len < len {
                truncated = true;
            }
        }

        let _ = conn.close().await;

        Ok(HttpsResponse {
            status,
            body,
            truncated,
        })
    }
}
