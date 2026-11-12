#![allow(dead_code, unused_variables)]
static __SPI2_STORAGE: espforge_runtime::SpiBusStorage = espforge_runtime::SpiBusStorage::new();
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
    pub tft: espforge_drivers::Ili9341Display,
    pub accel: espforge_drivers::Adxl345,
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
        let spi2_bus: espforge_runtime::SpiBus = espforge_runtime::SpiBus::build_in(
                &__SPI2_STORAGE,
                unsafe { registry.peripherals.SPI2.clone_unchecked() },
                unsafe { registry.peripherals.GPIO8.clone_unchecked() },
                unsafe { registry.peripherals.GPIO7.clone_unchecked() },
                unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                espforge_runtime::SpiConfig {
                    frequency: esp_hal::time::Rate::from_khz(40000),
                    mode: esp_hal::spi::Mode::_0,
                },
            )
            .expect("SPI bus configuration failed");
        Ok(Self {
            tft: espforge_drivers::Ili9341Display::new(
                    espforge_runtime::ConfiguredSpiDevice::new(
                        spi2_bus,
                        esp_hal::gpio::Output::new(
                            unsafe { registry.peripherals.GPIO9.clone_unchecked() },
                            esp_hal::gpio::Level::High,
                            esp_hal::gpio::OutputConfig::default(),
                        ),
                        espforge_runtime::Delay::new(),
                        espforge_runtime::SpiConfig {
                            frequency: esp_hal::time::Rate::from_khz(40000),
                            mode: esp_hal::spi::Mode::_0,
                        },
                        espforge_runtime::CsPolarity::ActiveLow,
                    ),
                    esp_hal::gpio::Output::new(
                        unsafe { registry.peripherals.GPIO4.clone_unchecked() },
                        esp_hal::gpio::Level::Low,
                        esp_hal::gpio::OutputConfig::default(),
                    ),
                    esp_hal::gpio::Output::new(
                        unsafe { registry.peripherals.GPIO5.clone_unchecked() },
                        esp_hal::gpio::Level::High,
                        esp_hal::gpio::OutputConfig::default(),
                    ),
                    Some(
                        esp_hal::gpio::Output::new(
                            unsafe { registry.peripherals.GPIO3.clone_unchecked() },
                            esp_hal::gpio::Level::High,
                            esp_hal::gpio::OutputConfig::default(),
                        ),
                    ),
                    espforge_drivers::Orientation::Landscape,
                    espforge_drivers::ColorOrder::Bgr,
                )
                .map_err(|kind| espforge_runtime::DeviceError {
                    device: "tft",
                    kind,
                })?,
            accel: espforge_drivers::Adxl345::new(
                    espforge_runtime::ConfiguredSpiDevice::new(
                        spi2_bus,
                        esp_hal::gpio::Output::new(
                            unsafe { registry.peripherals.GPIO10.clone_unchecked() },
                            esp_hal::gpio::Level::High,
                            esp_hal::gpio::OutputConfig::default(),
                        ),
                        espforge_runtime::Delay::new(),
                        espforge_runtime::SpiConfig {
                            frequency: esp_hal::time::Rate::from_khz(5000),
                            mode: esp_hal::spi::Mode::_3,
                        },
                        espforge_runtime::CsPolarity::ActiveLow,
                    ),
                )
                .map_err(|kind| espforge_runtime::DeviceError {
                    device: "accel",
                    kind,
                })?,
        })
    }
}
/// Constructed services.
pub struct Services {}
