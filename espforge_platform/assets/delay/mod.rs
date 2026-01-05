use esp_hal::delay::Delay as HalDelay;

pub struct Delay {
    inner: HalDelay,
}

impl Delay {
    pub fn new() -> Self {
        Self {
            inner: HalDelay::new(),
        }
    }

    pub fn delay_ms(&self, ms: u32) {
        self.inner.delay_millis(ms);
    }

    pub fn delay_us(&self, us: u32) {
        self.inner.delay_micros(us);
    }
}
