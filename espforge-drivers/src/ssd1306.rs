//! SSD1306 monochrome I2C OLED display driver.

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Size, prelude::OriginDimensions,
};
use esp_hal::Blocking;
use espforge_runtime::I2cDevice;
use ssd1306::mode::DisplayConfig;

pub use ssd1306::prelude::{
    DisplayRotation, DisplaySize128x32 as Oled128x32, DisplaySize128x64 as Oled128x64,
};

/// The rotation of an OLED panel, re-exported so generated code names only
/// this crate.
type Oled<SIZE> = ssd1306::Ssd1306<
    ssd1306::prelude::I2CInterface<I2cDevice<Blocking>>,
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
    /// The panel talks to the address the component declares in `.espf`: the
    /// `ssd1306` crate passes its own default (`0x3C`) into this handle, and
    /// [`I2cDevice`] ignores a caller-supplied address in favour of the
    /// configured one. A panel wired to `0x3D` therefore works as declared.
    pub fn new(device: I2cDevice<Blocking>, size: SIZE, rotation: DisplayRotation) -> Self {
        let interface = ssd1306::I2CDisplayInterface::new(device);
        let mut display =
            ssd1306::Ssd1306::new(interface, size, rotation).into_buffered_graphics_mode();
        display.init().expect("SSD1306 OLED initialization failed");
        Self { display }
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
