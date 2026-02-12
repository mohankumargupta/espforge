use display_interface_spi::SPIInterface;
use embedded_graphics::Drawable;
use embedded_graphics::mono_font::{MonoTextStyleBuilder, ascii::FONT_10X20};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiDevice;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

pub struct ILI9341Device<SPI, DC, RST> {
    display: Ili9341<SPIInterface<SPI, DC>, RST>,
}

impl<SPI, DC, RST> ILI9341Device<SPI, DC, RST>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
{
    pub fn new(spi: SPI, dc: DC, rst: RST, delay: &mut impl DelayNs) -> Self {
        let interface = SPIInterface::new(spi, dc);
        let display = Ili9341::new(
            interface,
            rst,
            delay,
            Orientation::Portrait,
            DisplaySize240x320,
        )
        .unwrap();
        Self { display }
    }

    pub fn clear(&mut self) {
        let _ = self.display.clear(Rgb565::BLACK);
    }

    pub fn print(&mut self, x: i32, y: i32, text: &str) {
        let style = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(Rgb565::WHITE)
            .background_color(Rgb565::BLACK)
            .build();

        Text::new(text, Point::new(x, y), style)
            .draw(&mut self.display)
            .ok();
    }
}
