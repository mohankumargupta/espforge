//! Thin wrapper around `esp_hal`'s hardware RNG.
//!
//! Keeps `esp_hal` out of crates that don't already depend on it
//! (e.g. `espforge_components`).

use esp_hal::rng::Rng as HalRng;

/// A simple hardware random-number generator backed by the ESP32's TRNG peripheral.
pub struct Rng {
    inner: HalRng,
}

impl Rng {
    /// Obtain an `Rng` instance.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no other code holds a reference to the
    /// RNG peripheral at the same time. In practice this is called once
    /// per random-number-generation site and the peripheral is released
    /// immediately after.
    pub unsafe fn new() -> Self {
        // esp-hal 1.1: Rng::new() takes no arguments
        Self { inner: HalRng::new() }
    }

    /// Return a random `u32`.
    pub fn random_u32(&mut self) -> u32 {
        self.inner.random()
    }

    /// Fill `buf` with random bytes.
    pub fn fill_bytes(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i + 4 <= buf.len() {
            let r = self.inner.random().to_ne_bytes();
            buf[i..i + 4].copy_from_slice(&r);
            i += 4;
        }
        if i < buf.len() {
            // Compute the remaining length before borrowing buf mutably,
            // avoiding a simultaneous mutable + immutable borrow.
            let remaining = buf.len() - i;
            let r = self.inner.random().to_ne_bytes();
            buf[i..].copy_from_slice(&r[..remaining]);
        }
    }
}

