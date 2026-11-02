//! Network services: outgoing clients over an embassy-net stack.
//!
//! Distinct from `hw`: nothing here names a pin or a peripheral. A service
//! takes the [`Stack`](embassy_net::Stack) a component (`<wifi_station>`)
//! exposes and speaks a protocol over it.
//!
//! Each protocol is behind its own runtime feature, so a project with no
//! service never pulls in `edge-http`, `edge-ws` or `embedded-tls`. The
//! WebSocket vocabulary and the pure URL/response parsing have no target API,
//! so they compile (and their tests run) on the host; the clients themselves
//! name `esp-hal`'s RNG and `embassy-net` sockets and build only for the
//! target, the way `hw` does.

// Pure WebSocket vocabulary and parsing: no target API, so it is host-testable.
#[cfg(feature = "ws-client")]
mod ws_proto;
#[cfg(feature = "ws-client")]
pub use ws_proto::{MessageType, WsError, WsMessage};

#[cfg(feature = "http-client")]
mod http_client;
#[cfg(feature = "http-client")]
pub use http_client::*;

// `esp-hal`'s RNG, shared by the TLS clients and the WebSocket clients. It
// names `esp_hal`, so it is target-only.
#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    any(feature = "https-client", feature = "ws-client")
))]
mod rng;

// The clients below name `esp-hal`'s RNG or `embassy-net` sockets, so they
// build only for the target. Their pure pieces live in `ws_proto` above.

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "https-client"
))]
mod https_client;
#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "https-client"
))]
pub use https_client::*;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ws-client"
))]
mod ws_client;
#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ws-client"
))]
pub use ws_client::*;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "wss-client"
))]
mod wss_client;
#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "wss-client"
))]
pub use wss_client::*;

#[cfg(all(any(target_arch = "riscv32", target_arch = "xtensa"), feature = "ntp"))]
mod ntp;
#[cfg(all(any(target_arch = "riscv32", target_arch = "xtensa"), feature = "ntp"))]
pub use ntp::*;
