//! Wall time, and the NTP era it lives in.
//!
//! There is no RTC on the board, so time comes from one of two places:
//!
//! - [`Clock`]: the anchor a successful SNTP query wrote, advanced by the
//!   monotonic clock. Reading it costs nothing and needs no radio.
//! - [`era_hint`]: the NTP era the *firmware was built in*. Before the first
//!   sync it is the only absolute-date information available, and it is what
//!   lets `sntpc` resolve a server reply correctly after the wire timestamp
//!   rolls over.
//!
//! The era belongs to the 64-bit NTP timestamp: one era is 2³² seconds, so era
//! 0 covers 1900-01-01 through 2036-02-07T06:28:16Z. The better-known 2038
//! date is the *signed 32-bit Unix* overflow — a different limit, and one an
//! anchor stored as `u64` Unix seconds never has to care about.

/// Seconds between the Unix epoch (1970-01-01) and the NTP epoch (1900-01-01).
pub const NTP_UNIX_DELTA: u64 = 2_208_988_800;

/// One NTP era, in seconds.
pub const SECONDS_PER_ERA: u64 = 1 << 32;

/// A point in wall time, as Unix seconds plus a sub-second fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnixTime {
    /// Seconds since the Unix epoch.
    pub seconds: u64,
    /// Sub-second fraction in microseconds (`0..1_000_000`).
    pub micros: u32,
}

impl UnixTime {
    /// 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: Self = Self {
        seconds: 0,
        micros: 0,
    };

    pub const fn new(seconds: u64, micros: u32) -> Self {
        Self { seconds, micros }
    }

    /// Total microseconds since the Unix epoch, or `None` on overflow.
    pub fn as_micros(&self) -> Option<u64> {
        self.seconds
            .checked_mul(1_000_000)?
            .checked_add(self.micros as u64)
    }

    /// Split total microseconds since the Unix epoch into seconds and micros.
    pub fn from_micros(total: u64) -> Self {
        Self {
            seconds: total / 1_000_000,
            micros: (total % 1_000_000) as u32,
        }
    }
}

/// Why a wall-clock read failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    /// No successful `sync` since boot.
    NotSynced,
    /// The anchor and the monotonic reading do not fit in `u64` microseconds.
    Overflow,
}

impl core::fmt::Display for ClockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ClockError::NotSynced => f.write_str("the clock has not been synchronised"),
            ClockError::Overflow => f.write_str("the clock reading overflowed"),
        }
    }
}

/// The NTP era a Unix instant belongs to.
///
/// Era 0 is 1900-01-01 through 2036-02-07T06:28:16Z; era 1 starts at
/// 2036-02-07T06:28:16Z (Unix `2_085_978_496`), and era 2 at
/// 2172-02-07T06:28:16Z.
pub const fn era_for(unix_seconds: u64) -> u8 {
    ((unix_seconds + NTP_UNIX_DELTA) / SECONDS_PER_ERA) as u8
}

/// The Unix second an era starts at, saturating at the Unix epoch for era 0.
pub const fn era_start_unix(era: u8) -> u64 {
    (era as u64 * SECONDS_PER_ERA).saturating_sub(NTP_UNIX_DELTA)
}

/// The era this firmware was built in.
///
/// `espforge-runtime`'s build script records the build time in
/// `ESPFORGE_BUILD_UNIX` (honouring `SOURCE_DATE_EPOCH` for reproducible
/// builds), so a firmware built after a rollover resolves the new era without
/// regenerating the project — and `generated.rs` never carries a timestamp.
pub fn era_hint() -> u8 {
    era_for(env!("ESPFORGE_BUILD_UNIX").parse().unwrap_or(0))
}

/// A wall time pinned to a monotonic reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    time: UnixTime,
    at_micros: u64,
}

impl Anchor {
    pub const fn new(time: UnixTime, at_micros: u64) -> Self {
        Self { time, at_micros }
    }

    pub const fn time(&self) -> UnixTime {
        self.time
    }

