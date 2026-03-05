use embassy_net::{
    Stack, StackResources,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use reqwless::{
    client::HttpClient,
    request::{Method, RequestBuilder},
};

// Fixed buffer sizes
const RX_BUF: usize = 4096;
const TX_BUF: usize = 1024;

pub struct WifiResources {
    tcp_state: TcpClientState<1, RX_BUF, TX_BUF>,
    rx_buf: [u8; RX_BUF],
    pub stack_resources: StackResources<3>,
}

impl WifiResources {
    pub const fn new() -> Self {
        Self {
            tcp_state: TcpClientState::new(),
            rx_buf: [0u8; RX_BUF],
            stack_resources: StackResources::new(),
        }
    }
}

impl Default for WifiResources {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum WifiError {
    ConnectionFailed,
    HttpError,
}

impl core::fmt::Display for WifiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WifiError::ConnectionFailed => write!(f, "WiFi connection failed"),
            WifiError::HttpError => write!(f, "HTTP request failed"),
        }
    }
}

pub struct WifiClient {
    stack: Stack<'static>,
    resources: &'static mut WifiResources,
}

impl WifiClient {
    pub fn new(stack: Stack<'static>, resources: &'static mut WifiResources) -> Self {
        Self { stack, resources }
    }

    pub async fn get(&mut self, url: &str) -> Result<HttpResponse, WifiError> {
        self.request(Method::GET, url, None).await
    }

    pub async fn post(&mut self, url: &str, body: &[u8]) -> Result<HttpResponse, WifiError> {
        self.request(Method::POST, url, Some(body)).await
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    async fn request(
        &mut self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, WifiError> {
        let tcp_client = TcpClient::new(self.stack, &self.resources.tcp_state);
        let dns_client = DnsSocket::new(self.stack);
        let mut http = HttpClient::new(&tcp_client, &dns_client);

        let mut req = http
            .request(method, url)
            .await
            .map_err(|_| WifiError::ConnectionFailed)?;

        let status;
        let mut body_buf = heapless::Vec::<u8, 2048>::new();
        let truncated;

        if let Some(b) = body {
            let mut req_with_body = req.body(b);
            let response = req_with_body
                .send(&mut self.resources.rx_buf)
                .await
                .map_err(|_| WifiError::HttpError)?;
            status = response.status.0;
            truncated = match response.body().read_to_end().await {
                Ok(data) => {
                    let copy_len: usize = data.len().min(2048);
                    let _ = body_buf.extend_from_slice(&data[..copy_len]);
                    data.len() > 2048
                }
                Err(_) => false,
            };
        } else {
            let response = req
                .send(&mut self.resources.rx_buf)
                .await
                .map_err(|_| WifiError::HttpError)?;
            status = response.status.0;
            truncated = match response.body().read_to_end().await {
                Ok(data) => {
                    let copy_len: usize = data.len().min(2048);
                    let _ = body_buf.extend_from_slice(&data[..copy_len]);
                    data.len() > 2048
                }
                Err(_) => false,
            };
        }

        Ok(HttpResponse {
            status,
            body: body_buf,
            truncated,
        })
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub body: heapless::Vec<u8, 2048>,
    pub truncated: bool,
}

impl HttpResponse {
    pub fn is_ok(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    pub fn text(&self) -> Option<&str> {
        core::str::from_utf8(&self.body).ok()
    }
}
