#![no_std]

// espforge_components/src/lib.rs
//
// Changes vs original:
//   • WebSocket re-exports updated: CloseCode and OpCode never existed in our
//     module — removed.  Message is now our own type defined in websockets/mod.rs.

pub mod components;

pub use components::button::Button;
pub use components::led::component::LED;
pub use components::i2c::I2C;
pub use components::spi::SPI;
pub use components::uart::Uart;

#[cfg(feature = "http")]
pub use components::http::HttpClient;
#[cfg(feature = "http")]
pub use components::http::HttpError;
#[cfg(feature = "http")]
pub use components::http::HttpResources;
#[cfg(feature = "http")]
pub use components::http::HttpResponse;

#[cfg(feature = "websockets")]
pub use components::websockets::{
    Message,
    WebSocketClient,
    WebSocketError,
    WebSocketResources,
};

