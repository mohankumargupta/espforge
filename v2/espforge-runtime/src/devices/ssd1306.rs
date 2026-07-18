//! `ssd1306` device: a terminal OLED driver (ADR-003), consuming an I2C bus
//! component by value (ADR-008). Uses the same external crates v1 used
//! (`ssd1306` + `embedded-graphics`; see `espforge_devices::devices::ssd1306`).
//!
//! The underlying display driver is held behind a `RefCell` (mirroring
//! `Led`/`Button`) so every method takes `&self`. This is what lets the
//! `device!` macro hand out a single shared reference that works uniformly in
//! both blocking (`&mut Context`) and Embassy (`&'static Context`) contexts —
//! a device shared into an async task no longer needs a `&mut` borrow of the
//! whole `Context` to be touched. Mutations go through `critical_section`
//! (same idiom as `Led`) so a reentrant caller panics loudly instead of
//! corrupting the buffer.

use core::cell::RefCell;
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
    display: RefCell<Ssd1306Display>,
    text_style: MonoTextStyle<'static, BinaryColor>,
}

impl Ssd1306 {
    /// `bus` is the I2C bus component. 
    /// `I2cBus` is a `Copy` handle, so this is a pointer bitcopy, not a peripheral move.
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
        Ssd1306 {
            display: RefCell::new(display),
            text_style,
        }
    }

    pub fn init(&self) {
        critical_section::with(|_cs| {
            let _ = self.display.borrow_mut().init();
        });
     }

    pub fn clear(&self) {
        critical_section::with(|_cs| {
            let _ = self.display.borrow_mut().clear(BinaryColor::Off);
        });

    pub fn flush(&self) {
        critical_section::with(|_cs| {
            let _ = self.display.borrow_mut().flush();
        });
     }

    pub fn print(&self, x: i32, y: i32, text: &str) {
        critical_section::with(|_cs| {
            let mut disp = self.display.borrow_mut();
            let _ = Text::with_baseline(text, Point::new(x, y), self.text_style, Baseline::Top)
                .draw(&mut *disp);
        });
    }
    
    }
}

