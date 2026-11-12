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
    pub clock: espforge_runtime::NtpClient,
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
        let clock = espforge_runtime::NtpClient::new(
            home.stack(),
            espforge_runtime::NtpConfig {
                server: "pool.ntp.org",
                port: 123u32 as u16,
                timeout_ms: 5000u32 as u64,
            },
        );
        Self { home, clock: clock }
    }
}
