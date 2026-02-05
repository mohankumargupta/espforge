use core::cell::RefCell;
use esp_hal::gpio::{AnyPin, Event, Input, InputConfig, Output, OutputConfig, Pull};
use embedded_hal::digital::{ErrorType, InputPin, OutputPin, StatefulOutputPin};

/// Async-capable GPIO Output wrapper
/// 
/// This wrapper provides async capabilities for GPIO output pins when using Embassy.
/// Currently provides the same blocking interface as GPIOOutput, but structured
/// to support future async output operations (e.g., async PWM, async protocols).
pub struct GPIOOutput {
    output: Output<'static>,
}

impl GPIOOutput {
    /// Creates a wrapper from an existing owned pin (Registry Pattern)
    pub fn from_pin(pin: AnyPin<'static>) -> Self {
        let output = Output::new(pin, esp_hal::gpio::Level::Low,  OutputConfig::default());
        Self { output }
    }

    pub fn new(pin_number: u8) -> Self {
        let pin = unsafe { AnyPin::steal(pin_number) };
        Self::from_pin(pin)
    }
}

impl ErrorType for GPIOOutput {
    type Error = core::convert::Infallible;
}

impl OutputPin for GPIOOutput {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.output.set_low())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.output.set_high())
    }
}

impl StatefulOutputPin for GPIOOutput {
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.output.is_set_high())
    }

    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.output.is_set_low())
    }

    fn toggle(&mut self) -> Result<(), Self::Error> {
        Ok(self.output.toggle())
    }
}

/// Async-capable GPIO Input wrapper
/// 
/// This wrapper provides interrupt-driven async GPIO operations using esp-hal's Flex type.
/// It supports efficient waiting for GPIO events without polling.
/// 
/// # Important: Not Cancellation-Safe
/// 
/// The async wait methods are **NOT** cancellation-safe. From esp-hal documentation:
/// 
/// > The GPIO driver will disable listening for the event once it occurs,
/// > or if the `Future` is dropped - which also means this method is **not**
/// > cancellation-safe, it will always wait for a future event.
/// 
/// This means:
/// - Dropping the future will cancel the wait operation
/// - Events that occur while no future is active will be lost
/// - Use caution with timeouts and `select!` operations
/// 
/// # Example
/// 
/// ```ignore
/// use espforge_platform::gpio::GPIOInput;
/// 
/// let mut button = GPIOInput::new(9, true, false);
/// 
/// // Wait for button press (falling edge with pull-up)
/// button.wait_for_falling_edge().await;
/// println!("Button pressed!");
/// 
/// // Wait for any change
/// button.wait_for_any_edge().await;
/// ```
pub struct GPIOInput {
    input: Input<'static>,
}

impl GPIOInput {
    /// Creates a wrapper from an existing owned pin
    /// 
    /// # Arguments
    /// 
    /// * `pin` - The GPIO pin to use
    /// * `pull_up` - Enable internal pull-up resistor
    /// * `pull_down` - Enable internal pull-down resistor
    pub fn from_pin(pin: AnyPin<'static>, pull_up: bool, pull_down: bool) -> Self {
        let pull = match (pull_up, pull_down) {
            (true, false) => Pull::Up,
            (false, true) => Pull::Down,
            _ => Pull::None,
        };

        let config = InputConfig::default().with_pull(pull);
        let input = Input::new(pin, config);
        // let mut flex = Flex::new(pin);
        // flex.set_as_input(config);
        
        Self { input }
    }

    /// Creates a new GPIO input from a pin number
    /// 
    /// # Safety
    /// 
    /// This uses `AnyPin::steal()` internally. Ensure the pin is not used elsewhere.
    pub fn new(pin_number: u8, pull_up: bool, pull_down: bool) -> Self {
        let pin = unsafe { AnyPin::steal(pin_number) };
        Self::from_pin(pin, pull_up, pull_down)
    }

    /// Wait until the pin experiences a particular event
    /// 
    /// # Cancellation Safety
    /// 
    /// ⚠️ **NOT CANCELLATION-SAFE**: Dropping the future will cancel the wait,
    /// and if the event occurs after dropping, a subsequent wait will ignore it.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// use esp_hal::gpio::Event;
    /// 
    /// // Wait for rising edge
    /// button.wait_for(Event::RisingEdge).await;
    /// ```
    pub async fn wait_for(&mut self, event: Event) {
        self.input.wait_for(event).await
    }

    /// Wait until the pin is high
    /// 
    /// # Cancellation Safety
    /// 
    /// ⚠️ **NOT CANCELLATION-SAFE**: See [`wait_for`](Self::wait_for) for details.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// button.wait_for_high().await;
    /// println!("Pin is now high");
    /// ```
    pub async fn wait_for_high(&mut self) {
        self.input.wait_for_high().await
    }

    /// Wait until the pin is low
    /// 
    /// # Cancellation Safety
    /// 
    /// ⚠️ **NOT CANCELLATION-SAFE**: See [`wait_for`](Self::wait_for) for details.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// button.wait_for_low().await;
    /// println!("Pin is now low");
    /// ```
    pub async fn wait_for_low(&mut self) {
        self.input.wait_for_low().await
    }

    /// Wait for the pin to undergo a transition from low to high
    /// 
    /// # Cancellation Safety
    /// 
    /// ⚠️ **NOT CANCELLATION-SAFE**: See [`wait_for`](Self::wait_for) for details.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// // Wait for button press (with pull-down)
    /// button.wait_for_rising_edge().await;
    /// ```
    pub async fn wait_for_rising_edge(&mut self) {
        self.input.wait_for_rising_edge().await
    }

    /// Wait for the pin to undergo a transition from high to low
    /// 
    /// # Cancellation Safety
    /// 
    /// ⚠️ **NOT CANCELLATION-SAFE**: See [`wait_for`](Self::wait_for) for details.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// // Wait for button press (with pull-up)
    /// button.wait_for_falling_edge().await;
    /// ```
    pub async fn wait_for_falling_edge(&mut self) {
        self.input.wait_for_falling_edge().await
    }

    /// Wait for the pin to undergo any transition (low to high OR high to low)
    /// 
    /// # Cancellation Safety
    /// 
    /// ⚠️ **NOT CANCELLATION-SAFE**: See [`wait_for`](Self::wait_for) for details.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// // Count edges
    /// let mut count = 0;
    /// loop {
    ///     button.wait_for_any_edge().await;
    ///     count += 1;
    ///     println!("Edge {}", count);
    /// }
    /// ```
    pub async fn wait_for_any_edge(&mut self) {
        self.input.wait_for_any_edge().await
    }

    // pub fn from_inner(input: Input<'static>) -> Self {
    //     Self { input }
    // }

    // /// Convert to blocking GPIO input (consumes self)
    // /// 
    // /// This is useful if you need to switch from async to blocking mode.
    // pub fn into_blocking(self) -> super::blocking::GPIOInput {
    //     super::blocking::GPIOInput::from_inner( self.input)
    // }
}

impl ErrorType for GPIOInput {
    type Error = core::convert::Infallible;
}

impl InputPin for GPIOInput {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input.is_high())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input.is_low())
    }
}
