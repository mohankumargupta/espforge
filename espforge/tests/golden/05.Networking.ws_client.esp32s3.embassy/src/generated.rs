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
    pub ws: espforge_runtime::WsClient,
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
        let ws = espforge_runtime::WsClient::new(
            home.stack(),
            espforge_runtime::WsClientConfig {
                rx_buffer_size: 2048u32 as usize,
                tx_buffer_size: 1024u32 as usize,
                timeout_ms: 10000u32 as u64,
            },
        );
        Self { home, ws: ws }
    }
}
