#![allow(dead_code, unused_variables)]
static __SPI2_STORAGE: espforge_runtime::AsyncSpiBusStorage = espforge_runtime::AsyncSpiBusStorage::new();
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
    pub spi2: espforge_runtime::AsyncMutex<espforge_runtime::AsyncSpiDevice>,
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
        let spi2_bus: espforge_runtime::AsyncSpiBus = espforge_runtime::AsyncSpiBus::build_in(
                &__SPI2_STORAGE,
                unsafe { registry.peripherals.SPI2.clone_unchecked() },
                unsafe { registry.peripherals.GPIO8.clone_unchecked() },
                unsafe { registry.peripherals.GPIO7.clone_unchecked() },
                unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                espforge_runtime::SpiConfig {
                    frequency: esp_hal::time::Rate::from_khz(100),
                    mode: esp_hal::spi::Mode::_0,
                },
            )
            .expect("SPI bus configuration failed");
        Ok(Self {
            spi2: espforge_runtime::AsyncMutex::new(
                espforge_runtime::AsyncSpiDevice::new(
                    spi2_bus,
                    esp_hal::gpio::Output::new(
                        unsafe { registry.peripherals.GPIO9.clone_unchecked() },
                        esp_hal::gpio::Level::High,
                        esp_hal::gpio::OutputConfig::default(),
                    ),
                    espforge_runtime::Delay::new(),
                ),
            ),
        })
    }
}
/// Constructed services.
pub struct Services {}
