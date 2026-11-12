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
    pub secure_client: espforge_runtime::HttpsClient,
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
        let secure_client = espforge_runtime::HttpsClient::new(
            home.stack(),
            espforge_runtime::HttpsClientConfig {
                rx_buffer_size: 4096u32 as usize,
                tx_buffer_size: 2048u32 as usize,
                timeout_ms: 15000u32 as u64,
            },
            true,
        );
        Self {
            home,
            secure_client: secure_client,
        }
    }
}
