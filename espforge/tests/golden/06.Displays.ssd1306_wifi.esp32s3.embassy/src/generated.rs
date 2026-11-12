#![allow(dead_code, unused_variables)]
static __HOME_STORAGE: espforge_runtime::WifiStorage = espforge_runtime::WifiStorage::new();
static __I2C0_STORAGE: espforge_runtime::AsyncI2cBusStorage = espforge_runtime::AsyncI2cBusStorage::new();
/// Built during `Devices::new`, initialised after boot.
pub struct OledFactory {
    endpoint: espforge_drivers::AsyncI2cEndpoint,
}
impl espforge_runtime::AsyncDeferredInit for OledFactory {
    type Device = espforge_drivers::AsyncSsd1306Display<espforge_drivers::Oled128x64>;
    fn init(
        self,
    ) -> impl core::future::Future<
        Output = Result<Self::Device, espforge_runtime::DeviceError>,
    > {
        async move {
            espforge_drivers::AsyncSsd1306Display::new(
                    self.endpoint,
                    60,
                    espforge_drivers::Oled128x64,
                    espforge_drivers::DisplayRotation::Rotate0,
                )
                .await
                .map_err(|kind| espforge_runtime::DeviceError {
                    device: "oled",
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
    pub oled: espforge_runtime::AsyncMutex<espforge_runtime::AsyncDeferred<OledFactory>>,
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
        let i2c0_bus: espforge_runtime::AsyncI2cBus = espforge_runtime::AsyncI2cBus::build_in(
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
            oled: espforge_runtime::AsyncMutex::new(
                espforge_runtime::AsyncDeferred::new(OledFactory { endpoint: i2c0_bus }),
            ),
        })
    }
}
/// Constructed services.
pub struct Services {
    pub home: espforge_runtime::WifiStation,
}
impl Services {
    pub fn new() -> Self {
        let home = espforge_runtime::WifiStation::new(
            espforge_runtime::WifiConfig {
                ssid: "Wokwi-GUEST",
                password: "",
            },
            &__HOME_STORAGE,
        );
        Self { home }
    }
}
