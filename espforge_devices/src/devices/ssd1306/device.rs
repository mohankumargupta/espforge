use crate::platform::components::i2c::I2C;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306, mode::BufferedGraphicsMode};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};

pub struct SSD1306Device {
    display: Ssd1306<
        I2CDisplayInterface<crate::platform::i2c::I2CMaster>, 
        DisplaySize128x64, 
        BufferedGraphicsMode<DisplaySize128x64>
    >,
    text_style: embedded_graphics::mono_font::MonoTextStyle<'static, BinaryColor>,
}

impl SSD1306Device {
    pub fn new(i2c_component: I2C) -> Self {
        // Unwrap the generic component to get the underlying ESP-HAL driver
        let i2c_hal = i2c_component.into_inner(); 

        let interface = I2CDisplayInterface::new(i2c_hal);
        
        let display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();

        Self {
            display,
            text_style,
        }
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
        let _ = Text::with_baseline(
            text,
            Point::new(x, y),
            self.text_style,
            Baseline::Top,
        )
        .draw(&mut self.display);
    }
}

