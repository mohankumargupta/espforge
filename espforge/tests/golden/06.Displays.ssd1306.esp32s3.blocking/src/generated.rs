#![allow(dead_code, unused_variables)]
static __I2C0_STORAGE: espforge_runtime::I2cBusStorage = espforge_runtime::I2cBusStorage::new();
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
    pub oled: espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>,
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
        let i2c0_bus: espforge_runtime::I2cBus = espforge_runtime::I2cBus::build_in(
                &__I2C0_STORAGE,
                unsafe { registry.peripherals.I2C0.clone_unchecked() },
                unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                unsafe { registry.peripherals.GPIO7.clone_unchecked() },
                espforge_runtime::I2cConfig {
                    frequency: esp_hal::time::Rate::from_khz(400),
                },
            )
            .expect("I2C bus configuration failed");
        Ok(Self {
            oled: espforge_drivers::Ssd1306Display::new(
                    espforge_runtime::embedded_hal_bus::i2c::RefCellDevice::new(
                        i2c0_bus.cell(),
                    ),
                    60,
                    espforge_drivers::Oled128x64,
                    espforge_drivers::DisplayRotation::Rotate0,
                )
                .map_err(|kind| espforge_runtime::DeviceError {
                    device: "oled",
                    kind,
                })?,
        })
    }
}
/// Constructed services.
pub struct Services {}
