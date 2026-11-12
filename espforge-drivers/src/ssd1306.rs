//! SSD1306 monochrome I2C OLED display driver.

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Size, prelude::OriginDimensions,
};
use esp_hal::Blocking;
use esp_hal::i2c::master::I2c;
use espforge_runtime::DeviceErrorKind;
use espforge_runtime::embedded_hal_bus::i2c::RefCellDevice;
use ssd1306::mode::DisplayConfig;

pub use ssd1306::prelude::{
    DisplayRotation, DisplaySize128x32 as Oled128x32, DisplaySize128x64 as Oled128x64,
};

#[cfg(feature = "ssd1306-async")]
use ssd1306::prelude::DisplayConfigAsync;

/// The async I2C endpoint the generated wiring builds. `AsyncI2cBus` itself:
/// it implements `embedded_hal_async::i2c::I2c`, locks the controller
/// internally and is `Copy`, and the panel's address is a constructor
/// argument, so two async panels need no extra wrapper.
#[cfg(feature = "ssd1306-async")]
pub type AsyncI2cEndpoint = espforge_runtime::AsyncI2cBus;

/// The I2C endpoint the generated wiring builds: the bus cell, shared by
/// `embedded-hal-bus`, with the panel address passed separately.
pub type I2cEndpoint = RefCellDevice<'static, I2c<'static, Blocking>>;

type Oled<SIZE> = ssd1306::Ssd1306<
    ssd1306::prelude::I2CInterface<I2cEndpoint>,
    SIZE,
    ssd1306::mode::BufferedGraphicsMode<SIZE>,
>;

/// An SSD1306 OLED panel on an I2C bus.
///
/// `SIZE` is one of [`Oled128x64`] or [`Oled128x32`]; the constructor takes the
/// size marker by value, in the same way `ssd1306` does.
pub struct Ssd1306Display<SIZE: ssd1306::prelude::DisplaySize> {
    display: Oled<SIZE>,
}

impl<SIZE: ssd1306::prelude::DisplaySize> Ssd1306Display<SIZE> {
    /// Build the panel on `device` and run its initialization sequence.
    ///
    /// `address` is the 7-bit slave address from the project file, and the
    /// `ssd1306` crate is told to use it: the endpoint does not rewrite the
    /// address a driver passes, so a panel wired to `0x3D` works as declared
    /// and a second part on the same bus is addressable too.
    pub fn new(
        device: I2cEndpoint,
        address: u8,
        size: SIZE,
        rotation: DisplayRotation,
    ) -> Result<Self, DeviceErrorKind> {
        let interface = ssd1306::I2CDisplayInterface::new_custom_address(device, address);
        let mut display =
            ssd1306::Ssd1306::new(interface, size, rotation).into_buffered_graphics_mode();
        display.init().map_err(|_| DeviceErrorKind::Init)?;
        Ok(Self { display })
    }

    /// Push the framebuffer to the panel. Draw calls only change the buffer.
    pub fn flush(&mut self) -> Result<(), ()> {
        self.display.flush().map_err(|_| ())
    }

    /// Clear the framebuffer (call [`flush`](Self::flush) to show it).
    pub fn clear(&mut self) {
        self.display.clear_buffer();
    }
}

impl<SIZE: ssd1306::prelude::DisplaySize> DrawTarget for Ssd1306Display<SIZE> {
    type Color = BinaryColor;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.display.draw_iter(pixels).map_err(|_| ())
    }
}

impl<SIZE: ssd1306::prelude::DisplaySize> OriginDimensions for Ssd1306Display<SIZE> {
    fn size(&self) -> Size {
        self.display.size()
    }
}

// ---------------------------------------------------------------------------
// Async panel (feature `ssd1306-async`)
// ---------------------------------------------------------------------------

/// An SSD1306 OLED panel whose transfer and initialization are async.
///
/// `SIZE` is one of [`Oled128x64`] or [`Oled128x32`]. The driver is always
/// built after boot: `Devices::new` is synchronous, so the generated wiring
/// stores it in an `AsyncDeferred` and the app awaits `init()`.
#[cfg(feature = "ssd1306-async")]
pub struct AsyncSsd1306Display<SIZE: ssd1306::size::DisplaySizeAsync> {
    display: ssd1306::Ssd1306Async<
        ssd1306::prelude::I2CInterface<AsyncI2cEndpoint>,
        SIZE,
        ssd1306::mode::BufferedGraphicsModeAsync<SIZE>,
    >,
}

#[cfg(feature = "ssd1306-async")]
impl<SIZE: ssd1306::size::DisplaySizeAsync> AsyncSsd1306Display<SIZE> {
    /// Build the panel on `device` and await its initialization sequence.
    pub async fn new(
        device: AsyncI2cEndpoint,
        address: u8,
        size: SIZE,
        rotation: DisplayRotation,
    ) -> Result<Self, DeviceErrorKind> {
        // `0x40` is the SSD1306 data control byte, the same one the blocking
        // `I2CDisplayInterface` hands to its interface.
        let interface = ssd1306::prelude::I2CInterface::new(device, address, 0x40);
        let mut display =
            ssd1306::Ssd1306Async::new(interface, size, rotation).into_buffered_graphics_mode();
        display.init().await.map_err(|_| DeviceErrorKind::Init)?;
        Ok(Self { display })
    }

    /// Push the framebuffer to the panel, awaiting the transfer.
    pub async fn flush(&mut self) -> Result<(), ()> {
        self.display.flush().await.map_err(|_| ())
    }

    /// Clear the framebuffer (call [`flush`](Self::flush) to show it).
    pub fn clear(&mut self) {
        self.display.clear_buffer();
    }
}

#[cfg(feature = "ssd1306-async")]
impl<SIZE: ssd1306::size::DisplaySizeAsync> DrawTarget for AsyncSsd1306Display<SIZE> {
    type Color = BinaryColor;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.display.draw_iter(pixels).map_err(|_| ())
    }
}

#[cfg(feature = "ssd1306-async")]
impl<SIZE: ssd1306::size::DisplaySizeAsync> OriginDimensions for AsyncSsd1306Display<SIZE> {
    fn size(&self) -> Size {
        self.display.size()
    }
}
