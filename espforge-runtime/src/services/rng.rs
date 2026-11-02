//! `EspRng`: `esp-hal`'s RNG, wrapped as the `rand_core` 0.6 traits.
//!
//! Both TLS (`embedded-tls`'s cipher suites) and the WebSocket clients (the
//! handshake nonce and the per-frame client mask) need randomness. `esp-hal`
//! implements `RngCore` but not the `CryptoRng` marker, so the wrapper is what
//! supplies it. One definition here, so a protocol can be added without
//! copying the four forwarding methods again.

use rand_core::{CryptoRng, RngCore};

/// `esp-hal`'s RNG as a `rand_core` 0.6 source.
pub(crate) struct EspRng(esp_hal::rng::Rng);

impl EspRng {
    pub(crate) fn new() -> Self {
        Self(esp_hal::rng::Rng::new())
    }
}

impl RngCore for EspRng {
    fn next_u32(&mut self) -> u32 {
        self.0.random()
    }

    fn next_u64(&mut self) -> u64 {
        ((self.0.random() as u64) << 32) | self.0.random() as u64
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.read(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.read(dest);
        Ok(())
    }
}

impl CryptoRng for EspRng {}
