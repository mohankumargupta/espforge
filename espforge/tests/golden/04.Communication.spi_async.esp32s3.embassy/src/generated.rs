#![allow(dead_code, unused_variables)]
static __SPI2_STORAGE: espforge_runtime::AsyncSpiBusStorage = espforge_runtime::AsyncSpiBusStorage::new();
/// Built during `Devices::new`, initialised after boot.
pub struct AccelFactory {
    endpoint: espforge_drivers::AsyncAdxl345Endpoint,
}
impl espforge_runtime::AsyncDeferredInit for AccelFactory {
    type Device = espforge_drivers::AsyncAdxl345;
    fn init(
        self,
    ) -> impl core::future::Future<
        Output = Result<Self::Device, espforge_runtime::DeviceError>,
    > {
        async move {
            espforge_drivers::AsyncAdxl345::new(self.endpoint)
                .await
                .map_err(|kind| espforge_runtime::DeviceError {
                    device: "accel",
                    kind,
                })
        }
    }
}
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
    pub accel: espforge_runtime::AsyncMutex<
        espforge_runtime::AsyncDeferred<AccelFactory>,
    >,
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
                    frequency: esp_hal::time::Rate::from_khz(5000),
                    mode: esp_hal::spi::Mode::_3,
                },
            )
            .expect("SPI bus configuration failed");
        Ok(Self {
            accel: espforge_runtime::AsyncMutex::new(
                espforge_runtime::AsyncDeferred::new(AccelFactory {
                    endpoint: espforge_runtime::ConfiguredAsyncSpiDevice::new(
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
                }),
            ),
        })
    }
}
/// Constructed services.
pub struct Services {}
