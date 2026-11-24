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
    pub uart1: espforge_runtime::UartDevice<esp_hal::Blocking>,
    pub uart_tx: espforge_runtime::BlockingRmtTxEndpoint,
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
            uart1: espforge_runtime::UartDevice::build(
                    unsafe { registry.peripherals.UART1.clone_unchecked() },
                    unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                    unsafe { registry.peripherals.GPIO5.clone_unchecked() },
                    espforge_runtime::UartConfig {
                        baudrate: 115200,
                        data_bits: esp_hal::uart::DataBits::_8,
                        parity: esp_hal::uart::Parity::None,
                        stop_bits: esp_hal::uart::StopBits::_1,
                    },
                )
                .expect("UART configuration failed"),
            uart_tx: espforge_runtime::RmtTxChannel::with_clock(
                esp_hal::rmt::TxChannelCreator::configure_tx(
                        rmt0_bus.channel0,
                        &espforge_runtime::TxChannelConfig::default()
                            .with_clk_divider(1)
                            .with_idle_output(true)
                            .with_idle_output_level(esp_hal::gpio::Level::High),
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
