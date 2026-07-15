//! `ili9341` device: a terminal SPI TFT driver (ADR-003). Uses the same
//! external crates v1 used (`ili9341` + `display-interface-spi` +
//! `embedded-graphics`; see `espforge_devices::devices::ili9341`).

use display_interface_spi::SPIInterface;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Baseline, Text},
};
use esp_hal::gpio::Output;
use ili9341::{DisplaySize240x320, Ili9341 as Ili9341Driver, Orientation};

use crate::components::spi::SpiDevice;

pub struct Ili9341 {
    display: Ili9341Driver<SPIInterface<SpiDevice, Output<'static>>, Output<'static>>,
}

impl Ili9341 {
    /// `spi` is a per-device SPI handle (bus `Copy` handle + private CS + delay,
    /// see `espforge_runtime::components::SpiDevice`), `dc`/`rst` are control
    /// pins moved in by value.
    pub fn new(spi: SpiDevice, dc: Output<'static>, rst: Output<'static>) -> Self {
        let mut delay = spi.delay_clone();
        let interface = SPIInterface::new(spi, dc);
        let display = Ili9341Driver::new(
            interface,
            rst,
            &mut delay,
            Orientation::Portrait,
            DisplaySize240x320,
        )
        .unwrap();
        Ili9341 { display }
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
        let _ = Text::with_baseline(text, Point::new(x, y), style, Baseline::Top)
            .draw(&mut self.display);
    }
}
