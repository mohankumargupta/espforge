//! `ssd1306` device: a terminal OLED driver (ADR-003), consuming an I2C bus
//! component by value (ADR-008). Uses the same external crates v1 used
//! (`ssd1306` + `embedded-graphics`; see `espforge_devices::devices::ssd1306`).

use display_interface_i2c::I2CInterface;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use ssd1306::{
    mode::BufferedGraphicsMode, prelude::*, Ssd1306 as Ssd1306Driver,
};

use crate::components::i2c::I2cBus;

type Ssd1306Display = Ssd1306Driver<
    I2CInterface<I2cBus>,
    DisplaySize128x64,
    BufferedGraphicsMode<DisplaySize128x64>,
>;

pub struct Ssd1306 {
    display: Ssd1306Display,
    text_style: MonoTextStyle<'static, BinaryColor>,
}

impl Ssd1306 {
    /// `bus` is the I2C bus component, moved in by value (matches v1's
    /// `SSD1306Device::new(i2c)` — no reset/DC lines on the I2C variant). `I2cBus`
    /// is a `Copy` handle, so this is a pointer bitcopy, not a peripheral move.
    /// `address` is the 7-bit SSD1306 I2C slave address (e.g. 0x3C).
    pub fn new(bus: I2cBus, address: u8) -> Self {
        // `data_byte` is the SSD1306 control/Co-DC byte; 0x40 sets the D/C bit
        // (data), the library clears it for command bytes.
        let interface = I2CInterface::new(bus, address, 0x40);
        let display = Ssd1306Driver::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();
        Ssd1306 { display, text_style }
    }

    pub fn init(&mut self) {
        let _ = self.display.init();
    }

    pub fn clear(&mut self) {
        let _ = self.display.clear(BinaryColor::Off);
    }

    pub fn flush(&mut self) {
        let _ = self.display.flush();
    }

    pub fn print(&mut self, x: i32, y: i32, text: &str) {
        let _ = Text::with_baseline(text, Point::new(x, y), self.text_style, Baseline::Top)
            .draw(&mut self.display);
    }
}
