//! Signal synchronization primitive facade for espforge
//!
//! This module provides a thin wrapper around `embassy_sync::signal::Signal`
//! for use in espforge projects with the embassy runtime.
//!
//! # Example
//! ```rust,no_run
//! use espforge_platform::signal::Signal;
//!
//! enum Command {
//!     On,
//!     Off,
//! }
//!
//! static COMMAND_SIGNAL: Signal<Command> = Signal::new();
//!
//! // In one task:
//! COMMAND_SIGNAL.signal(Command::On);
//!
//! // In another task:
//! let cmd = COMMAND_SIGNAL.wait().await;
//! ```

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal as EmbassySignal;

/// A single-slot signaling primitive for passing the latest value to a task.
///
/// This is a wrapper around `embassy_sync::signal::Signal` that uses
/// `CriticalSectionRawMutex` by default, making it suitable for use across
/// all ESP32 chips supported by espforge.
///
/// When "sending" (via `signal()`) while full, the previous value is overwritten
/// instead of blocking. This makes it ideal for state updates where only the
/// latest value matters.
///
/// # Type Parameters
/// * `T` - The type of value to signal. Must be `Send`.
///
/// # Static Declaration
/// Signals are typically declared as statics:
/// ```rust,no_run
/// use espforge_platform::signal::Signal;
///
/// static MY_SIGNAL: Signal<u32> = Signal::new();
/// ```
pub struct Signal<T: Send> {
    inner: EmbassySignal<CriticalSectionRawMutex, T>,
}

impl<T: Send> Signal<T> {
    /// Create a new `Signal`.
    ///
    /// This is a const function, so it can be used in static initialization.
    ///
    /// # Example
    /// ```rust,no_run
    /// use espforge_platform::signal::Signal;
    ///
    /// static STATE: Signal<bool> = Signal::new();
    /// ```
    pub const fn new() -> Self {
        Self {
            inner: EmbassySignal::new(),
        }
    }

    /// Signal a value to the waiting task.
    ///
    /// If a value was already signaled and not yet retrieved, it will be
    /// overwritten with the new value.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::Signal;
    /// # static SIGNAL: Signal<u32> = Signal::new();
    /// SIGNAL.signal(42);
    /// ```
    pub fn signal(&self, val: T) {
        self.inner.signal(val)
    }

    /// Wait for a signal and retrieve its value.
    ///
    /// This is an async function that will suspend the current task until
    /// a value is signaled. Once retrieved, the signal is cleared.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::Signal;
    /// # static SIGNAL: Signal<u32> = Signal::new();
    /// # async fn example() {
    /// let value = SIGNAL.wait().await;
    /// println!("Received: {}", value);
    /// # }
    /// ```
    pub async fn wait(&self) -> T {
        self.inner.wait().await
    }

    /// Try to take the signaled value without waiting.
    ///
    /// Returns `Some(T)` if a value was signaled, `None` otherwise.
    /// If a value is returned, the signal is cleared.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::Signal;
    /// # static SIGNAL: Signal<u32> = Signal::new();
    /// if let Some(value) = SIGNAL.try_take() {
    ///     println!("Got value: {}", value);
    /// } else {
    ///     println!("No signal pending");
    /// }
    /// ```
    pub fn try_take(&self) -> Option<T> {
        self.inner.try_take()
    }

    /// Check if a signal is pending without clearing it.
    ///
    /// Returns `true` if a value has been signaled and not yet retrieved.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::Signal;
    /// # static SIGNAL: Signal<u32> = Signal::new();
    /// if SIGNAL.signaled() {
    ///     println!("Signal is pending");
    /// }
    /// ```
    pub fn signaled(&self) -> bool {
        self.inner.signaled()
    }

    /// Clear any pending signal.
    ///
    /// After calling this, `signaled()` will return `false` and `try_take()`
    /// will return `None` until a new value is signaled.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::Signal;
    /// # static SIGNAL: Signal<u32> = Signal::new();
    /// SIGNAL.reset();
    /// ```
    pub fn reset(&self) {
        self.inner.reset()
    }
}

