//! The Rust backend (Phase 3): `src/generated.rs`, `src/lib.rs`,
//! `src/bin/main.rs` and `build.rs`, emitted from `Board` with `quote!` and
//! printed by `prettyplease`.
//!
//! Comments are inserted by [`super::comments`] after printing; the AST cannot
//! carry them. The four files are owned by the project (regenerated on every
//! build).

use anyhow::{Context, Result, bail};
use espforge_model::{
    Artifact, Board, Bus, Component, Meta, Ownership, Parity, Runtime, Service, StopBits,
};
use espforge_options::ResolvedOptions;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::comments::{self, Comment};

/// Every Rust artifact, in a deterministic order.
pub fn generate(board: &Board, meta: &Meta, opts: &ResolvedOptions) -> Result<Vec<Artifact>> {
    Ok(vec![
        Artifact {
            path: "build.rs".into(),
            content: build_rs(meta)?,
            ownership: Ownership::Owned,
        },
        Artifact {
            path: "src/bin/main.rs".into(),
            content: main(board, meta, opts)?,
            ownership: Ownership::Owned,
        },
        Artifact {
            path: "src/generated.rs".into(),
            content: generated(board, meta)?,
            ownership: Ownership::Owned,
        },
        Artifact {
            path: "src/lib.rs".into(),
            content: lib(meta)?,
            ownership: Ownership::Owned,
        },
    ])
}

fn print(tokens: TokenStream) -> Result<String> {
    let file: syn::File = syn::parse2(tokens).context("generated Rust does not parse")?;
    Ok(prettyplease::unparse(&file))
}

