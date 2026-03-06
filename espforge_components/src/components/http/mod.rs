use espforge_platform::embassy_net::{
    Stack,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use heapless::Vec;
use reqwless::client::HttpClient as ReqwlessClient;
use reqwless::request::{Method, RequestBuilder};

pub struct HttpResources {
    pub rx_buf: [u8; 4096],
    pub tcp_state: TcpClientState<1, 4096, 4096>,
}

impl HttpResources {
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; 4096],
            tcp_state: TcpClientState::new(),
        }
    }
}

impl Default for HttpResources {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum HttpError {
    ConnectionFailed,
    RequestFailed,
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HttpError::ConnectionFailed => write!(f, "HTTP connection failed"),
            HttpError::RequestFailed => write!(f, "HTTP request failed"),
        }
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8, 2048>,
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

pub struct HttpClient {
    stack: Stack<'static>,
    resources: &'static mut HttpResources,
}

impl HttpClient {
    pub fn new(stack: Stack<'static>, resources: &'static mut HttpResources) -> Self {
        Self { stack, resources }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.is_link_up() && self.stack.config_v4().is_some()
    }

    pub async fn get(&mut self, url: &str) -> Result<HttpResponse, HttpError> {
        self.do_request(Method::GET, url, &[]).await
    }

    pub async fn post(&mut self, url: &str, body: &[u8]) -> Result<HttpResponse, HttpError> {
        self.do_request(Method::POST, url, body).await
    }

    async fn do_request(
        &mut self,
        method: Method,
        url: &str,
        body: &[u8],
    ) -> Result<HttpResponse, HttpError> {
        let tcp_client = TcpClient::new(self.stack, &self.resources.tcp_state);
        let dns_socket = DnsSocket::new(self.stack);
        let mut client = ReqwlessClient::new(&tcp_client, &dns_socket);

        let mut req = client
            .request(method, url)
            .await
            .map_err(|_| HttpError::ConnectionFailed)?;

        // Bind req_with_body to a local so it lives long enough for .send()
        // to complete and for `response` to be valid.
        let mut req_with_body = req.body(body);
        let response = req_with_body
            .send(&mut self.resources.rx_buf)
            .await
            .map_err(|_| HttpError::RequestFailed)?;

        let status = response.status.0;
        let mut body_buf: Vec<u8, 2048> = Vec::new();
        let mut truncated = false;

        if let Ok(body_reader) = response.body().read_to_end().await {
            let body_bytes: &[u8] = body_reader;
            let copy_len = body_bytes.len().min(2048);
            let _ = body_buf.extend_from_slice(&body_bytes[..copy_len]);
            truncated = body_bytes.len() > 2048;
        }

        Ok(HttpResponse {
            status,
            body: body_buf,
            truncated,
        })
    }
}
