//! Shared emission helpers for bus construction and endpoints.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use crate::element::EmitCx;
use crate::model::{
    Board, Bus, BusId, BusMode, Parity, RmtChannelDevice, RmtDirection, RmtLevel, Runtime, StopBits,
};

/// The esp-hal driver-mode type for the project's runtime.
///
/// Only a device that is inherently tied to the scheduler (a UART read that
/// awaits) uses this; a bus's mode comes from [`Board::bus_mode`].
pub(crate) fn runtime_type(runtime: Runtime) -> TokenStream {
    match runtime {
        Runtime::Blocking => quote! { esp_hal::Blocking },
        Runtime::Embassy => quote! { esp_hal::Async },
    }
}

/// `unsafe { registry.peripherals.GPIO<pin>.clone_unchecked() }`.
pub(crate) fn gpio(pin: u8) -> TokenStream {
    let gpio = format_ident!("GPIO{}", pin);
    quote! { unsafe { registry.peripherals.#gpio.clone_unchecked() } }
}

/// `unsafe { registry.peripherals.<name>.clone_unchecked() }`.
pub(crate) fn peripheral_access(name: &str) -> TokenStream {
    let peripheral = format_ident!("{}", name);
    quote! { unsafe { registry.peripherals.#peripheral.clone_unchecked() } }
}

/// An integer literal spelled as the templates spell it: no type suffix.
pub(crate) fn int(value: u32) -> Literal {
    Literal::u32_unsuffixed(value)
}

/// `esp_hal::time::Rate::from_khz(<khz>)`.
pub(crate) fn rate(khz: u32) -> TokenStream {
    let khz = int(khz);
    quote! { esp_hal::time::Rate::from_khz(#khz) }
}

pub(crate) fn parity(value: Parity) -> TokenStream {
    match value {
        Parity::None => quote! { esp_hal::uart::Parity::None },
        Parity::Even => quote! { esp_hal::uart::Parity::Even },
        Parity::Odd => quote! { esp_hal::uart::Parity::Odd },
    }
}

pub(crate) fn stop_bits(value: StopBits) -> TokenStream {
    match value {
        StopBits::One => quote! { esp_hal::uart::StopBits::_1 },
        StopBits::Two => quote! { esp_hal::uart::StopBits::_2 },
    }
}

pub(crate) fn spi_mode(value: u8) -> TokenStream {
    let mode = format_ident!("_{}", value);
    quote! { esp_hal::spi::Mode::#mode }
}

/// The bus types for one resolved mode: the `static` storage type, the bus
/// constructor, and the bus handle.
pub(crate) fn i2c_types(mode: BusMode) -> (TokenStream, TokenStream, TokenStream) {
    match mode {
        BusMode::Blocking => (
            quote! { espforge_runtime::I2cBusStorage },
            quote! { espforge_runtime::I2cBus },
            quote! { espforge_runtime::I2cBus },
        ),
        BusMode::Async => (
            quote! { espforge_runtime::AsyncI2cBusStorage },
            quote! { espforge_runtime::AsyncI2cBus },
            quote! { espforge_runtime::AsyncI2cBus },
        ),
    }
}

pub(crate) fn spi_types(mode: BusMode) -> (TokenStream, TokenStream, TokenStream) {
    match mode {
        BusMode::Blocking => (
            quote! { espforge_runtime::SpiBusStorage },
            quote! { espforge_runtime::SpiBus },
            quote! { espforge_runtime::SpiBus },
        ),
        BusMode::Async => (
            quote! { espforge_runtime::AsyncSpiBusStorage },
            quote! { espforge_runtime::AsyncSpiBus },
            quote! { espforge_runtime::AsyncSpiBus },
        ),
    }
}

/// Build the I2C controller once per bus inside `Devices::new`, and return the
/// device endpoint that rides it.
///
/// A blocking bus hands out an `embedded-hal-bus` `CriticalSectionDevice`: the
/// adapter owns the sharing, its critical section makes the handle `Send +
/// Sync`, and the driver receives its address as a constructor argument, so
/// nothing rewrites the address a driver passes.
pub(crate) fn i2c_device(cx: &mut EmitCx, board: &Board, bus: BusId) -> TokenStream {
    let Some(Bus::I2c {
        id,
        peripheral,
        sda,
        scl,
        khz,
    }) = board.resources.bus(bus)
    else {
        return quote! {};
    };

    let mode = board.bus_mode(bus);
    let handle = format_ident!("{}_bus", id.rust_name());
    let storage = format_ident!("__{}_STORAGE", id.rust_name().to_uppercase());
    let peripheral_tokens = peripheral_access(peripheral);
    let sda_tokens = gpio(sda.pin().0);
    let scl_tokens = gpio(scl.pin().0);
    let rate_tokens = rate(*khz);
    let (storage_ty, bus_ty, _) = i2c_types(mode);
    let key = handle.to_string();
    let inserted = cx.device_once(
        &key,
        quote! {
            let #handle: #bus_ty = #bus_ty::build_in(
                &#storage,
                #peripheral_tokens,
                #sda_tokens,
                #scl_tokens,
                espforge_runtime::I2cConfig { frequency: #rate_tokens },
            )
            .expect("I2C bus configuration failed");
        },
    );
    if inserted {
        cx.module_item(quote! {
            static #storage: #storage_ty = #storage_ty::new();
        });
    }

    match mode {
        BusMode::Blocking => quote! {
            espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice::new(#handle.cs_cell())
        },
        // No async driver exists yet (phase 5); a bus only resolves to `Async`
        // when the scheduler is embassy and nothing blocking binds to it.
        BusMode::Async => quote! { #handle },
    }
}

/// Build a UART port once, inside `Devices::new`, and return the endpoint the
/// device's constructor takes.
///
/// A `<uart>` that no device binds keeps its `UartDevice` field
/// (`raw_bus.rs`); a bound one is built here instead, because the device owns
/// the port. Lowering guarantees the port has at most one device, so this
/// builder runs once and nothing shares a `Uart`.
///
/// The port's driver mode follows the project runtime, exactly like the raw
/// field: `espforge-runtime` compiles one `UartDevice` flavour per build.
pub(crate) fn uart_device(cx: &mut EmitCx, board: &Board, bus: BusId) -> TokenStream {
    let Some(Bus::Uart {
        id,
        peripheral,
        tx,
        rx,
        baud,
        framing,
    }) = board.resources.bus(bus)
    else {
        return quote! {};
    };

    let handle = format_ident!("{}_bus", id.rust_name());
    let runtime = runtime_type(board.meta.runtime);
    let bus_ty = quote! { espforge_runtime::UartDevice<#runtime> };
    let peripheral_tokens = peripheral_access(peripheral);
    let tx_tokens = gpio(tx.pin().0);
    let rx_tokens = gpio(rx.pin().0);
    let baud = int(*baud);
    let parity = parity(framing.parity);
    let stop_bits = stop_bits(framing.stop_bits);
    cx.device_once(
        &handle.to_string(),
        quote! {
            let #handle: #bus_ty = espforge_runtime::UartDevice::build(
                #peripheral_tokens,
                #tx_tokens,
                #rx_tokens,
                espforge_runtime::UartConfig {
                    baudrate: #baud,
                    data_bits: esp_hal::uart::DataBits::_8,
                    parity: #parity,
                    stop_bits: #stop_bits,
                },
            )
            .expect("UART configuration failed");
        },
    );
    quote! { #handle }
}

/// Build the RMT block once per `<rmt>` and move one configured channel out
/// of it, wrapped in the runtime endpoint the element declares.
///
/// **Invariants.** `#handle` is a local inside `Devices::new`, never a
/// `Devices` field: each channel builder moves one `channelN` field out, and
/// nothing touches the whole `Rmt` afterwards. `device_once` runs before the
/// first move, so the `let` that creates the block precedes every channel.
/// Do not debug-print, borrow wholesale, or return `#handle`.
pub(crate) fn rmt_channel(
    cx: &mut EmitCx,
    board: &Board,
    device: &RmtChannelDevice,
) -> TokenStream {
    rmt_endpoint(cx, board, &device.transport)
}

/// Build an RMT endpoint for a `bus "rmt"` driver.
///
/// Shares [`rmt_endpoint`] with the raw channel path, so both move one channel
/// out of one block and neither can grow a second `Rmt::new`.
pub(crate) fn rmt_device(
    cx: &mut EmitCx,
    board: &Board,
    transport: &crate::model::RmtTransport,
) -> TokenStream {
    rmt_endpoint(cx, board, transport)
}

fn rmt_endpoint(
    cx: &mut EmitCx,
    board: &Board,
    transport: &crate::model::RmtTransport,
) -> TokenStream {
    let Some(Bus::Rmt {
        id: bus_id,
        peripheral,
        khz,
    }) = board.resources.bus(transport.bus)
    else {
        return quote! {};
    };

    let mode = board.bus_mode(transport.bus);
    let handle = format_ident!("{}_bus", bus_id.rust_name());
    let peripheral = peripheral_access(peripheral);
    let rate = rate(*khz);
    let clock = int(*khz);
    let into_async = match mode {
        BusMode::Blocking => quote! {},
        BusMode::Async => quote! { .into_async() },
    };
    cx.device_once(
        &handle.to_string(),
        quote! {
            let #handle = esp_hal::rmt::Rmt::new(#peripheral, #rate)
                .expect("RMT configuration failed") #into_async;
        },
    );

    let channel = format_ident!("channel{}", transport.channel);
    let pin = gpio(transport.pin.pin().0);
    let divider = Literal::u8_unsuffixed(transport.clk_divider);
    let direction = transport.direction;
    let config = match direction {
        RmtDirection::Tx => {
            let level = match transport.idle_level {
                RmtLevel::Low => quote! { esp_hal::gpio::Level::Low },
                RmtLevel::High => quote! { esp_hal::gpio::Level::High },
            };
            let mut config = quote! {
                espforge_runtime::TxChannelConfig::default()
                    .with_clk_divider(#divider)
                    .with_idle_output(true)
                    .with_idle_output_level(#level)
            };
            if let Some(carrier) = transport.carrier {
                let (high, low) = crate::rmt::carrier_ticks(
                    *khz,
                    transport.clk_divider,
                    carrier.hz,
                    carrier.duty,
                )
                .expect("lowering validated the carrier timing");
                let high = Literal::u16_unsuffixed(high);
                let low = Literal::u16_unsuffixed(low);
                config = quote! {
                    #config
                        .with_carrier_modulation(true)
                        .with_carrier_high(#high)
                        .with_carrier_low(#low)
                        .with_carrier_level(esp_hal::gpio::Level::Low)
                };
            }
            quote! {
                esp_hal::rmt::TxChannelCreator::configure_tx(#handle.#channel, &#config)
                    .expect("RMT TX channel configuration failed")
                    .with_pin(#pin)
            }
        }
        RmtDirection::Rx => quote! {
            esp_hal::rmt::RxChannelCreator::configure_rx(
                #handle.#channel,
                &espforge_runtime::RxChannelConfig::default().with_clk_divider(#divider),
            )
            .expect("RMT RX channel configuration failed")
            .with_pin(#pin)
        },
    };
    match direction {
        RmtDirection::Tx => quote! {
            espforge_runtime::RmtTxChannel::with_clock(#config, #clock, #divider)
        },
        RmtDirection::Rx => quote! {
            espforge_runtime::RmtRxChannel::new(#config)
        },
    }
}

/// Build the SPI bus once per bus inside `Devices::new`, and return the
/// endpoint with this device's own chip select, configuration and polarity.
pub(crate) fn spi_device(
    cx: &mut EmitCx,
    board: &Board,
    bus: BusId,
    cs: u8,
    config_khz: u32,
    config_mode: u8,
    polarity: crate::model::CsPolarity,
) -> TokenStream {
    let Some(Bus::Spi {
        id,
        peripheral,
        sclk,
        mosi,
        miso,
        khz,
        mode,
        ..
    }) = board.resources.bus(bus)
    else {
        return quote! {};
    };

    let bus_mode = board.bus_mode(bus);
    let handle = format_ident!("{}_bus", id.rust_name());
    let storage = format_ident!("__{}_STORAGE", id.rust_name().to_uppercase());
    let peripheral_tokens = peripheral_access(peripheral);
    let sclk_tokens = gpio(sclk.pin().0);
    let mosi_tokens = gpio(mosi.pin().0);
    let rate_tokens = rate(*khz);
    let mode_tokens = spi_mode(*mode);
    let (storage_ty, bus_ty, _) = spi_types(bus_mode);
    let build = match miso {
        Some(miso) => {
            let miso_tokens = gpio(miso.pin().0);
            quote! {
                let #handle: #bus_ty = #bus_ty::build_in(
                    &#storage,
                    #peripheral_tokens,
                    #sclk_tokens,
                    #mosi_tokens,
                    #miso_tokens,
                    espforge_runtime::SpiConfig {
                        frequency: #rate_tokens,
                        mode: #mode_tokens,
                    },
                )
                .expect("SPI bus configuration failed");
            }
        }
        None => quote! {
            let #handle: #bus_ty = #bus_ty::build_write_only_in(
                &#storage,
                #peripheral_tokens,
                #sclk_tokens,
                #mosi_tokens,
                espforge_runtime::SpiConfig {
                    frequency: #rate_tokens,
                    mode: #mode_tokens,
                },
            )
            .expect("SPI bus configuration failed");
        },
    };
    if cx.device_once(&handle.to_string(), build) {
        cx.module_item(quote! {
            static #storage: #storage_ty = #storage_ty::new();
        });
    }

    let cs = gpio(cs);
    let config_rate = rate(config_khz);
    let config_mode = spi_mode(config_mode);
    let (cs_idle, polarity) = match polarity {
        crate::model::CsPolarity::ActiveLow => (
            quote! { esp_hal::gpio::Level::High },
            quote! { espforge_runtime::CsPolarity::ActiveLow },
        ),
        crate::model::CsPolarity::ActiveHigh => (
            quote! { esp_hal::gpio::Level::Low },
            quote! { espforge_runtime::CsPolarity::ActiveHigh },
        ),
    };
    match bus_mode {
        BusMode::Blocking => quote! {
            espforge_runtime::ConfiguredSpiDevice::new(
                #handle,
                esp_hal::gpio::Output::new(
                    #cs,
                    #cs_idle,
                    esp_hal::gpio::OutputConfig::default(),
                ),
                espforge_runtime::Delay::new(),
                espforge_runtime::SpiConfig {
                    frequency: #config_rate,
                    mode: #config_mode,
                },
                #polarity,
            )
        },
        // The async endpoint applies the part's own mode and clock before
        // asserting CS, exactly like the blocking one.
        BusMode::Async => quote! {
            espforge_runtime::ConfiguredAsyncSpiDevice::new(
                #handle,
                esp_hal::gpio::Output::new(
                    #cs,
                    #cs_idle,
                    esp_hal::gpio::OutputConfig::default(),
                ),
                espforge_runtime::Delay::new(),
                espforge_runtime::SpiConfig {
                    frequency: #config_rate,
                    mode: #config_mode,
                },
                #polarity,
            )
        },
    }
}
