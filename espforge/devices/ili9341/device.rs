use crate::components::spi::SPI;
//use crate::platform::gpio::GPIOOutput;
use esp_hal::gpio::{AnyPin, Output, OutputConfig, Level};
use esp_hal::delay::Delay;
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use mipidsi::{Builder, models::ILI9341Rgb565, options::Orientation, interface::SpiInterface};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Baseline, Text},
};

// CHANGE: Added lifetime 'a to struct to hold the buffer reference
pub struct ILI9341Device<'a> {
    // We box the display type to avoid extremely long generic signatures in the generated main struct
    // In a production embedded env, we might typedef this instead of boxing, but this is safer for generation.
    display: mipidsi::Display<
        SpiInterface<'a, 
            ExclusiveDevice<
                esp_hal::spi::master::Spi<'static, esp_hal::Blocking>, 
                Output<'static>, 
                NoDelay
            >, 
            Output<'static>
        >, 
        ILI9341Rgb565, 
        Output<'static>
    >,
    text_style: MonoTextStyle<'static, Rgb565>,
}

impl<'a> ILI9341Device<'a> {
    // CHANGE: Added buffer argument with lifetime 'a
    pub fn new(spi_component: SPI, cs_pin: u8, dc_pin: u8, rst_pin: u8, buffer: &'a mut [u8]) -> Self {
        // 1. Unwrap the SPI bus from the generic component
        let spi_hal = spi_component.into_inner().into_inner();

        // 2. Setup GPIOs manually using AnyPin steal (standard espforge pattern)
        // We use Output from esp_hal directly because mipidsi expects embedded-hal outputs
        let cs_any = unsafe { AnyPin::steal(cs_pin) };
        let cs = Output::new(cs_any, Level::High, OutputConfig::default());

        let dc_any = unsafe { AnyPin::steal(dc_pin) };
        let dc = Output::new(dc_any, Level::Low, OutputConfig::default());

        let rst_any = unsafe { AnyPin::steal(rst_pin) };
        let rst = Output::new(rst_any, Level::Low, OutputConfig::default());

        // 3. Create Exclusive Device (Software CS control)
        let spi_device = ExclusiveDevice::new_no_delay(spi_hal, cs).unwrap();

        // 4. Create Interface
        // CHANGE: Use the passed-in buffer
        let di = SpiInterface::new(spi_device, dc, buffer);

        // 5. Init Display
        // Note: We perform the builder init here. In the generated main, .init() is called, 
        // but for mipidsi the builder pattern initializes during construction of the struct.
        // We can keep a separate init() if we want to delay turning it on, but standard builder does it now.
        let display = Builder::new(ILI9341Rgb565, di)
            .orientation(Orientation::default().flip_horizontal())
            .reset_pin(rst)
            .init(&mut Delay::new())
            .unwrap();

        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(Rgb565::WHITE)
            .background_color(Rgb565::BLACK)
            .build();

        Self {
            display,
            text_style,
        }
    }

    pub fn init(&mut self) {
        // Already initialized in constructor for mipidsi builder, 
        // but we can clear screen here.
        self.clear();
    }

    pub fn clear(&mut self) {
        self.display.clear(Rgb565::BLACK).unwrap();
    }

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