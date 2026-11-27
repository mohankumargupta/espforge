//! WS2812 / NeoPixel addressable RGB strip over an RMT channel.
//!
//! A pixel is 24 bits in **GRB** order (the strips take green, red, blue), sent
//! most-significant bit first: a short high pulse is a zero, a long one a one.
//! The line then stays low for at least 50 µs to latch the frame. Pulse lengths
//! are datasheet nanoseconds converted with the channel's **actual** divided
//! clock: `esp32h2` clocks RMT at 32 MHz, not 80 MHz, so the same bit is 40
//! ticks there and 100 ticks on the other supported chips.
//!
//! One `write` is one `transmit`: every pixel's codes, the latch and the end
//! marker go into a single slice. A gap between pixels would latch the strip
//! early, and the next pixel would land on LED 0 instead of shifting down the
//! strip.
//!
//! The timing table and the GRB packing are pure and host-tested; the channel
//! wrappers are built only for ESP targets and only when the generated project
//! enables the matching `espforge-drivers` feature.

/// The most pixels one frame can carry.
///
/// The frame is built in a stack buffer, not on the heap: 24 codes per pixel
/// plus the latch and its end marker. A longer strip needs a caller-owned
/// buffer, which this API does not have yet.
pub const MAX_PIXELS: usize = 16;

/// The codes of one frame: 24 per pixel, then the latch and its end marker.
// Used by the target-only channel wrappers and by the host tests; a host
// library build has neither.
#[cfg_attr(
    not(all(
        any(target_arch = "riscv32", target_arch = "xtensa"),
        feature = "ws2812-rmt"
    )),
    allow(dead_code)
)]
const fn frame_codes(pixels: usize) -> usize {
    pixels * 24 + 2
}

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

/// One pixel as a 24-bit GRB word, most-significant bit first.
///
/// The app talks RGB; a WS2812B strip takes the three bytes in GRB order.
// Same story as `frame_codes`: the encoder is only built for ESP targets.
#[cfg_attr(
    not(all(
        any(target_arch = "riscv32", target_arch = "xtensa"),
        feature = "ws2812-rmt"
    )),
    allow(dead_code)
)]
fn grb(pixel: [u8; 3]) -> u32 {
    let [red, green, blue] = pixel;
    (u32::from(green) << 16) | (u32::from(red) << 8) | u32::from(blue)
}

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ws2812-rmt"
))]
mod device {
    use super::{MAX_PIXELS, frame_codes, grb, timings};
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

        /// Send `pixels` as one frame and latch it.
        pub fn write(&mut self, pixels: &[[u8; 3]]) -> Result<(), RmtError> {
            let timings = timings(self.channel.clock_hz());
            let mut frame = [PulseCode::end_marker(); frame_codes(MAX_PIXELS)];
            let codes = encode(pixels, timings, &mut frame)?;
            self.channel.transmit(&frame[..codes])
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

        /// Send `pixels` as one frame and latch it.
        pub async fn write(&mut self, pixels: &[[u8; 3]]) -> Result<(), RmtError> {
            let timings = timings(self.channel.clock_hz());
            let mut frame = [PulseCode::end_marker(); frame_codes(MAX_PIXELS)];
            let codes = encode(pixels, timings, &mut frame)?;
            self.channel.transmit(&frame[..codes]).await
        }
    }

    /// Encode `pixels` into `frame`, returning the frame's length.
    ///
    /// The frame ends with the latch (a low pulse of at least `reset` ticks)
    /// followed by a real end marker: `esp-hal` refuses a slice that does not
    /// end on one, which is what per-pixel `PulseCode` tables without a marker
    /// got wrong.
    fn encode(
        pixels: &[[u8; 3]],
        timings: super::Timings,
        frame: &mut [PulseCode],
    ) -> Result<usize, RmtError> {
        if pixels.len() > MAX_PIXELS || frame.len() < frame_codes(pixels.len()) {
            return Err(RmtError::InvalidArgument);
        }
        let mut codes = 0;
        for pixel in pixels {
            let bits = grb(*pixel);
            for index in 0..24 {
                let (high, low) = if (bits >> (23 - index)) & 1 == 1 {
                    (timings.t1h, timings.t1l)
                } else {
                    (timings.t0h, timings.t0l)
                };
                frame[codes] = PulseCode::new(Level::High, high, Level::Low, low);
                codes += 1;
            }
        }
        frame[codes] = latch(timings);
        codes += 1;
        frame[codes] = PulseCode::end_marker();
        Ok(codes + 1)
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

    #[test]
    fn a_pixel_word_is_green_then_red_then_blue() {
        assert_eq!(grb([32, 0, 0]), 0x00_20_00);
        assert_eq!(grb([0, 32, 0]), 0x20_00_00);
        assert_eq!(grb([0, 0, 32]), 0x00_00_20);
        assert_eq!(grb([0x12, 0x34, 0x56]), 0x34_12_56);
    }

    #[test]
    fn a_frame_has_24_codes_a_pixel_plus_the_latch_and_its_end_marker() {
        assert_eq!(frame_codes(3), 74);
        assert_eq!(frame_codes(1), 26);
        assert_eq!(frame_codes(0), 2);
    }

    #[test]
    fn the_stack_buffer_covers_the_biggest_allowed_frame() {
        assert_eq!(frame_codes(MAX_PIXELS), 386);
    }
}
