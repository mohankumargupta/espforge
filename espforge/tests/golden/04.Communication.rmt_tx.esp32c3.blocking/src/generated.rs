#![allow(dead_code, unused_variables)]
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
    pub ir_tx: espforge_runtime::BlockingRmtTxEndpoint,
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
            .expect("RMT configuration failed");
        Ok(Self {
            ir_tx: espforge_runtime::RmtTxChannel::with_clock(
                esp_hal::rmt::TxChannelCreator::configure_tx(
                        rmt0_bus.channel0,
                        &espforge_runtime::TxChannelConfig::default()
                            .with_clk_divider(1)
                            .with_idle_output(true)
                            .with_idle_output_level(esp_hal::gpio::Level::Low)
                            .with_carrier_modulation(true)
                            .with_carrier_high(1052)
                            .with_carrier_low(1053)
                            .with_carrier_level(esp_hal::gpio::Level::Low),
                    )
                    .expect("RMT TX channel configuration failed")
                    .with_pin(unsafe { registry.peripherals.GPIO4.clone_unchecked() }),
                80000,
                1,
            ),
        })
    }
}
/// Constructed services.
pub struct Services {}
