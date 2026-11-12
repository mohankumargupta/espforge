#![allow(dead_code, unused_variables)]
/// Raw ESP32 peripherals, moved out of `esp_hal::peripherals::Peripherals` once at init.
pub struct PeripheralRegistry {
    pub peripherals: esp_hal::peripherals::Peripherals,
}
impl PeripheralRegistry {
    pub fn new(peripherals: esp_hal::peripherals::Peripherals) -> Self {
        Self { peripherals }
    }
}
/// Constructed devices.
pub struct Devices {
    pub red_led: espforge_runtime::AsyncMutex<espforge_runtime::Led>,
    pub button: espforge_runtime::AsyncMutex<espforge_runtime::Button>,
}
impl Devices {
    /// Build every declared device.
    ///
    /// Fallible: a part that does not answer is a named
    /// `DeviceError`, not a panic. The boot site decides whether
    /// to stop there.
    pub fn new(
        registry: &mut PeripheralRegistry,
    ) -> Result<Self, espforge_runtime::DeviceError> {
        Ok(Self {
            red_led: espforge_runtime::AsyncMutex::new(
                espforge_runtime::Led::new(
                    unsafe { registry.peripherals.GPIO7.clone_unchecked() },
                    false,
                ),
            ),
            button: espforge_runtime::AsyncMutex::new(
                espforge_runtime::Button::new(
                    unsafe { registry.peripherals.GPIO9.clone_unchecked() },
                    true,
                ),
            ),
        })
    }
}
/// Constructed services.
pub struct Services {}