impl<T: Send> Default for Signal<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A simplified signal for notification-only use cases (no data).
///
/// This is a convenience wrapper around `Signal<()>` for cases where you
/// only need to notify that an event occurred, without passing any data.
///
/// # Example
/// ```rust,no_run
/// use espforge_platform::signal::SimpleSignal;
///
/// static BUTTON_PRESSED: SimpleSignal = SimpleSignal::new();
///
/// // In interrupt or button task:
/// BUTTON_PRESSED.signal();
///
/// // In another task:
/// BUTTON_PRESSED.wait().await;
/// println!("Button was pressed!");
/// ```
pub struct SimpleSignal {
    inner: Signal<()>,
}

impl SimpleSignal {
    /// Create a new `SimpleSignal`.
    ///
    /// This is a const function, so it can be used in static initialization.
    ///
    /// # Example
    /// ```rust,no_run
    /// use espforge_platform::signal::SimpleSignal;
    ///
    /// static EVENT: SimpleSignal = SimpleSignal::new();
    /// ```
    pub const fn new() -> Self {
        Self {
            inner: Signal::new(),
        }
    }

    /// Signal that an event has occurred.
    ///
    /// This notifies any task waiting on this signal.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::SimpleSignal;
    /// # static EVENT: SimpleSignal = SimpleSignal::new();
    /// EVENT.signal();
    /// ```
    pub fn signal(&self) {
        self.inner.signal(())
    }

    /// Wait for the signal.
    ///
    /// This is an async function that will suspend the current task until
    /// the signal is triggered.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::SimpleSignal;
    /// # static EVENT: SimpleSignal = SimpleSignal::new();
    /// # async fn example() {
    /// EVENT.wait().await;
    /// println!("Event occurred!");
    /// # }
    /// ```
    pub async fn wait(&self) {
        self.inner.wait().await
    }

    /// Try to check if the signal was triggered without waiting.
    ///
    /// Returns `true` if the signal was triggered and clears it.
    /// Returns `false` if no signal is pending.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::SimpleSignal;
    /// # static EVENT: SimpleSignal = SimpleSignal::new();
    /// if EVENT.try_take() {
    ///     println!("Event was pending");
    /// }
    /// ```
    pub fn try_take(&self) -> bool {
        self.inner.try_take().is_some()
    }

    /// Check if a signal is pending without clearing it.
    ///
    /// Returns `true` if the signal has been triggered and not yet cleared.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::SimpleSignal;
    /// # static EVENT: SimpleSignal = SimpleSignal::new();
    /// if EVENT.signaled() {
    ///     println!("Event is pending");
    /// }
    /// ```
    pub fn signaled(&self) -> bool {
        self.inner.signaled()
    }

    /// Clear any pending signal.
    ///
    /// After calling this, `signaled()` will return `false` and `try_take()`
    /// will return `false` until the signal is triggered again.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use espforge_platform::signal::SimpleSignal;
    /// # static EVENT: SimpleSignal = SimpleSignal::new();
    /// EVENT.reset();
    /// ```
    pub fn reset(&self) {
        self.inner.reset()
    }
}

