#![no_std]

pub mod components;

#[cfg(feature = "button")]
pub use components::button::Button;

#[cfg(feature = "led")]
pub use components::led::component::LED;

#[cfg(feature = "i2c")]
pub use components::i2c::I2C;

#[cfg(feature = "spi")]
pub use components::spi::SPI;

#[cfg(feature = "uart")]
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
pub use components::websockets::Message;
#[cfg(feature = "websockets")]
pub use components::websockets::WebSocketConnector;
pub use components::websockets::WebSocketSession;
#[cfg(feature = "websockets")]
pub use components::websockets::WebSocketError;
#[cfg(feature = "websockets")]
pub use components::websockets::WebSocketResources;
#[cfg(feature = "websockets")]
pub use mbedtls_rs;
