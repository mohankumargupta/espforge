//! Direct handles for buses that no device drives.
//!
//! A bus with no driver is still useful at runtime: an I2C scanner sweeps a
//! controller, a UART example echoes lines, and an SPI example talks to a
//! generic slave. Those buses become fields on `Devices` under the bus id, so
//! the app uses `device!(ctx, i2c0)` and no intermediate element is needed.

use quote::{format_ident, quote};

use crate::bus_emit::{
    gpio, i2c_types, int, parity, peripheral_access, rate, runtime_type, spi_mode, spi_types,
    stop_bits,
};
use crate::element::EmitCx;
use crate::model::{Board, Bus, BusDevice, BusId, BusMode, Framing, Runtime};

/// Emit a raw handle for every bus that no device binds to.
pub fn emit_raw_buses(board: &Board, cx: &mut EmitCx) {
    let bound: Vec<BusId> = board
        .devices
        .iter()
        .filter_map(|device| device.as_bus().map(BusDevice::bus))
        .collect();
    for (index, bus) in board.resources.buses().iter().enumerate() {
        let slot = BusId(index);
        if bound.contains(&slot) {
            continue;
        }
        match bus {
            Bus::I2c {
                id,
                peripheral,
                sda,
                scl,
                khz,
            } => emit_i2c(
                cx,
                board,
                slot,
                id,
                peripheral,
                sda.pin().0,
                scl.pin().0,
                *khz,
            ),
            Bus::Spi {
                id,
                peripheral,
                sclk,
                mosi,
                miso,
                khz,
                mode,
                cs,
            } => {
                let Some(cs) = cs else {
                    continue;
                };
                emit_spi(
                    cx,
                    board,
                    slot,
                    id,
                    peripheral,
                    sclk.pin().0,
                    mosi.pin().0,
                    miso.as_ref().map(|pin| pin.pin().0),
                    *khz,
                    *mode,
                    cs.pin().0,
                );
            }
            Bus::Uart {
                id,
                peripheral,
                tx,
                rx,
                baud,
                framing,
            } => emit_uart(
                cx,
                board,
                id,
                peripheral,
                tx.pin().0,
                rx.pin().0,
                *baud,
                *framing,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_i2c(
    cx: &mut EmitCx,
    board: &Board,
    bus: BusId,
    id: &crate::model::Ident,
    peripheral: &str,
    sda: u8,
    scl: u8,
    khz: u32,
) {
    let mode = board.bus_mode(bus);
    let name = format_ident!("{}", id.rust_name());
    let handle = format_ident!("{}_bus", id.rust_name());
    let storage = format_ident!("__{}_STORAGE", id.rust_name().to_uppercase());
    let (storage_ty, bus_ty, _) = i2c_types(mode);
    let peripheral = peripheral_access(peripheral);
    let sda = gpio(sda);
    let scl = gpio(scl);
    let rate = rate(khz);
    let inserted = cx.device_once(
        &handle.to_string(),
        quote! {
            let #handle: #bus_ty = #bus_ty::build_in(
                &#storage,
                #peripheral,
                #sda,
                #scl,
                espforge_runtime::I2cConfig { frequency: #rate },
            )
            .expect("I2C bus configuration failed");
        },
    );
    if inserted {
        cx.module_item(quote! {
            static #storage: #storage_ty = #storage_ty::new();
        });
    }
    if board.meta.runtime == Runtime::Embassy {
        cx.device_field(quote! { pub #name: espforge_runtime::AsyncMutex<#bus_ty>, });
        cx.device_init(quote! { #name: espforge_runtime::AsyncMutex::new(#handle), });
    } else {
        cx.device_field(quote! { pub #name: #bus_ty, });
        cx.device_init(quote! { #name: #handle, });
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_spi(
    cx: &mut EmitCx,
    board: &Board,
    bus: BusId,
    id: &crate::model::Ident,
    peripheral: &str,
    sclk: u8,
    mosi: u8,
    miso: Option<u8>,
    khz: u32,
    mode: u8,
    cs: u8,
) {
    let name = format_ident!("{}", id.rust_name());
    let handle = format_ident!("{}_bus", id.rust_name());
    let storage = format_ident!("__{}_STORAGE", id.rust_name().to_uppercase());
    let bus_mode = board.bus_mode(bus);
    let (storage_ty, bus_ty, _) = spi_types(bus_mode);
    let peripheral = peripheral_access(peripheral);
    let sclk = gpio(sclk);
    let mosi = gpio(mosi);
    let rate = rate(khz);
    let mode_tokens = spi_mode(mode);
    let build = match miso {
        Some(miso) => {
            let miso = gpio(miso);
            quote! {
                let #handle: #bus_ty = #bus_ty::build_in(
                    &#storage,
                    #peripheral,
                    #sclk,
                    #mosi,
                    #miso,
                    espforge_runtime::SpiConfig { frequency: #rate, mode: #mode_tokens },
                )
                .expect("SPI bus configuration failed");
            }
        }
        None => quote! {
            let #handle: #bus_ty = #bus_ty::build_write_only_in(
                &#storage,
                #peripheral,
                #sclk,
                #mosi,
                espforge_runtime::SpiConfig { frequency: #rate, mode: #mode_tokens },
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
    let (device_ty, endpoint) = match bus_mode {
        BusMode::Blocking => (
            quote! { espforge_runtime::SpiDevice },
            quote! {
                espforge_runtime::SpiDevice::new(
                    #handle,
                    esp_hal::gpio::Output::new(
                        #cs,
                        esp_hal::gpio::Level::High,
                        esp_hal::gpio::OutputConfig::default(),
                    ),
                    espforge_runtime::Delay::new(),
                )
            },
        ),
        BusMode::Async => (
            quote! { espforge_runtime::AsyncSpiDevice },
            quote! {
                espforge_runtime::AsyncSpiDevice::new(
                    #handle,
                    esp_hal::gpio::Output::new(
                        #cs,
                        esp_hal::gpio::Level::High,
                        esp_hal::gpio::OutputConfig::default(),
                    ),
                    espforge_runtime::Delay::new(),
                )
            },
        ),
    };
    if board.meta.runtime == Runtime::Embassy {
        cx.device_field(quote! { pub #name: espforge_runtime::AsyncMutex<#device_ty>, });
        cx.device_init(quote! { #name: espforge_runtime::AsyncMutex::new(#endpoint), });
    } else {
        cx.device_field(quote! { pub #name: #device_ty, });
        cx.device_init(quote! { #name: #endpoint, });
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_uart(
    cx: &mut EmitCx,
    board: &Board,
    id: &crate::model::Ident,
    peripheral: &str,
    tx: u8,
    rx: u8,
    baud: u32,
    framing: Framing,
) {
    let name = format_ident!("{}", id.rust_name());
    let peripheral = peripheral_access(peripheral);
    let tx = gpio(tx);
    let rx = gpio(rx);
    let baud = int(baud);
    let parity = parity(framing.parity);
    let stop_bits = stop_bits(framing.stop_bits);
    let runtime = runtime_type(board.meta.runtime);
    let init = quote! {
        espforge_runtime::UartDevice::build(
            #peripheral,
            #tx,
            #rx,
            espforge_runtime::UartConfig {
                baudrate: #baud,
                data_bits: esp_hal::uart::DataBits::_8,
                parity: #parity,
                stop_bits: #stop_bits,
            },
        )
        .expect("UART configuration failed")
    };
    if board.meta.runtime == Runtime::Embassy {
        cx.device_field(quote! {
            pub #name: espforge_runtime::AsyncMutex<espforge_runtime::UartDevice<#runtime>>,
        });
        cx.device_init(quote! {
            #name: espforge_runtime::AsyncMutex::new(#init),
        });
    } else {
        cx.device_field(quote! {
            pub #name: espforge_runtime::UartDevice<#runtime>,
        });
        cx.device_init(quote! { #name: #init, });
    }
}
