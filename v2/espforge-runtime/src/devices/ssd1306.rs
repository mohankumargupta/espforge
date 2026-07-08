//! `ssd1306` device: a terminal OLED driver (ADR-003), consuming an I2C bus
//! component by shared reference plus reset/DC pins by value (ADR-008).

use esp_hal::gpio::Output;

use crate::components::I2cBus;

pub struct Ssd1306 {
    bus: I2cBus,
    reset: Output<'static>,
    dc: Output<'static>,
}

impl Ssd1306 {
    /// `bus` is a shared I2C bus (borrowed from the `I2cBus` component),
    /// `reset`/`dc` are control pins moved in by value.
    pub fn new(bus: I2cBus, reset: Output<'static>, dc: Output<'static>) -> Self {
        Ssd1306 { bus, reset, dc }
    }

    /// Hardware reset pulse.
    pub fn reset(&mut self) {
        self.reset.set_low();
        self.reset.set_high();
    }

    /// Placeholder frame write. A real impl would send the SSD1306 init
    /// sequence + framebuffer over `bus`.
    pub fn write_raw(&mut self, _data: &[u8]) {
        let _ = &self.dc;
        let _ = self.bus.bus();
    }
}
