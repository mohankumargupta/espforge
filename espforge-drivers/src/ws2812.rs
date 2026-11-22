//! WS2812 / NeoPixel addressable RGB strip over an RMT channel.
//!
//! A pixel is 24 bits, sent most-significant bit first: a short high pulse is a
//! zero, a long one a one. The line then stays low for at least 50 µs to latch
//! the frame. Pulse lengths are datasheet nanoseconds converted with the
//! channel's **actual** divided clock: `esp32h2` clocks RMT at 32 MHz, not
//! 80 MHz, so the same bit is 40 ticks there and 100 ticks on the other
//! supported chips.
//!
//! The timing table is pure and host-tested; the channel wrappers are built
//! only for ESP targets and only when the generated project enables the
//! matching `espforge-drivers` feature.

/// The four data-bit phases and the latch gap, in channel-clock ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timings {
    pub t0h: u16,
    pub t0l: u16,
    pub t1h: u16,
    pub t1l: u16,
    pub reset: u16,
}

/// The WS2812B timing at `clock_hz`.
///
/// Values are rounded to the nearest tick. At 80 MHz this is the canonical
/// 32/68/64/36 table; at the H2's 32 MHz it is 13/27/26/14.
pub fn timings(clock_hz: u32) -> Timings {
    fn ticks(clock_hz: u32, ns: u64) -> u16 {
        let ticks = (u64::from(clock_hz.max(1)) * ns + 500_000_000) / 1_000_000_000;
        ticks.clamp(1, 0x7fff) as u16
    }
    Timings {
        t0h: ticks(clock_hz, 400),
        t0l: ticks(clock_hz, 850),
        t1h: ticks(clock_hz, 800),
        t1l: ticks(clock_hz, 450),
        // The datasheet requires at least 50 µs low.
        reset: ticks(clock_hz, 50_000),
    }
}

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ws2812-rmt"
))]
mod device {
    use super::timings;
    use esp_hal::gpio::Level;
    use esp_hal::rmt::PulseCode;
    use espforge_runtime::{DeviceErrorKind, RmtError};

    /// One RGB strip on a blocking RMT TX channel.
    pub struct Ws2812Rmt {
        channel: espforge_runtime::BlockingRmtTxEndpoint,
    }

    impl Ws2812Rmt {
        /// Take the configured channel. Infallible: a strip has no identity to
        /// probe, so a bad wire only shows as a frame that changes nothing.
        pub fn new(
            channel: espforge_runtime::BlockingRmtTxEndpoint,
        ) -> Result<Self, DeviceErrorKind> {
            Ok(Self { channel })
        }

        /// Send one frame and latch it.
        pub fn write(&mut self, pixels: &[[u8; 3]]) -> Result<(), RmtError> {
            let timings = timings(self.channel.clock_hz());
            let mut frame = [PulseCode::end_marker(); 24];
            for pixel in pixels {
                encode(*pixel, timings, &mut frame);
                self.channel.transmit(&frame)?;
            }
            self.channel.transmit(&[latch(timings)])?;
            Ok(())
        }
    }

    /// One RGB strip on an async RMT TX channel.
    #[cfg(feature = "ws2812-rmt-async")]
    pub struct AsyncWs2812Rmt {
        channel: espforge_runtime::AsyncRmtTxEndpoint,
    }

    #[cfg(feature = "ws2812-rmt-async")]
    impl AsyncWs2812Rmt {
        /// Take the configured channel. Async so the generated deferred
        /// factory can await the constructor uniformly.
        pub async fn new(
            channel: espforge_runtime::AsyncRmtTxEndpoint,
        ) -> Result<Self, DeviceErrorKind> {
            Ok(Self { channel })
        }

        /// Send one frame, awaiting each channel transaction, then latch it.
        pub async fn write(&mut self, pixels: &[[u8; 3]]) -> Result<(), RmtError> {
            let timings = timings(self.channel.clock_hz());
            let mut frame = [PulseCode::end_marker(); 24];
            for pixel in pixels {
                encode(*pixel, timings, &mut frame);
                self.channel.transmit(&frame).await?;
            }
            self.channel.transmit(&[latch(timings)]).await?;
            Ok(())
        }
    }

    /// Encode one GRB-independent RGB pixel as 24 high/low pulse codes.
    fn encode(pixel: [u8; 3], timings: super::Timings, codes: &mut [PulseCode; 24]) {
        for (index, bit) in (0..24)
            .map(|index| {
                let byte = pixel[index / 8];
                let shift = 7 - (index % 8);
                (byte >> shift) & 1 == 1
            })
            .enumerate()
        {
            let (high, low) = if bit {
                (timings.t1h, timings.t1l)
            } else {
                (timings.t0h, timings.t0l)
            };
            codes[index] = PulseCode::new(Level::High, high, Level::Low, low);
        }
    }

    /// The low period that latches a frame.
    fn latch(timings: super::Timings) -> PulseCode {
        PulseCode::new(Level::Low, timings.reset, Level::Low, 0)
    }
}

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ws2812-rmt"
))]
pub use device::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_80_mhz_table_is_the_canonical_ws2812_timing() {
        assert_eq!(
            timings(80_000_000),
            Timings {
                t0h: 32,
                t0l: 68,
                t1h: 64,
                t1l: 36,
                reset: 4_000,
            }
        );
    }

    #[test]
    fn the_h2_32_mhz_source_recomputes_the_same_bit_slots() {
        let h2 = timings(32_000_000);
        assert_eq!((h2.t0h, h2.t0l, h2.t1h, h2.t1l), (13, 27, 26, 14));
        assert_eq!(h2.t0h + h2.t0l, 40);
        assert_eq!(h2.t1h + h2.t1l, 40);
        assert_eq!(h2.reset, 1_600);
    }
}
