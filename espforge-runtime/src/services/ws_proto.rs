//! WebSocket protocol vocabulary and the pure bits of the HTTP upgrade.
//!
//! The shared types ([`WsError`], [`MessageType`], [`WsMessage`]) and the URL
//! and response parsing live here, away from the clients, because the clients
//! pull in `esp-hal`'s RNG and `embassy-net`. Keeping the vocabulary and the
//! parsing target-free is what lets their unit tests run on the host.

/// Why a WebSocket operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsError {
    /// The URL is not `ws[s]://host[:port]/path`.
    BadUrl,
    /// DNS could not resolve the host.
    Dns,
    /// The TCP connection could not be opened.
    Connect,
    /// The server refused the WebSocket upgrade (bad status or `Accept` key).
    UpgradeFailed,
    /// The socket failed while sending or receiving a frame, or the caller's
    /// buffer was too small to hold the socket's own buffers.
    Socket,
    /// A frame's header or payload did not decode.
    FrameError,
    /// A frame's payload is larger than the buffer it was given.
    PayloadTooLarge,
    /// The exchange did not finish inside `timeout_ms`.
    Timeout,
    /// The peer closed the WebSocket.
    Closed,
}

/// The kind of an incoming WebSocket message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

/// One incoming WebSocket message, borrowing its payload from the read buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsMessage<'a> {
    pub message_type: MessageType,
    pub payload: &'a [u8],
}

/// Split `ws[s]://host[:port]/path` into its parts. `default_port` fills in
/// the port when the URL omits one. `secure` selects the scheme prefix.
pub(crate) fn parse_ws_url(
    url: &str,
    default_port: u16,
    secure: bool,
) -> Result<(&str, u16, &str), WsError> {
    let rest = if secure {
        url.strip_prefix("wss://").ok_or(WsError::BadUrl)?
    } else {
        url.strip_prefix("ws://").ok_or(WsError::BadUrl)?
    };
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(WsError::BadUrl);
    }
    match authority.rsplit_once(':') {
        // A colon means an explicit port: an empty host (`:80`) or an
        // unparsable port is a bad URL, not a defaulted one.
        Some((host, port)) => {
            if host.is_empty() {
                return Err(WsError::BadUrl);
            }
            let port = port.parse().map_err(|_| WsError::BadUrl)?;
            Ok((host, port, path))
        }
        None => Ok((authority, default_port, path)),
    }
}

/// The `Host` header value: `host`, plus `:port` when it is not the default.
pub(crate) fn host_header<'a>(
    host: &'a str,
    port: u16,
    default_port: u16,
    buf: &'a mut [u8],
) -> &'a str {
    if port == default_port {
        return host;
    }

    let mut len = 0;
    for byte in host.bytes().chain(*b":") {
        if len < buf.len() {
            buf[len] = byte;
            len += 1;
        }
    }
    let mut digits = [0u8; 5];
    let mut at = digits.len();
    let mut value = port;
    while value > 0 {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for digit in &digits[at..] {
        if len < buf.len() {
            buf[len] = *digit;
            len += 1;
        }
    }

    core::str::from_utf8(&buf[..len]).unwrap_or(host)
}

/// The status code from a response's first line.
pub(crate) fn status_code(response: &str) -> Option<u16> {
    response.lines().next()?.split(' ').nth(1)?.parse().ok()
}

/// An iterator over a response's header block as `(name, value)` pairs. The
/// status line has no `:`, so it is skipped; the blank line ends the block.
pub(crate) struct Headers<'a> {
    rest: &'a str,
}

impl<'a> Headers<'a> {
    pub(crate) fn new(response: &'a str) -> Self {
        Self { rest: response }
    }
}

impl<'a> Iterator for Headers<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let end = self.rest.find("\r\n")?;
            let line = &self.rest[..end];
            self.rest = &self.rest[end + 2..];
            if line.is_empty() {
                return None;
            }
            if let Some((name, value)) = line.split_once(':') {
                return Some((name.trim(), value.trim()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_urls_split_into_host_port_and_path() {
        assert_eq!(
            parse_ws_url("ws://echo.local/ws", 80, false),
            Ok(("echo.local", 80, "/ws"))
        );
        assert_eq!(
            parse_ws_url("ws://echo.local:8080/ws", 80, false),
            Ok(("echo.local", 8080, "/ws"))
        );
        assert_eq!(
            parse_ws_url("wss://example.com", 443, true),
            Ok(("example.com", 443, "/"))
        );
        assert_eq!(parse_ws_url("http://x/", 80, false), Err(WsError::BadUrl));
        assert_eq!(parse_ws_url("ws://", 80, false), Err(WsError::BadUrl));
        assert_eq!(parse_ws_url("ws://:80/x", 80, false), Err(WsError::BadUrl));
    }

    #[test]
    fn the_host_header_gains_a_port_only_when_non_default() {
        let mut buf = [0u8; 96];
        assert_eq!(host_header("echo.local", 80, 80, &mut buf), "echo.local");
        let mut buf = [0u8; 96];
        assert_eq!(
            host_header("echo.local", 8080, 80, &mut buf),
            "echo.local:8080"
        );
    }

    #[test]
    fn a_status_line_yields_its_code() {
        assert_eq!(
            status_code("HTTP/1.1 101 Switching Protocols\r\n"),
            Some(101)
        );
        assert_eq!(status_code("not a response"), None);
    }

    #[test]
    fn headers_skip_the_status_line_and_stop_at_the_blank_line() {
        let response = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: abc=\r\n\r\n";
        let headers: Vec<(&str, &str)> = Headers::new(response).collect();
        assert_eq!(
            headers,
            [
                ("Upgrade", "websocket"),
                ("Connection", "Upgrade"),
                ("Sec-WebSocket-Accept", "abc="),
            ]
        );
    }
}
