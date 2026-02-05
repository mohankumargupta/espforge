use embassy_time::{Duration, Timer};

/// Async delay implementation using Embassy timers
pub struct Delay;

impl Delay {
    pub fn new() -> Self {
        Self
    }

    /// Delay for the specified number of milliseconds
    /// Returns a future that must be awaited
    pub fn delay_ms(&self, ms: u32) -> Timer {
        Timer::after(Duration::from_millis(ms as u64))
    }

    /// Delay for the specified number of microseconds
    /// Returns a future that must be awaited
    pub fn delay_us(&self, us: u32) -> Timer {
        Timer::after(Duration::from_micros(us as u64))
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}