impl Default for SimpleSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Macro for declaring signals with convenient syntax.
///
/// # Syntax
/// - `signal!(NAME)` - Creates a `SimpleSignal` (notification-only)
/// - `signal!(NAME, Type)` - Creates a `Signal<Type>` (with data)
///
/// # Examples
///
/// ```rust,no_run
/// use espforge_platform::signal;
///
/// // Simple notification signal
/// signal!(BUTTON_PRESSED);
///
/// // Signal with data
/// signal!(LED_BRIGHTNESS, u8);
/// signal!(MOTOR_SPEED, u32);
///
/// enum Command {
///     Start,
///     Stop,
/// }
/// signal!(SYSTEM_CMD, Command);
/// ```
///
/// The macro expands to:
/// ```rust,ignore
/// static BUTTON_PRESSED: espforge_platform::signal::SimpleSignal =
///     espforge_platform::signal::SimpleSignal::new();
///
/// static LED_BRIGHTNESS: espforge_platform::signal::Signal<u8> =
///     espforge_platform::signal::Signal::new();
/// ```
#[macro_export]
macro_rules! signal {
    // Simple signal (no data type) - expands to SimpleSignal
    ($name:ident) => {
        static $name: $crate::signal::SimpleSignal = $crate::signal::SimpleSignal::new();
    };

    // Signal with data type - expands to Signal<T>
    ($name:ident, $ty:ty) => {
        static $name: $crate::signal::Signal<$ty> = $crate::signal::Signal::new();
    };
}

// Re-export the macro at module level for use with `use espforge_platform::signal::signal;`
pub use signal;

// Re-export the underlying embassy signal for advanced use cases
pub use embassy_sync::signal::Signal as RawSignal;
//pub use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_creation() {
        let _signal: Signal<u32> = Signal::new();
    }

    #[test]
    fn test_try_take_empty() {
        let signal: Signal<u32> = Signal::new();
        assert_eq!(signal.try_take(), None);
        assert!(!signal.signaled());
    }

    #[test]
    fn test_signal_and_take() {
        let signal: Signal<u32> = Signal::new();
        signal.signal(42);
        assert!(signal.signaled());
        assert_eq!(signal.try_take(), Some(42));
        assert!(!signal.signaled());
    }

    #[test]
    fn test_signal_overwrite() {
        let signal: Signal<u32> = Signal::new();
        signal.signal(1);
        signal.signal(2);
        signal.signal(3);
        // Only the last value should be available
        assert_eq!(signal.try_take(), Some(3));
        assert_eq!(signal.try_take(), None);
    }

    #[test]
    fn test_reset() {
        let signal: Signal<u32> = Signal::new();
        signal.signal(42);
        assert!(signal.signaled());
        signal.reset();
        assert!(!signal.signaled());
        assert_eq!(signal.try_take(), None);
    }

    // SimpleSignal tests
    #[test]
    fn test_simple_signal_creation() {
        let _signal = SimpleSignal::new();
    }

    #[test]
    fn test_simple_signal_try_take_empty() {
        let signal = SimpleSignal::new();
        assert!(!signal.try_take());
        assert!(!signal.signaled());
    }

    #[test]
    fn test_simple_signal_and_take() {
        let signal = SimpleSignal::new();
        signal.signal();
        assert!(signal.signaled());
        assert!(signal.try_take());
        assert!(!signal.signaled());
    }

    #[test]
    fn test_simple_signal_reset() {
        let signal = SimpleSignal::new();
        signal.signal();
        assert!(signal.signaled());
        signal.reset();
        assert!(!signal.signaled());
        assert!(!signal.try_take());
    }

    // Macro tests
    #[test]
    fn test_signal_macro_simple() {
        signal!(TEST_EVENT);
        TEST_EVENT.signal();
        assert!(TEST_EVENT.signaled());
        assert!(TEST_EVENT.try_take());
    }

    #[test]
    fn test_signal_macro_with_type() {
        signal!(TEST_VALUE, u32);
        TEST_VALUE.signal(42);
        assert!(TEST_VALUE.signaled());
        assert_eq!(TEST_VALUE.try_take(), Some(42));
    }

    #[test]
    fn test_signal_macro_with_enum() {
        #[derive(Debug, PartialEq)]
        enum TestCmd {
            Start,
            Stop,
        }

        signal!(TEST_CMD, TestCmd);
        TEST_CMD.signal(TestCmd::Start);
        assert_eq!(TEST_CMD.try_take(), Some(TestCmd::Start));

        TEST_CMD.signal(TestCmd::Stop);
        assert_eq!(TEST_CMD.try_take(), Some(TestCmd::Stop));
    }
}