    /// The anchored time plus the monotonic time elapsed since it was set.
    pub fn advance(&self, now_micros: u64) -> Result<UnixTime, ClockError> {
        let elapsed = now_micros
            .checked_sub(self.at_micros)
            .ok_or(ClockError::Overflow)?;
        let total = self
            .time
            .as_micros()
            .and_then(|anchor| anchor.checked_add(elapsed))
            .ok_or(ClockError::Overflow)?;
        Ok(UnixTime::from_micros(total))
    }
}

#[cfg(feature = "embassy")]
mod wall {
    use core::cell::RefCell;

    use embassy_time::Instant;

    use super::{Anchor, ClockError, UnixTime};

    /// The clock a successful SNTP query writes.
    ///
    /// One per [`NtpClient`](crate::NtpClient): two clients that sync to two
    /// servers keep two anchors rather than overwriting each other. The anchor
    /// sits in a `RefCell` so `now()` takes `&self`, exactly like the rest of
    /// the shared `Context`.
    pub struct Clock {
        anchor: RefCell<Option<Anchor>>,
    }

    impl Clock {
        pub fn new() -> Self {
            Self {
                anchor: RefCell::new(None),
            }
        }

        /// Anchor `time` to the monotonic clock, now.
        pub fn set(&self, time: UnixTime) {
            self.set_at(time, Instant::now().as_micros());
        }

        /// `set`, with an explicit monotonic reading (used by tests).
        pub fn set_at(&self, time: UnixTime, now_micros: u64) {
            *self.anchor.borrow_mut() = Some(Anchor::new(time, now_micros));
        }

        /// The anchored time plus elapsed monotonic time, with no I/O.
        pub fn now(&self) -> Result<UnixTime, ClockError> {
            self.now_at(Instant::now().as_micros())
        }

        /// `now`, with an explicit monotonic reading (used by tests).
        pub fn now_at(&self, now_micros: u64) -> Result<UnixTime, ClockError> {
            match self.anchor.borrow().as_ref() {
                Some(anchor) => anchor.advance(now_micros),
                None => Err(ClockError::NotSynced),
            }
        }

        pub fn is_synced(&self) -> bool {
            self.anchor.borrow().is_some()
        }
    }

    impl Default for Clock {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(feature = "embassy")]
pub use wall::Clock;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eras_roll_where_the_ntp_timestamp_rolls() {
        assert_eq!(era_for(0), 0, "1970 is era 0");
        assert_eq!(era_for(2_085_978_495), 0, "one second before 2036");
        assert_eq!(era_for(2_085_978_496), 1, "2036-02-07T06:28:16Z is era 1");
        assert_eq!(
            era_for(2_085_978_496 + SECONDS_PER_ERA),
            2,
            "2172 starts era 2"
        );
        // `era_start_unix` is the inverse at the boundaries.
        assert_eq!(era_start_unix(0), 0);
        assert_eq!(era_start_unix(1), 2_085_978_496);
        assert_eq!(era_start_unix(2), 2_085_978_496 + SECONDS_PER_ERA);
    }

    #[test]
    fn an_anchor_advances_with_the_monotonic_clock() {
        let anchor = Anchor::new(UnixTime::new(1_800_000_000, 500_000), 10_000);
        assert_eq!(
            anchor.advance(10_500).unwrap(),
            UnixTime::new(1_800_000_000, 500_500)
        );
        assert_eq!(
            anchor.advance(12_000_000).unwrap(),
            UnixTime::new(1_800_000_012, 490_000)
        );
        // A reading before the anchor cannot be elapsed time.
        assert_eq!(anchor.advance(9_999), Err(ClockError::Overflow));
    }

    #[test]
    fn the_era_hint_is_a_small_integer() {
        // Whatever the checkout's build date, the value is a valid era index;
        // a leaked build timestamp would fail this.
        let hint = era_hint();
        assert!(hint <= 2, "a firmware built in era {hint} is out of range");
    }

    #[cfg(feature = "embassy")]
    #[test]
    fn reading_without_a_sync_is_an_error_then_works() {
        let clock = Clock::new();
        assert_eq!(clock.now_at(0), Err(ClockError::NotSynced));
        assert!(!clock.is_synced());

        clock.set_at(UnixTime::new(1_800_000_000, 0), 0);
        assert!(clock.is_synced());
        assert_eq!(
            clock.now_at(2_000_000).unwrap(),
            UnixTime::new(1_800_000_002, 0)
        );
    }
}
