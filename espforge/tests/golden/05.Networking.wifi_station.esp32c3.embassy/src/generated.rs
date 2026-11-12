#![allow(dead_code, unused_variables)]
static __HOME_STORAGE: espforge_runtime::WifiStorage = espforge_runtime::WifiStorage::new();
/// Raw ESP32 peripherals, moved out of `esp_hal::peripherals::Peripherals` once at init.
pub struct PeripheralRegistry {
    pub peripherals: esp_hal::peripherals::Peripherals,
}
impl PeripheralRegistry {
    pub fn new(peripherals: esp_hal::peripherals::Peripherals) -> Self {
        Self { peripherals }
    }
}
/// Constructed devices (none yet).
pub struct Devices {}
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
