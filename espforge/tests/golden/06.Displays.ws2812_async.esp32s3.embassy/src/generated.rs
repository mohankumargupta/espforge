#![allow(dead_code, unused_variables)]
/// Built during `Devices::new`, initialised after boot.
pub struct StripFactory {
    endpoint: espforge_runtime::AsyncRmtTxEndpoint,
}
impl espforge_runtime::AsyncDeferredInit for StripFactory {
    type Device = espforge_drivers::AsyncWs2812Rmt;
    fn init(
        self,
    ) -> impl core::future::Future<
        Output = Result<Self::Device, espforge_runtime::DeviceError>,
    > {
        async move {
            espforge_drivers::AsyncWs2812Rmt::new(self.endpoint)
                .await
                .map_err(|kind| espforge_runtime::DeviceError {
                    device: "strip",
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
    pub strip: espforge_runtime::AsyncMutex<
        espforge_runtime::AsyncDeferred<StripFactory>,
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
        let rmt0_bus = esp_hal::rmt::Rmt::new(
                unsafe { registry.peripherals.RMT.clone_unchecked() },
                esp_hal::time::Rate::from_khz(80000),
            )
            .expect("RMT configuration failed")
            .into_async();
        Ok(Self {
            strip: espforge_runtime::AsyncMutex::new(
                espforge_runtime::AsyncDeferred::new(StripFactory {
                    endpoint: espforge_runtime::RmtTxChannel::with_clock(
                        esp_hal::rmt::TxChannelCreator::configure_tx(
                                rmt0_bus.channel0,
                                &espforge_runtime::TxChannelConfig::default()
                                    .with_clk_divider(1)
                                    .with_idle_output(true)
                                    .with_idle_output_level(esp_hal::gpio::Level::Low),
                            )
                            .expect("RMT TX channel configuration failed")
                            .with_pin(unsafe {
                                registry.peripherals.GPIO8.clone_unchecked()
                            }),
                        80000,
                        1,
                    ),
                }),
            ),
        })
    }
}
/// Constructed services.
pub struct Services {}