fn gpio(pin: u8) -> TokenStream {
    let gpio = format_ident!("GPIO{}", pin);
    quote! { unsafe { registry.peripherals.#gpio.clone_unchecked() } }
}

fn peripheral_access(name: &str) -> TokenStream {
    let peripheral = format_ident!("{}", name);
    quote! { unsafe { registry.peripherals.#peripheral.clone_unchecked() } }
}

/// An integer literal spelled as the templates spell it: no type suffix.
fn int(value: u32) -> proc_macro2::Literal {
    proc_macro2::Literal::u32_unsuffixed(value)
}

fn rate(khz: u32) -> TokenStream {
    let khz = int(khz);
    quote! { esp_hal::time::Rate::from_khz(#khz) }
}

fn parity(value: Parity) -> TokenStream {
    match value {
        Parity::None => quote! { esp_hal::uart::Parity::None },
        Parity::Even => quote! { esp_hal::uart::Parity::Even },
        Parity::Odd => quote! { esp_hal::uart::Parity::Odd },
    }
}

fn stop_bits(value: StopBits) -> TokenStream {
    match value {
        StopBits::One => quote! { esp_hal::uart::StopBits::_1 },
        StopBits::Two => quote! { esp_hal::uart::StopBits::_2 },
    }
}

fn mode(value: u8) -> TokenStream {
    let mode = format_ident!("_{}", value);
    quote! { esp_hal::spi::Mode::#mode }
}

fn runtime_type(meta: &Meta) -> TokenStream {
    match meta.runtime {
        Runtime::Blocking => quote! { esp_hal::Blocking },
        Runtime::Embassy => quote! { esp_hal::Async },
    }
}

fn bus_of(board: &Board, id: espforge_model::BusId) -> Result<&Bus> {
    board.bus(id).context("board component names a missing bus")
}

fn generated(board: &Board, meta: &Meta) -> Result<String> {
    let runtime_type = runtime_type(meta);
    let mut fields = Vec::new();
    let mut inits = Vec::new();
    let mut bus_lets = Vec::new();

    for component in &board.components {
        match component {
            Component::Led(led) => {
                let name = format_ident!("{}", led.id.rust_name());
                let pin = gpio(led.pin.pin().0);
                let active_low = led.active_low;
                fields.push(quote! { pub #name: espforge_runtime::Led, });
                inits.push(quote! {
                    #name: espforge_runtime::Led::new(#pin, #active_low),
                });
            }
            Component::Button(button) => {
                let name = format_ident!("{}", button.id.rust_name());
                let pin = gpio(button.pin.pin().0);
                let pull_up = button.pull_up;
                fields.push(quote! { pub #name: espforge_runtime::Button, });
                inits.push(quote! {
                    #name: espforge_runtime::Button::new(#pin, #pull_up),
                });
            }
            Component::I2c(_) | Component::Spi(_) | Component::Uart(_) => {}
        }
    }

    // SPI buses are constructed once, before `Self`, because devices borrow
    // their handle. Only buses with at least one device are emitted.
    for (index, bus) in board.resources.buses().iter().enumerate() {
        let Bus::Spi {
            id,
            peripheral,
            sclk,
            mosi,
            miso,
            khz,
            mode: bus_mode,
        } = bus
        else {
            continue;
        };
        let used = board
            .components
            .iter()
            .any(|c| matches!(c, Component::Spi(spi) if spi.bus.0 == index));
        if !used {
            continue;
        }
        let handle = format_ident!("{}_bus", id.rust_name());
        let peripheral = peripheral_access(peripheral);
        let sclk = gpio(sclk.pin().0);
        let mosi = gpio(mosi.pin().0);
        let rate = rate(*khz);
        let spi_mode = mode(*bus_mode);
        bus_lets.push(match miso {
            Some(miso) => {
                let miso = gpio(miso.pin().0);
                quote! {
                    let #handle = espforge_runtime::SpiBus::build(
                        #peripheral, #sclk, #mosi, #miso,
                        espforge_runtime::SpiConfig {
                            frequency: #rate,
                            mode: #spi_mode,
                        },
                    )
                    .expect("SPI bus configuration failed");
                }
            }
            None => quote! {
                let #handle = espforge_runtime::SpiBus::build_write_only(
                    #peripheral, #sclk, #mosi,
                    espforge_runtime::SpiConfig {
                        frequency: #rate,
                        mode: #spi_mode,
                    },
                )
                .expect("SPI bus configuration failed");
            },
        });
    }

    for component in &board.components {
        match component {
            Component::I2c(i2c) => {
                let bus = bus_of(board, i2c.bus)?;
                let Bus::I2c {
                    peripheral,
                    sda,
                    scl,
                    khz,
                    ..
                } = bus
                else {
                    bail!("i2c component names a non-i2c bus");
                };
                let name = format_ident!("{}", i2c.id.rust_name());
                let peripheral = peripheral_access(peripheral);
                let sda = gpio(sda.pin().0);
                let scl = gpio(scl.pin().0);
                let rate = rate(*khz);
                fields.push(quote! {
                    pub #name: espforge_runtime::I2cBus<#runtime_type>,
                });
                inits.push(quote! {
                    #name: espforge_runtime::I2cBus::build(
                        #peripheral, #sda, #scl,
                        espforge_runtime::I2cConfig { frequency: #rate },
                    )
                    .expect("I2C bus configuration failed"),
                });
            }
            Component::Spi(spi) => {
                let bus = bus_of(board, spi.bus)?;
                let handle = format_ident!("{}_bus", bus.id().rust_name());
                let name = format_ident!("{}", spi.id.rust_name());
                let cs = gpio(spi.cs.pin().0);
                fields.push(quote! {
                    pub #name: espforge_runtime::SpiDevice<#runtime_type>,
                });
                inits.push(quote! {
                    #name: espforge_runtime::SpiDevice::new(
                        #handle,
                        esp_hal::gpio::Output::new(
                            #cs,
                            esp_hal::gpio::Level::High,
                            esp_hal::gpio::OutputConfig::default(),
                        ),
                        espforge_runtime::Delay::new(),
                    ),
                });
            }
            Component::Uart(uart) => {
                let bus = bus_of(board, uart.port)?;
                let Bus::Uart {
                    peripheral, tx, rx, ..
                } = bus
                else {
                    bail!("uart component names a non-uart bus");
                };
                let name = format_ident!("{}", uart.id.rust_name());
                let peripheral = peripheral_access(peripheral);
                let tx = gpio(tx.pin().0);
                let rx = gpio(rx.pin().0);
                let baud = int(uart.baud);
                let parity = parity(uart.framing.parity);
                let stop_bits = stop_bits(uart.framing.stop_bits);
                fields.push(quote! {
                    pub #name: espforge_runtime::UartDevice<#runtime_type>,
                });
                inits.push(quote! {
                    #name: espforge_runtime::UartDevice::build(
                        #peripheral, #tx, #rx,
                        espforge_runtime::UartConfig {
                            baudrate: #baud,
                            data_bits: esp_hal::uart::DataBits::_8,
                            parity: #parity,
                            stop_bits: #stop_bits,
                        },
                    )
                    .expect("UART configuration failed"),
                });
            }
            Component::Led(_) | Component::Button(_) => {}
        }
    }

    let wifi = wifi(board);
    let wifi_field = wifi
        .as_ref()
        .map(|_| quote! { pub wifi: espforge_runtime::Wifi, });
    let wifi_impl = wifi.as_ref().map(|wifi| {
        let ssid = &wifi.ssid;
        let password = &wifi.password;
        quote! {
            impl Services {
                #[doc = " Starts the Wi-Fi station: it returns right away and connects in the"]
                #[doc = " background. Call `wifi.wait_connected().await` to wait for an address."]
                pub fn new(
                    wifi: esp_hal::peripherals::WIFI<'static>,
                    spawner: &embassy_executor::Spawner,
                ) -> Result<Self, espforge_runtime::WifiError> {
                    let wifi = espforge_runtime::Wifi::start(
                        spawner,
                        wifi,
                        espforge_runtime::WifiConfig {
                            ssid: #ssid,
                            password: #password,
                        },
                    )?;
                    Ok(Self { wifi })
                }
            }
        }
    });

    let tokens = quote! {
        #![allow(dead_code, unused_variables)]

        #[doc = " Raw ESP32 peripherals, moved out of `esp_hal::peripherals::Peripherals` once at init."]
        pub struct PeripheralRegistry {
            pub peripherals: esp_hal::peripherals::Peripherals,
        }

        impl PeripheralRegistry {
            pub fn new(peripherals: esp_hal::peripherals::Peripherals) -> Self {
                Self { peripherals }
            }
        }

        #[doc = " Constructed components."]
        pub struct Components {
            #(#fields)*
        }

        impl Components {
            pub fn new(registry: &mut PeripheralRegistry) -> Self {
                #(#bus_lets)*
                Self {
                    #(#inits)*
                }
            }
        }

        #[doc = " Constructed devices (none yet)."]
        pub struct Devices {}

        #[doc = " Constructed services."]
        pub struct Services {
            #wifi_field
        }

        #wifi_impl
    };

    let mut text = comments::prepend(
        print(tokens)?,
        &["GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file."],
    );
    text = comments::apply(text, &safety_comments(board)?)?;
    // Re-parse: an inserted comment must never change what the file means.
    syn::parse_file(&text).context("commented generated.rs does not parse")?;
    Ok(text)
}

fn wifi<'a>(board: &'a Board) -> Option<&'a espforge_model::WifiService> {
    board.services.iter().find_map(|service| match service {
        Service::Wifi(wifi) => Some(wifi),
    })
}

/// The `SAFETY` notes the old template emitted around the unsafe moves.
fn safety_comments(board: &Board) -> Result<Vec<Comment>> {
    let mut comments = Vec::new();

    for (index, bus) in board.resources.buses().iter().enumerate() {
        let Bus::Spi {
            id,
            sclk,
            mosi,
            miso,
            ..
        } = bus
        else {
            continue;
        };
        let used = board
            .components
            .iter()
            .any(|c| matches!(c, Component::Spi(spi) if spi.bus.0 == index));
        if !used {
            continue;
        }
        let mut pins = format!("GPIO{}/GPIO{}", sclk.pin().0, mosi.pin().0);
        if let Some(miso) = miso {
            pins.push_str(&format!("/GPIO{}", miso.pin().0));
        }
        let anchor = format!("let {}_bus =", id.rust_name());
        comments.push(Comment::block(
            anchor.clone(),
            &[
                "SAFETY: espforge checked that no other component uses this",
                &format!("peripheral or {pins}."),
            ],
        ));
        if miso.is_none() {
            comments.push(Comment::new(
                anchor,
                "No miso: a write-only bus (displays).",
            ));
        }
    }

    for component in &board.components {
        match component {
            Component::Led(led) => {
                let name = led.id.rust_name();
                let pin = led.pin.pin().0;
                comments.push(Comment::new(
                    format!("{name}: espforge_runtime::Led::new("),
                    format!("SAFETY: espforge checked that no other component uses GPIO{pin}."),
                ));
            }
            Component::Button(button) => {
                let name = button.id.rust_name();
                let pin = button.pin.pin().0;
                comments.push(Comment::new(
                    format!("{name}: espforge_runtime::Button::new("),
                    format!("SAFETY: espforge checked that no other component uses GPIO{pin}."),
                ));
            }
            Component::I2c(i2c) => {
                let bus = bus_of(board, i2c.bus)?;
                let Bus::I2c { sda, scl, .. } = bus else {
                    bail!("i2c component names a non-i2c bus");
                };
                let name = i2c.id.rust_name();
                comments.push(Comment::block(
                    format!("{name}: espforge_runtime::I2cBus::build("),
                    &[
                        "SAFETY: espforge checked that no other component uses this",
                        &format!("peripheral or GPIO{}/GPIO{}.", sda.pin().0, scl.pin().0),
                    ],
                ));
            }
            Component::Spi(spi) => {
                let name = spi.id.rust_name();
                let cs = spi.cs.pin().0;
                comments.push(Comment::block(
                    format!("{name}: espforge_runtime::SpiDevice::new("),
                    &[
                        &format!("SAFETY: espforge checked that no other component uses GPIO{cs}."),
                        "Chip-select idles high (deasserted).",
                    ],
                ));
            }
            Component::Uart(uart) => {
                let bus = bus_of(board, uart.port)?;
                let Bus::Uart { tx, rx, .. } = bus else {
                    bail!("uart component names a non-uart bus");
                };
                let name = uart.id.rust_name();
                comments.push(Comment::block(
                    format!("{name}: espforge_runtime::UartDevice::build("),
                    &[
                        "SAFETY: espforge checked that no other component uses this",
                        &format!("peripheral or GPIO{}/GPIO{}.", tx.pin().0, rx.pin().0),
                    ],
                ));
            }
        }
    }
    Ok(comments)
}

fn lib(meta: &Meta) -> Result<String> {
    let signal = if meta.runtime == Runtime::Embassy {
        quote! {
            #[doc = " Define a cross-task signal. Usage: `crate::signal!(BUTTON_PRESSED)`"]
            #[macro_export]
            macro_rules! signal {
                ($name:ident) => {
                    static $name: embassy_sync::signal::Signal<
                        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
                        (),
                    > = embassy_sync::signal::Signal::new();
                };
            }
        }
    } else {
        quote! {}
    };

    let tokens = quote! {
        #![no_std]

        pub mod generated;

        pub use espforge_runtime::Delay;
        pub use espforge_runtime::embedded_hal;

        #[doc = " Access a component from the `Context`. Usage: `component!(ctx, my_led)`"]
        #[macro_export]
        macro_rules! component {
            ($ctx:expr, $name:ident) => {
                &$ctx.components.$name
            };
        }

        #[doc = " Access a device from the `Context`. Usage: `device!(ctx, my_screen)`"]
        #[macro_export]
        macro_rules! device {
            ($ctx:expr, $name:ident) => {
                &$ctx.devices.$name
            };
        }

        #[doc = " Access a service from the `Context`. Usage: `service!(ctx, wifi)`"]
        #[macro_export]
        macro_rules! service {
            ($ctx:expr, $name:ident) => {
                &$ctx.services.$name
            };
        }

        #signal

        pub mod app;

        pub use generated::{Components, Devices, PeripheralRegistry, Services};

        #[doc = " The whole context the app receives."]
        pub struct Context {
            pub logger: espforge_runtime::Logger,
            pub delay: espforge_runtime::Delay,
            pub components: Components,
            pub devices: Devices,
            pub services: Services,
        }

        #[doc = " Set by the entry point once wiring is complete."]
        pub static CTX: static_cell::StaticCell<Context> = static_cell::StaticCell::new();
    };
    let mut text = comments::prepend(
        print(tokens)?,
        &["GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file."],
    );
    text = comments::apply(
        text,
        &[Comment::new(
            "pub use espforge_runtime::Delay;",
            "Re-exports for `app.rs` (it only uses `crate::` paths).",
        )],
    )?;
    Ok(text)
}

fn main(board: &Board, meta: &Meta, opts: &ResolvedOptions) -> Result<String> {
    let crate_name = format_ident!("{}", meta.crate_name());
    let wifi = wifi(board).is_some();
    let heap = opts.is_on("wifi") || opts.is_on("ble-trouble");
    let services = if wifi {
        quote! {
            let services = Services::new(registry.peripherals.WIFI, &spawner)
                .expect("failed to start Wi-Fi");
        }
    } else {
        quote! { let services = Services {}; }
    };

    let tokens = match meta.runtime {
        Runtime::Blocking => quote! {
            #![no_std]
            #![no_main]

            use esp_backtrace as _;
            use #crate_name::{app, Components, Context, Devices, PeripheralRegistry, CTX};
            use #crate_name::Services;

            #[esp_hal::main]
            fn main() -> ! {
                esp_println::logger::init_logger_from_env();
                esp_println::print!("\x1b[20h");
                esp_bootloader_esp_idf::esp_app_desc!();
                let peripherals = esp_hal::init(esp_hal::Config::default());
                #[allow(unused_mut)]
                let mut registry = PeripheralRegistry::new(peripherals);

                let logger = espforge_runtime::Logger::new();
                let delay = espforge_runtime::Delay::new();

                let components = Components::new(&mut registry);
                let devices = Devices {};
                let services = Services {};

                let ctx = Context { logger, delay, components, devices, services };
                let ctx = CTX.init(ctx);
                app::setup(ctx);
                loop {
                    app::forever(ctx);
                }
            }
        },
        Runtime::Embassy => {
            let heap = if heap {
                quote! { esp_alloc::heap_allocator!(size: 72 * 1024); }
            } else {
                quote! {}
            };
            quote! {
                #![no_std]
                #![no_main]

                use embassy_executor::Spawner;
                use esp_backtrace as _;
                use esp_hal::timer::timg::TimerGroup;
                use #crate_name::{app, Components, Context, Devices, PeripheralRegistry, CTX};
                use #crate_name::Services;

                #[esp_rtos::main]
                async fn main(spawner: Spawner) {
                    esp_println::logger::init_logger_from_env();
                    esp_println::print!("\x1b[20h");
                    esp_bootloader_esp_idf::esp_app_desc!();
                    let peripherals = esp_hal::init(esp_hal::Config::default());
                    #heap
                    #[allow(unused_mut)]
                    let mut registry = PeripheralRegistry::new(peripherals);

                    let components = Components::new(&mut registry);
                    let devices = Devices {};

                    let timg0 = TimerGroup::new(registry.peripherals.TIMG0);
                    esp_rtos::start(timg0.timer0, registry.peripherals.FROM_CPU_INTR0);

                    let logger = espforge_runtime::Logger::new();
                    let delay = espforge_runtime::Delay::new();

                    #services

                    let ctx = Context { logger, delay, components, devices, services };
                    let ctx = CTX.init(ctx);
                    app::setup(ctx, spawner).await;
                    loop {
                        app::forever(ctx).await;
                    }
                }
            }
        }
    };

    let mut text = comments::prepend(
        print(tokens)?,
        &[
            "GENERATED BY ESPFORGE — DO NOT EDIT.",
            "Source of truth is the project .espf file.",
        ],
    );
    let mut comments = vec![Comment::new(
        "let components = Components::new(",
        "Built first: it borrows the whole registry, which the moves below break.",
    )];
    if meta.runtime == Runtime::Embassy {
        comments.push(Comment::block(
            "let logger = espforge_runtime::Logger::new();",
            &[
                "Hoisted above the component/device wiring because some drivers take",
                "`&ctx.delay` at construct time.",
            ],
        ));
        comments.push(Comment::new(
            "let services =",
            "Services need the scheduler (started above) and the spawner.",
        ));
    }
    text = comments::apply(text, &comments)?;
    Ok(text)
}

fn build_rs(meta: &Meta) -> Result<String> {
    let chip = esp_metadata_generated::Chip::iter()
        .find(|chip| chip.name() == meta.chip)
        .with_context(|| format!("unknown chip `{}`", meta.chip))?;
    let xtensa = chip.is_xtensa();
    let xtensa_check = if xtensa {
        quote! { check_xtensa_linker_available(); }
    } else {
        quote! {}
    };
    let xtensa_funcs = if xtensa {
        quote! {
            #[cfg(unix)]
            fn check_xtensa_linker_available() {
                println!("cargo:rerun-if-env-changed=PATH");

                let target = std::env::var("TARGET").unwrap_or_default();
                println!(
                    "cargo:rerun-if-env-changed={}",
                    cargo_linker_env_var(&target)
                );

                let linker = xtensa_linker(&target);

                let error = match std::process::Command::new(&linker)
                    .arg("--version")
                    .output()
                {
                    Ok(_) => return,
                    Err(error) => error,
                };

                let export_file = std::env::var("HOME")
                    .map(|home| format!("{home}/export-esp.sh"))
                    .unwrap_or_else(|_| "$HOME/export-esp.sh".to_string());

                panic!(
                    "Xtensa linker `{linker}` was not found in PATH or could not be executed: {error}.\n\n\
                     Xtensa targets on Unix require espup's environment export file to be sourced before building.\n\
                     Try: `source {export_file}`\n\n\
                     If the export file does not exist, run `espup install` first.\n\
                     For more details, see:\n\
                     https://github.com/esp-rs/espup?tab=readme-ov-file#environment-variables-setup"
                );
            }

            #[cfg(not(unix))]
            fn check_xtensa_linker_available() {}

            #[cfg(unix)]
            fn xtensa_linker(target: &str) -> String {
                if let Some(linker) = cargo_configured_linker(target) {
                    return linker;
                }

                target
                    .strip_prefix("xtensa-")
                    .and_then(|target| target.strip_suffix("-none-elf"))
                    .map(|chip| format!("xtensa-{chip}-elf-gcc"))
                    .unwrap_or_else(|| "xtensa-esp32-elf-gcc".to_string())
            }

            #[cfg(unix)]
            fn cargo_configured_linker(target: &str) -> Option<String> {
                std::env::var(cargo_linker_env_var(target)).ok()
            }

            #[cfg(unix)]
            fn cargo_linker_env_var(target: &str) -> String {
                let target = target.replace('-', "_").to_ascii_uppercase();
                format!("CARGO_TARGET_{target}_LINKER")
            }
        }
    } else {
        quote! {}
    };
    let arch_arg = if xtensa {
        quote! {
            println!(
                "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
                std::env::current_exe().unwrap().display()
            );
        }
    } else {
        quote! {
            println!(
                "cargo:rustc-link-arg=--error-handling-script={}",
                std::env::current_exe().unwrap().display()
            );
        }
    };

    let tokens = quote! {
        fn main() {
            linker_be_nice();
            #xtensa_check
            // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
            println!("cargo:rustc-link-arg=-Tlinkall.x");
        }

        #xtensa_funcs

        fn linker_be_nice() {
            let args: Vec<String> = std::env::args().collect();
            if args.len() > 1 {
                let kind = &args[1];
                let what = &args[2];

                match kind.as_str() {
                    "undefined-symbol" => match what.as_str() {
                        "_stack_start" => {
                            eprintln!();
                            eprintln!("💡 Is the linker script `linkall.x` missing?");
                            eprintln!();
                        }
                        what if what.starts_with("esp_rtos_") => {
                            eprintln!();
                            eprintln!(
                                "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                            );
                            eprintln!();
                        }
                        "free"
                        | "malloc"
                        | "calloc"
                        | "get_free_internal_heap_size"
                        | "malloc_internal"
                        | "realloc_internal"
                        | "calloc_internal"
                        | "free_internal" => {
                            eprintln!();
                            eprintln!(
                                "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                            );
                            eprintln!();
                        }
                        _ => (),
                    },
                    _ => {
                        std::process::exit(1);
                    }
                }

                std::process::exit(0);
            }

            #arch_arg
        }
    };
    let mut text = comments::prepend(
        print(tokens)?,
        &["GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file."],
    );
    text = comments::apply(
        text,
        &[Comment::new(
            "println!(\"cargo:rustc-link-arg=-Tlinkall.x\");",
            "make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)",
        )],
    )?;
    Ok(text)
}
