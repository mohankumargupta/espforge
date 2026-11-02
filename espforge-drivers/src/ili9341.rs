//! ILI9341 SPI color TFT display driver.

use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Size, prelude::OriginDimensions,
};
use esp_hal::Blocking;
use esp_hal::gpio::{Level, Output};
use espforge_runtime::SpiDevice;

pub use sky_ili9341::{ColorOrder, Orientation};

/// An ILI9341 color TFT panel on an SPI device.
///
/// The display owns its SPI device: the `<spi_component>` it was declared with
/// is taken out of `Components` when `Devices::new` runs.
pub struct Ili9341Display {
    display: sky_ili9341::Display<sky_ili9341::SpiInterface<SpiDevice<Blocking>, Output<'static>>>,
    backlight: Option<Output<'static>>,
}

impl Ili9341Display {
    /// Build the panel and run its initialization sequence.
    ///
    /// The driver always pulses a hardware reset line, so `reset` is required
    /// (the schema makes `reset="$<gpio>"` mandatory for `<ili9341>`). The
    /// optional `backlight` is switched on after init.
    pub fn new(
        spi_device: SpiDevice<Blocking>,
        dc: Output<'static>,
        mut reset: Output<'static>,
        backlight: Option<Output<'static>>,
        orientation: Orientation,
        color_order: ColorOrder,
    ) -> Self {
        let interface = sky_ili9341::SpiInterface::new(spi_device, dc);
        let mut delay = esp_hal::delay::Delay::new();
        let display = sky_ili9341::Builder::new(interface)
            .orientation(orientation)
            .color_order(color_order)
            .init(&mut reset, &mut delay)
            .expect("ILI9341 TFT initialization failed");

        let mut backlight = backlight;
        if let Some(pin) = backlight.as_mut() {
            pin.set_high();
        }
        Self { display, backlight }
    }

    /// Turn the backlight on or off, when the panel has a controllable one.
    pub fn set_backlight(&mut self, on: bool) {
        if let Some(pin) = self.backlight.as_mut() {
            pin.set_level(if on { Level::High } else { Level::Low });
        }
    }
}

impl DrawTarget for Ili9341Display {
    type Color = embedded_graphics::pixelcolor::Rgb565;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.display.draw_iter(pixels).map_err(|_| ())
    }

    fn fill_solid(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        self.display.fill_solid(area, color).map_err(|_| ())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.display.clear(color).map_err(|_| ())
    }
}

impl OriginDimensions for Ili9341Display {
    fn size(&self) -> Size {
        self.display.size()
    }
}
