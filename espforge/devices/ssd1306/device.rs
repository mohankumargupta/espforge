use crate::components::i2c::I2C; // Import the component type
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306, mode::BufferedGraphicsMode};
use display_interface_i2c::I2CInterface;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};

// We wrap the complex generic type into a concrete struct for the main app to use
pub struct SSD1306Device {
    // Ssd1306<I2CInterface<I2c<I2C0, Blocking>>, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>
    // For simplicity in this example, we use a dynamic dispatch or simplified type expectation.
    // In a real scenario, you might box this or make the wrapper generic.
    display: Ssd1306<
        I2CInterface<crate::platform::i2c::I2CMaster>, 
        DisplaySize128x64, 
        BufferedGraphicsMode<DisplaySize128x64>
    >,
    text_style: embedded_graphics::mono_font::MonoTextStyle<'static, BinaryColor>,
}

impl SSD1306Device {
    /// The constructor takes the I2C Component struct.
    /// Note: This consumes the I2C component instance.
    pub fn new(i2c_component: I2C) -> Self {
        // We assume the I2C component exposes its inner HAL driver via .into_inner()
        // or implements the embedded-hal traits directly.
        let i2c_hal = i2c_component.into_inner(); 

        let interface = I2CDisplayInterface::new(i2c_hal);
        
        // Initialize in Buffered Mode
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
        self.display.init().unwrap();
    }

    pub fn clear(&mut self) {
        self.display.clear(BinaryColor::Off).unwrap();
    }

    pub fn flush(&mut self) {
        self.display.flush().unwrap();
    }

    /// Helper for Ruchy: prints text at x, y
    pub fn print(&mut self, x: i32, y: i32, text: &str) {
        Text::with_baseline(
            text,
            Point::new(x, y),
            self.text_style,
            Baseline::Top,
        )
        .draw(&mut self.display)
        .unwrap();
    }
}