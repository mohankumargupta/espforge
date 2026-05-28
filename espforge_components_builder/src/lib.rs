pub mod button;
pub mod http;
pub mod i2c;
pub mod led;
pub mod spi;
pub mod uart;
pub mod websocket;

pub fn init() {
    // Force the MSVC linker to retain these modules by referencing them
    let _ = std::hint::black_box(&button::ButtonPlugin);
    let _ = std::hint::black_box(&http::HttpClientPlugin);
    let _ = std::hint::black_box(&i2c::I2cDevicePlugin);
    let _ = std::hint::black_box(&led::LedPlugin);
    let _ = std::hint::black_box(&spi::SpiDevicePlugin);
    let _ = std::hint::black_box(&uart::UartDevicePlugin);
}
