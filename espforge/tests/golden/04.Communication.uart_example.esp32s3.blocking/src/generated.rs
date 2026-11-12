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
    pub uart1: espforge_runtime::UartDevice<esp_hal::Blocking>,
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
            uart1: espforge_runtime::UartDevice::build(
                    unsafe { registry.peripherals.UART1.clone_unchecked() },
                    unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                    unsafe { registry.peripherals.GPIO5.clone_unchecked() },
                    espforge_runtime::UartConfig {
                        baudrate: 9600,
                        data_bits: esp_hal::uart::DataBits::_8,
                        parity: esp_hal::uart::Parity::None,
                        stop_bits: esp_hal::uart::StopBits::_1,
                    },
                )
                .expect("UART configuration failed"),
        })
    }
}
/// Constructed services.
pub struct Services {}
