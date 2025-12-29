use ili9341::{Ili9341, Orientation, DisplaySize240x320};
use display_interface_spi::SPIInterface;
use embedded_hal::spi::SpiDevice;
use embedded_hal::digital::OutputPin;
use embedded_hal::delay::DelayNs;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Baseline, Text},
};

pub struct ILI9341Device<SPI, DC, RST> {
    display: Ili9341<SPIInterface<SPI, DC>, RST>,
    text_style: embedded_graphics::mono_font::MonoTextStyle<'static, Rgb565>,
}

impl<SPI, DC, RST> ILI9341Device<SPI, DC, RST>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
{
    pub fn new(spi: SPI, dc: DC, rst: RST, delay: &mut impl DelayNs) -> Self {
        let interface = SPIInterface::new(spi, dc);
        let display = Ili9341::new(interface, rst, delay, Orientation::Portrait, DisplaySize240x320).unwrap();
        
        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(Rgb565::WHITE)
            .background_color(Rgb565::BLACK)
            .build();

        Self { display, text_style }
    }

    pub fn clear(&mut self) {
        let _ = self.display.clear(Rgb565::BLACK);
    }

    pub fn print(&mut self, x: i32, y: i32, text: &str) {
         let _ = Text::with_baseline(text, Point::new(x, y), self.text_style, Baseline::Top)
            .draw(&mut self.display);
    }
}