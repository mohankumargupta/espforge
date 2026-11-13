//! The Rust backend: `src/generated.rs`, `src/lib.rs`, `src/bin/main.rs` and
//! `build.rs`, emitted from `Board` with `quote!` and printed by
//! `prettyplease`.
//!
//! The four files are owned by the project (regenerated on every build).
//!
//! `generated()` walks `board.devices` and `board.services` and calls each
//! one's `Lowered::emit` through an `EmitCx`; it has no arm per element. With
//! nothing declared it emits exactly the empty `Devices {}` and `Services {}`
//! of the helloworld golden.

use anyhow::{Context, Result};
use espforge_elements::element::EmitParts;
use espforge_elements::{
    Board, Device, EmitCx, LogLevel, Lowered, MainCtx, Meta, ResolvedOptions, Runtime, Service,
};
use proc_macro2::{Literal, TokenStream};
use quote::format_ident;

use crate::artifact::Artifact;

/// Every Rust artifact, in a deterministic order.
///
/// The elements are asked to emit **once**, here: the same `EmitParts` feed
/// `generated.rs` (fields, initialisers, prelude) and `main.rs`
/// (startup statements). An element cannot be asked to emit twice.
pub fn generate(board: &Board, meta: &Meta, opts: &ResolvedOptions) -> Result<Vec<Artifact>> {
    let parts = collect(board);
    Ok(vec![
        Artifact::new("build.rs", build_rs(meta)?),
        Artifact::new("src/bin/main.rs", main(board, &parts, meta, opts)?),
        Artifact::new("src/generated.rs", generated(&parts, meta)?),
        Artifact::new("src/lib.rs", lib(meta, !parts.device_fields.is_empty())?),
    ])
}

fn print(tokens: TokenStream) -> Result<String> {
    let file: syn::File = syn::parse2(tokens).context("generated Rust does not parse")?;
    Ok(prettyplease::unparse(&file))
}

fn generated(parts: &EmitParts, meta: &Meta) -> Result<String> {
    // The elements choose the container at their own emission site: a blocking
    // board keeps the bare device field, an embassy board wraps every device
    // in `AsyncMutex`. There is no second wrapping pass here.
    let device_fields = &parts.device_fields;
    let device_inits = &parts.device_inits;
    let service_fields = &parts.service_fields;
    let service_inits = &parts.service_inits;
    let service_prelude = &parts.service_prelude;
    let module_items = &parts.module_items;
    let device_prelude = &parts.device_prelude;

    // `Devices` is byte-identical to the styles that predate devices (the
    // helloworld `Devices {}` with no `new`) when nothing is declared.
    let devices = if device_fields.is_empty() {
        quote::quote! {
            #[doc = " Constructed devices (none yet)."]
            pub struct Devices {}
        }
    } else {
        quote::quote! {
            #[doc = " Constructed devices."]
            pub struct Devices {
                #(#device_fields)*
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
                    #(#device_prelude)*
                    Ok(Self {
                        #(#device_inits)*
                    })
                }
            }
        }
    };

    // `Services` is byte-identical to the styles that predate services (the
    // helloworld `Services {}` with no `new`) when nothing is declared, so
    // adding a service is a change to projects that have one, and no change to
    // the projects that do not.
    let services = if service_fields.is_empty() {
        quote::quote! {
            #[doc = " Constructed services."]
            pub struct Services {}
        }
    } else {
        quote::quote! {
            #[doc = " Constructed services."]
            pub struct Services {
                #(#service_fields)*
            }

            impl Services {
                pub fn new() -> Self {
                    #(#service_prelude)*
                    Self {
                        #(#service_inits)*
                    }
                }
            }
        }
    };

    // The OTA updater's storage: one cell per project, declared here rather
    // than inside `espforge-runtime` (which never hides a `static`), and
    // filled by `main()` from the `FLASH` peripheral.
    let ota_storage = if meta.ota.is_some() {
        quote::quote! {
            #[doc = " The OTA updater's storage, filled once by `main()`."]
            pub static __OTA_STORAGE: espforge_runtime::OtaStorage =
                espforge_runtime::OtaStorage::new();
        }
    } else {
        quote::quote! {}
    };

    // `<logger>` installs the runtime's target filter. The rule table and the
    // logger itself are statics declared here, not in the runtime, which never
    // hides storage in a `static`. A project without `<logger>` emits neither
    // and keeps esp-println's `ESP_LOG` logger; a level="none" project strips
    // logging at compile time and installs nothing.
    let logger = match meta.logger {
        Some(config) if !config.silenced() => {
            let default = level_filter(config.level);
            let rules: Vec<TokenStream> = config
                .filters()
                .into_iter()
                .map(|(prefix, level)| {
                    let level = level_filter(level);
                    quote::quote! {
                        espforge_runtime::FilterRule {
                            prefix: #prefix,
                            level: log::LevelFilter::#level,
                        }
                    }
                })
                .collect();
            quote::quote! {
                #[doc = " The target filters `<logger>` declared: a module prefix and its level."]
                pub static LOG_FILTERS: &[espforge_runtime::FilterRule] = &[#(#rules),*];

                #[doc = " The logger `main()` installs before any other code runs."]
                pub static LOGGER: espforge_runtime::FilteredLogger =
                    espforge_runtime::FilteredLogger::new(
                        log::LevelFilter::#default,
                        LOG_FILTERS,
                    );
            }
        }
        _ => quote::quote! {},
    };

    let tokens = quote::quote! {
        #![allow(dead_code, unused_variables)]

        // Generated storage: one `static` per bus/station/client, declared by
        // the element that owns it.
        #(#module_items)*

        #ota_storage

        #logger

        #[doc = " Raw ESP32 peripherals, moved out of `esp_hal::peripherals::Peripherals` once at init."]
        pub struct PeripheralRegistry {
            pub peripherals: esp_hal::peripherals::Peripherals,
        }

        impl PeripheralRegistry {
            pub fn new(peripherals: esp_hal::peripherals::Peripherals) -> Self {
                Self { peripherals }
            }
        }

        #devices

        #services
    };

    print(tokens)
}

/// Fill the `Devices` and `Services` fields and initialisers, and collect the
/// `main()` startup statements, by asking every pin device, raw bus, bus
/// device and service to emit itself. An element's own file owns every token
/// it prints.
///
/// A station's provider local must exist before a client names it, and its
/// storage static has always preceded the bus statics, so the station pass
/// stays first. Pin devices and raw buses follow, then bus devices, then the
/// protocol services that consume the station. That keeps the generated text
/// stable while removing the old non-device pass.
fn collect(board: &Board) -> EmitParts {
    let mut cx = EmitCx::with_main(MainCtx::default());
    for service in &board.services {
        if matches!(service, Service::WifiStation(_)) {
            service.emit(board, &mut cx);
        }
    }
    for device in &board.devices {
        if let Device::Pin(device) = device {
            device.emit(board, &mut cx);
        }
    }
    espforge_elements::raw_bus::emit_raw_buses(board, &mut cx);
    for device in &board.devices {
        if let Device::Bus(device) = device {
            device.emit(board, &mut cx);
        }
    }
    for service in &board.services {
        if matches!(service, Service::Protocol(_)) {
            service.emit(board, &mut cx);
        }
    }
    cx.into_parts()
}

fn lib(meta: &Meta, has_devices: bool) -> Result<String> {
    // One universal accessor: the container decides the spelling. A blocking
    // board hands out `&mut`; an embassy board awaits the device's
    // `AsyncMutex`. A board with no devices keeps the shared-reference
    // spelling it has always had (the helloworld golden depends on it byte
    // for byte).
    let device_macro = match (has_devices, meta.runtime) {
        (true, Runtime::Embassy) => quote::quote! {
            #[doc = " Access a device from the `Context`: waits for its `AsyncMutex` and yields a guard. Usage: `let mut dev = device!(ctx, my_device);`"]
            #[macro_export]
            macro_rules! device {
                ($ctx:expr, $name:ident) => {
                    $ctx.devices.$name.lock().await
                };
            }

            #[doc = " Backward compatibility alias for `device!`"]
            #[macro_export]
            macro_rules! async_device {
                ($ctx:expr, $name:ident) => {
                    $crate::device!($ctx, $name)
                };
            }
        },
        (true, Runtime::Blocking) => quote::quote! {
            #[doc = " Access a device from the `Context`. Usage: `device!(ctx, my_screen)`"]
            #[macro_export]
            macro_rules! device {
                ($ctx:expr, $name:ident) => {
                    &mut $ctx.devices.$name
                };
            }
        },
        (false, _) => quote::quote! {
            #[doc = " Access a device from the `Context`. Usage: `device!(ctx, my_screen)`"]
            #[macro_export]
            macro_rules! device {
                ($ctx:expr, $name:ident) => {
                    &$ctx.devices.$name
                };
            }
        },
    };
    // `<ota />` puts the updater handle on the context: `ctx.ota`.
    let ota_field = if meta.ota.is_some() {
        quote::quote! {
            #[doc = " Dual-slot A/B firmware updates (`<ota />`)."]
            pub ota: espforge_runtime::Ota,
        }
    } else {
        quote::quote! {}
    };
    let signal = if meta.runtime == Runtime::Embassy {
        quote::quote! {
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
        quote::quote! {}
    };

    let tokens = quote::quote! {
        #![no_std]

        pub mod generated;

        pub use espforge_runtime::Delay;
        pub use espforge_runtime::embedded_hal;

        #device_macro

        #[doc = " Backward compatibility alias for `device!`"]
        #[macro_export]
        macro_rules! component {
            ($ctx:expr, $name:ident) => {
                $crate::device!($ctx, $name)
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

        pub use generated::{Devices, PeripheralRegistry, Services};

        #[doc = " The whole context the app receives."]
        pub struct Context {
            pub logger: espforge_runtime::Logger,
            pub delay: espforge_runtime::Delay,
            pub devices: Devices,
            pub services: Services,
            #ota_field
        }

        #[doc = " Set by the entry point once wiring is complete."]
        pub static CTX: static_cell::StaticCell<Context> = static_cell::StaticCell::new();
    };
    print(tokens)
}

/// The heap line `main()` must install, if any.
///
/// The radio is the only thing that asks for one: `esp-radio` allocates, and
/// without a heap the WiFi tasks fail to start. A service that states
/// `heap_bytes` (TLS keeps `embedded-tls`'s crypto scratch and record buffers)
/// asks for its own, and the largest one wins. The number is written
/// unsuffixed, as whole KiB when it is one, because that is the form the
/// goldens carry. The caller leaves the line out entirely when nothing wants a
/// heap (a blocking project cannot have a radio: `wifi` requires `embassy`).
fn heap_line(board: &Board, opts: &ResolvedOptions) -> TokenStream {
    if let Some(heap_size) = board.max_service_heap() {
        if heap_size % 1024 == 0 {
            let kib = Literal::u32_unsuffixed(heap_size / 1024);
            quote::quote! { esp_alloc::heap_allocator!(size: #kib * 1024); }
        } else {
            let size = Literal::u32_unsuffixed(heap_size);
            quote::quote! { esp_alloc::heap_allocator!(size: #size); }
        }
    } else if opts.is_on("wifi") {
        quote::quote! { esp_alloc::heap_allocator!(size: 72 * 1024); }
    } else {
        quote::quote! {}
    }
}

/// The `log::LevelFilter` variant for a `<logger>` level.
fn level_filter(level: LogLevel) -> proc_macro2::Ident {
    format_ident!("{}", level.variant())
}

fn main(board: &Board, parts: &EmitParts, meta: &Meta, opts: &ResolvedOptions) -> Result<String> {
    let crate_name = format_ident!("{}", meta.crate_name());
    let heap = heap_line(board, opts);
    let startup = &parts.startup;

    // How the entry point installs the `log` sink: the generated filtered
    // logger, nothing at all when `<logger level="none" />` stripped logging,
    // or esp-println's env logger for a project that did not declare `<logger>`.
    let logger_init = match meta.logger {
        Some(config) if config.silenced() => quote::quote! {
            #[doc = " logger disabled: `<logger level=\"none\" />` strips every log call at compile time."]
            const _: () = ();
        },
        Some(_) => quote::quote! {
            espforge_runtime::logger::init(&#crate_name::generated::LOGGER);
        },
        None => quote::quote! {
            esp_println::logger::init_logger_from_env();
        },
    };

    // Build the updater before the context is packed: it owns the `FLASH`
    // peripheral, and a table that cannot be read is an error at boot, not a
    // surprise when the first update arrives.
    let (ota_import, ota_init, ota_field) = match meta.ota {
        Some(ota) => {
            let rollback = ota.rollback;
            (
                quote::quote! { use #crate_name::generated::__OTA_STORAGE; },
                quote::quote! {
                    let ota = match espforge_runtime::Ota::new_in(
                        &__OTA_STORAGE,
                        registry.peripherals.FLASH,
                        #rollback,
                    ) {
                        Ok(ota) => ota,
                        Err(error) => {
                            logger.error(format_args!("{}", error));
                            loop {
                                esp_hal::delay::Delay::new().delay_millis(1_000);
                            }
                        }
                    };
                },
                quote::quote! { ota, },
            )
        }
        None => (quote::quote! {}, quote::quote! {}, quote::quote! {}),
    };

    // The empty `Devices {}` keeps the helloworld golden byte-identical; a
    // declared device gets the constructor, which builds its buses itself.
    let devices = if parts.device_fields.is_empty() {
        quote::quote! { let devices = Devices {}; }
    } else {
        quote::quote! {
            let devices = match Devices::new(&mut registry) {
                Ok(devices) => devices,
                Err(error) => {
                    logger.error(format_args!("{}", error));
                    loop {
                        esp_hal::delay::Delay::new().delay_millis(1_000);
                    }
                }
            };
        }
    };

    // `Services {}` without a `new` when nothing is declared keeps the
    // helloworld golden byte-identical; a declared service gets the
    // `Services::new()` constructor instead.
    let services = if parts.service_fields.is_empty() {
        quote::quote! { let services = Services {}; }
    } else {
        quote::quote! { let services = Services::new(); }
    };

    let tokens = match meta.runtime {
        Runtime::Blocking => quote::quote! {
            #![no_std]
            #![no_main]

            use esp_backtrace as _;
            use #crate_name::{app, Context, Devices, PeripheralRegistry, CTX};
            use #crate_name::Services;
            #ota_import

            #[esp_hal::main]
            fn main() -> ! {
                #logger_init
                esp_println::print!("\x1b[20h");
                esp_bootloader_esp_idf::esp_app_desc!();
                let peripherals = esp_hal::init(esp_hal::Config::default());
                #[allow(unused_mut)]
                let mut registry = PeripheralRegistry::new(peripherals);

                let logger = espforge_runtime::Logger::new();
                let delay = espforge_runtime::Delay::new();

                #devices
                #services
                #ota_init

                let ctx = Context { logger, delay, devices, services, #ota_field };
                let ctx = CTX.init(ctx);
                app::setup(ctx);
                loop {
                    app::forever(ctx);
                }
            }
        },
        Runtime::Embassy => quote::quote! {
            #![no_std]
            #![no_main]

            use embassy_executor::Spawner;
            use esp_backtrace as _;
            use esp_hal::timer::timg::TimerGroup;
            use #crate_name::{app, Context, Devices, PeripheralRegistry, CTX};
            use #crate_name::Services;
            #ota_import

            #[esp_rtos::main]
            async fn main(spawner: Spawner) {
                #logger_init
                esp_println::print!("\x1b[20h");
                esp_bootloader_esp_idf::esp_app_desc!();
                let peripherals = esp_hal::init(esp_hal::Config::default());
                #heap
                #[allow(unused_mut)]
                let mut registry = PeripheralRegistry::new(peripherals);

                let logger = espforge_runtime::Logger::new();
                let delay = espforge_runtime::Delay::new();

                #devices

                let timg0 = TimerGroup::new(registry.peripherals.TIMG0);
                esp_rtos::start(timg0.timer0, registry.peripherals.FROM_CPU_INTR0);

                #services

                #(#startup)*
                #ota_init

                let ctx = Context { logger, delay, devices, services, #ota_field };
                let ctx = CTX.init(ctx);
                app::setup(ctx, spawner).await;
                loop {
                    app::forever(ctx).await;
                }
            }
        },
    };

    print(tokens)
}

fn build_rs(meta: &Meta) -> Result<String> {
    let chip = esp_metadata_generated::Chip::iter()
        .find(|chip| chip.name() == meta.chip)
        .with_context(|| format!("unknown chip `{}`", meta.chip))?;
    let xtensa = chip.is_xtensa();
    let xtensa_check = if xtensa {
        quote::quote! { check_xtensa_linker_available(); }
    } else {
        quote::quote! {}
    };
    let xtensa_funcs = if xtensa {
        quote::quote! {
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
        quote::quote! {}
    };
    let arch_arg = if xtensa {
        quote::quote! {
            println!(
                "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
                std::env::current_exe().unwrap().display()
            );
        }
    } else {
        quote::quote! {
            println!(
                "cargo:rustc-link-arg=--error-handling-script={}",
                std::env::current_exe().unwrap().display()
            );
        }
    };

    let tokens = quote::quote! {
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
    print(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::{case, device, service};
    use espforge_elements::{
        BusDevice, BusTransport, Device, Driver, I2cTransport, Ident, Owner, OwnerKind, Pin,
        PinDevice, PinUse, ResourceGraph, Service, SpiTransport,
    };

    /// Every name the generated `lib.rs`/`main.rs` binds must be reserved for
    /// the user: a local added without reserving its name is how a user id
    /// starts producing a `rustc` error in generated code.
    #[test]
    fn every_name_the_generated_code_binds_is_reserved() {
        use espforge_elements::MainCtx;
        use espforge_elements::element::RESERVED_IDENTS;

        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);

        for name in MainCtx::default().names() {
            assert!(
                RESERVED_IDENTS.contains(&name.as_str()),
                "`main()` binds `{name}`, which is not in RESERVED_IDENTS"
            );
        }

        let text = lib(&case.meta, true).unwrap();
        let context = text
            .split("pub struct Context {")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .expect("`lib()` prints a `Context` struct");
        let fields: Vec<&str> = context
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub "))
            .filter_map(|field| field.split(':').next())
            .map(str::trim)
            .collect();
        assert_eq!(fields.len(), 4, "`Context` field scan found {fields:?}");
        for field in fields {
            assert!(
                RESERVED_IDENTS.contains(&field),
                "`Context` has a field `{field}` that is not in RESERVED_IDENTS"
            );
        }
    }

    /// WS-A's grep guard: no runtime singleton may hide its storage in a
    /// `static`. Each one takes a `&'static` storage value that the generated
    /// code declares per element, so two instances cannot share one cell.
    #[test]
    fn no_static_storage_in_runtime() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("the runtime sources are readable") {
                let path = entry.expect("a readable directory entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.to_string_lossy().ends_with(".rs") {
                    out.push(path);
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the emit crate lives beside espforge-runtime")
            .join("espforge-runtime/src");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(
            !files.is_empty(),
            "no runtime sources under {}",
            root.display()
        );

        let mut offenders = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(file).expect("a runtime source is UTF-8");
            // A `#[cfg(test)]` module may hold a static; the code shipped to the
            // chip may not.
            let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (number, line) in shipped.lines().enumerate() {
                let line = line.trim_start();
                if ["static ", "pub static ", "static mut "]
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
                {
                    offenders.push(format!("{}:{}: {line}", file.display(), number + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "runtime storage must be a parameter, not a `static`:\n{}",
            offenders.join("\n")
        );

        // The same on the generated side: a board without a bus or a service
        // needs no storage at all, so it emits no `static`.
        let (board, meta) = blink();
        let text = generated(&collect(&board), &meta).unwrap();
        assert!(!text.contains("static "), "{text}");
    }

    /// `blink`: one LED on GPIO18.
    fn blink() -> (Board, Meta) {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = Board {
            meta: case.meta.clone(),
            resources: ResourceGraph::default(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        };
        let id = Ident::new("red_led", 0..0);
        let pin = board
            .resources
            .claim(
                Pin(18),
                Owner::new(OwnerKind::Pin("led".into()), id.clone()),
                0..0,
                PinUse::Output,
            )
            .expect("pin 18 is free");
        board
            .devices
            .push(Device::Pin(PinDevice::new("led", id, pin)));
        (board, case.meta)
    }

    #[test]
    fn a_led_emits_its_field_and_init() {
        let (board, meta) = blink();
        let text = generated(&collect(&board), &meta).unwrap();
        assert!(
            text.contains("pub red_led: espforge_runtime::Led,"),
            "{text}"
        );
        assert!(
            text.contains("unsafe { registry.peripherals.GPIO18.clone_unchecked() }"),
            "{text}"
        );
        assert!(text.contains("false,"), "{text}");
    }

    /// A service that owns mutable state gets one module-scope `static`, and
    /// its constructor takes it first: two clients never share one buffer.
    #[test]
    fn a_service_with_storage_owns_one_static() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &["wifi"]);
        let mut board = empty_board(&case);
        let Service::Protocol(mut client) = service("http_client", "http-client", None) else {
            panic!("the fixture is a protocol service");
        };
        client.new =
            espforge_elements::model::RustPath::parse("espforge_runtime::HttpClient::new_in")
                .unwrap();
        client.storage = Some(
            espforge_elements::model::RustType::parse("espforge_runtime::HttpClientStorage")
                .unwrap(),
        );
        board.services.push(Service::Protocol(client));

        let text = generated(&collect(&board), &case.meta).unwrap();
        assert!(
            text.contains("static __HTTP_CLIENT_STORAGE: espforge_runtime::HttpClientStorage"),
            "{text}"
        );
        assert_eq!(
            text.matches("__HTTP_CLIENT_STORAGE").count(),
            2,
            "declared once, passed once: {text}"
        );
        assert!(text.contains("new_in("), "{text}");
    }

    /// A storage whose slot count is a const argument is declared, and
    /// constructed, with that argument: `HttpClientStorage::<2>`.
    #[test]
    fn a_generic_storage_bakes_its_slot_count() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &["wifi"]);
        let mut board = empty_board(&case);
        let Service::Protocol(mut client) = service("http_client", "http-client", None) else {
            panic!("the fixture is a protocol service");
        };
        client.ty =
            espforge_elements::model::RustType::parse("espforge_runtime::HttpClient::<2>").unwrap();
        client.new =
            espforge_elements::model::RustPath::parse("espforge_runtime::HttpClient::<2>::new_in")
                .unwrap();
        client.storage = Some(
            espforge_elements::model::RustType::parse("espforge_runtime::HttpClientStorage::<2>")
                .unwrap(),
        );
        board.services.push(Service::Protocol(client));
        let text = generated(&collect(&board), &case.meta).unwrap();
        // The printer wraps a long turbofish across lines (adding a trailing
        // comma inside it) and writes a *type's* argument without the
        // turbofish. Comparing without whitespace, with that comma dropped,
        // checks the types without pinning the layout.
        let flat: String = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .replace(",>", ">");
        assert!(
            flat.contains(
                "static__HTTP_CLIENT_STORAGE:espforge_runtime::HttpClientStorage<2>=\
                 espforge_runtime::HttpClientStorage::<2>::new()"
            ),
            "{text}"
        );
        assert!(
            flat.contains("pubhttp_client:espforge_runtime::HttpClient<2>,"),
            "{text}"
        );
        assert!(
            flat.contains("espforge_runtime::HttpClient::<2>::new_in(&__HTTP_CLIENT_STORAGE"),
            "{text}"
        );
    }

    /// A UART device owns its port: `Devices::new` builds one `UartDevice` and
    /// the constructor's first argument is that port. A bound `<uart>` emits
    /// no raw field beside it.
    #[test]
    fn a_uart_device_builds_its_port_inside_devices() {
        use espforge_elements::model::{
            Bus, ClaimedPin, Framing, Init, Parity, RustPath, RustType, StopBits, UartTransport,
        };

        fn claim(board: &mut Board, pin: u8) -> ClaimedPin {
            board
                .resources
                .claim(
                    Pin(pin),
                    Owner::new(OwnerKind::Uart, Ident::new("uart1", 0..0)),
                    0..0,
                    PinUse::Unspecified,
                )
                .expect("the port's pins are free")
        }

        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = empty_board(&case);
        let tx = claim(&mut board, 6);
        let rx = claim(&mut board, 5);
        let bus = board.resources.add_bus(Bus::Uart {
            id: Ident::new("uart1", 0..0),
            peripheral: "UART1".into(),
            tx,
            rx,
            baud: 9600,
            framing: Framing {
                parity: Parity::None,
                stop_bits: StopBits::One,
            },
        });
        board.devices.push(Device::Bus(BusDevice {
            id: Ident::new("gps", 0..0),
            tag: "uart_probe".into(),
            driver: Driver {
                feature: "uart-probe".into(),
                deps: Vec::new(),
                ty: RustType::parse("test::Gps").expect("a driver type"),
                new: RustPath::parse("test::Gps::new").expect("a constructor"),
                pins: Vec::new(),
                args: Vec::new(),
                factory_args: Vec::new(),
                runtimes: espforge_elements::RuntimeSet::BLOCKING,
                init: Init::Boot,
                async_ty: None,
                async_new: None,
                async_feature: None,
                package: None,
                expose: Vec::new(),
                dependencies: Vec::new(),
                manifest: None,
            },
            transport: BusTransport::Uart(UartTransport {
                bus,
                endpoint: None,
                async_endpoint: None,
            }),
        }));

        let parts = collect(&board);
        let text = generated(&parts, &case.meta).unwrap();
        assert!(
            text.contains(
                "let uart1_bus: espforge_runtime::UartDevice<esp_hal::Blocking> = \
                 espforge_runtime::UartDevice::build("
            ),
            "{text}"
        );
        assert!(text.contains("esp_hal::uart::Parity::None"), "{text}");
        assert!(text.contains("test::Gps::new(uart1_bus)"), "{text}");
        assert!(
            !text.contains("pub uart1:"),
            "a bound port is not also a raw field: {text}"
        );
    }

    /// A composite is two devices: one field each, one constructor call each.
    /// Emission knows nothing about `compose` — the lowerer hands it one
    /// `Device` per part, named `<id>_<part>`.
    #[test]
    fn a_composite_emits_a_field_per_part() {
        use espforge_elements::model::Bus;

        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = empty_board(&case);
        let owner = Owner::new(OwnerKind::I2c, Ident::new("i2c0", 0..0));
        let sda = board
            .resources
            .claim(Pin(6), owner.clone(), 0..0, PinUse::Bidirectional)
            .expect("sda is free");
        let scl = board
            .resources
            .claim(Pin(7), owner, 0..0, PinUse::Bidirectional)
            .expect("scl is free");
        board.resources.add_bus(Bus::I2c {
            id: Ident::new("i2c0", 0..0),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
        });

        // The composite and its part, named as lowering names them.
        board
            .devices
            .push(crate::codegen::test_support::device("panel", &[]));
        let mut part = crate::codegen::test_support::device("touch", &[]);
        if let Device::Bus(bus) = &mut part {
            bus.id = Ident::new("panel_touch", 0..0);
        }
        board.devices.push(part);

        let text = generated(&collect(&board), &case.meta).unwrap();
        assert!(
            text.contains("pub panel: espforge_drivers::panel,"),
            "{text}"
        );
        assert!(
            text.contains("pub panel_touch: espforge_drivers::touch,"),
            "{text}"
        );
        assert_eq!(
            text.matches("espforge_drivers::panel::new(").count(),
            1,
            "{text}"
        );
        assert_eq!(
            text.matches("espforge_drivers::touch::new(").count(),
            1,
            "{text}"
        );
    }

    /// A declared display fills `Devices` and builds its own I2C bus and
    /// addressed handle inside `Devices::new`.
    #[test]
    fn a_display_builds_its_bus_inside_devices() {
        use espforge_elements::model::{Bus, RustExpr, RustPath, RustType};

        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = empty_board(&case);
        let owner = Owner::new(OwnerKind::I2c, Ident::new("i2c0", 0..0));
        let sda = board
            .resources
            .claim(Pin(6), owner.clone(), 0..0, PinUse::Bidirectional)
            .expect("sda is free");
        let scl = board
            .resources
            .claim(Pin(7), owner, 0..0, PinUse::Bidirectional)
            .expect("scl is free");
        let bus = board.resources.add_bus(Bus::I2c {
            id: Ident::new("i2c0", 0..0),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
        });
        board.devices.push(Device::Bus(BusDevice {
            id: Ident::new("oled", 0..0),
            tag: "ssd1306".into(),
            driver: Driver {
                feature: "ssd1306".into(),
                deps: vec!["embedded-graphics".into()],
                ty: RustType::parse(
                    "espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>",
                )
                .expect("a display type"),
                new: RustPath::parse("espforge_drivers::Ssd1306Display::new")
                    .expect("a display constructor"),
                pins: Vec::new(),
                args: vec![
                    RustExpr::parse("espforge_drivers::Oled128x64").expect("a size"),
                    RustExpr::parse("espforge_drivers::DisplayRotation::Rotate0")
                        .expect("a rotation"),
                ],
                factory_args: Vec::new(),
                runtimes: espforge_elements::RuntimeSet::BLOCKING,
                init: espforge_elements::model::Init::Boot,
                async_ty: None,
                async_new: None,
                async_feature: None,
                package: None,
                expose: Vec::new(),
                dependencies: Vec::new(),
                manifest: None,
            },
            transport: BusTransport::I2c(I2cTransport {
                bus,
                address: 0x3C,
                endpoint: None,
                async_endpoint: None,
            }),
        }));
        let parts = collect(&board);
        let text = generated(&parts, &case.meta).unwrap();
        assert!(
            text.contains(
                "pub oled: espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>,"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "let i2c0_bus: espforge_runtime::I2cBus = espforge_runtime::I2cBus::build_in("
            ),
            "{text}"
        );
        // The address is a constructor argument, and the sharing is the
        // standard adapter's job.
        assert!(
            text.contains("embedded_hal_bus::i2c::RefCellDevice::new("),
            "{text}"
        );
        assert!(text.contains("i2c0_bus.cell(),"), "{text}");
        assert!(text.contains("device: \"oled\""), "{text}");
        assert!(text.contains("impl Devices"), "{text}");
        assert!(!text.contains("components.oled_bus"), "{text}");

        let boot = main(&board, &parts, &case.meta, &case.opts).unwrap();
        assert!(boot.contains("match Devices::new(&mut registry)"), "{boot}");
        // There is no `Components` struct any more.
        assert!(!boot.contains("Components"), "{boot}");
        // Drawing needs `&mut`, so the accessor hands one out.
        let lib = lib(&case.meta, true).unwrap();
        assert!(lib.contains("& mut $ctx .devices"), "{lib}");
    }

    /// A device that drives an SPI bus builds the bus and its CS-wrapped
    /// handle inside `Devices::new`: `Components` never carries an `Option`,
    /// and nothing can be taken out from under the app at boot.
    #[test]
    fn a_display_that_drives_an_spi_bus_builds_it_inside_devices() {
        use espforge_elements::model::{Bus, ClaimedPin, Pin, RustExpr, RustPath, RustType};

        fn claim(board: &mut Board, pin: u8, kind: OwnerKind) -> ClaimedPin {
            board
                .resources
                .claim(
                    Pin(pin),
                    Owner::new(kind, Ident::new("spi2", 0..0)),
                    0..0,
                    PinUse::Bidirectional,
                )
                .expect("the display's pins are free")
        }

        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = empty_board(&case);
        let sclk = claim(&mut board, 8, OwnerKind::Spi);
        let mosi = claim(&mut board, 7, OwnerKind::Spi);
        let bus = board.resources.add_bus(Bus::Spi {
            id: Ident::new("spi2", 0..0),
            peripheral: "SPI2".into(),
            sclk,
            mosi,
            miso: None,
            khz: 40_000,
            mode: 0,
            cs: None,
        });
        let cs = claim(&mut board, 9, OwnerKind::Device("ili9341".into()));
        board.devices.push(Device::Bus(BusDevice {
            id: Ident::new("tft", 0..0),
            tag: "ili9341".into(),
            driver: Driver {
                feature: "ili9341".into(),
                deps: vec!["embedded-graphics".into()],
                ty: RustType::parse("espforge_drivers::Ili9341Display").expect("a display type"),
                new: RustPath::parse("espforge_drivers::Ili9341Display::new")
                    .expect("a display constructor"),
                pins: Vec::new(),
                args: vec![
                    RustExpr::parse("espforge_drivers::Orientation::Landscape")
                        .expect("an orientation"),
                ],
                factory_args: Vec::new(),
                runtimes: espforge_elements::RuntimeSet::BLOCKING,
                init: espforge_elements::model::Init::Boot,
                async_ty: None,
                async_new: None,
                async_feature: None,
                package: None,
                expose: Vec::new(),
                dependencies: Vec::new(),
                manifest: None,
            },
            transport: BusTransport::Spi(SpiTransport {
                bus,
                cs,
                spi_config: None,
                cs_polarity: espforge_elements::model::CsPolarity::ActiveLow,
                endpoint: None,
                async_endpoint: None,
            }),
        }));

        let parts = collect(&board);
        let text = generated(&parts, &case.meta).unwrap();
        assert!(
            text.contains(
                "let spi2_bus: espforge_runtime::SpiBus = espforge_runtime::SpiBus::build_write_only_in("
            ),
            "{text}"
        );
        assert!(
            text.contains("espforge_runtime::ConfiguredSpiDevice::new("),
            "{text}"
        );
        assert!(text.contains("espforge_runtime::SpiConfig"), "{text}");
        assert!(text.contains("CsPolarity::ActiveLow"), "{text}");
        assert!(text.contains("device: \"tft\""), "{text}");
        assert!(text.contains("Ili9341Display::new("), "{text}");
        // No shared legacy handle is involved at all.
        assert!(!text.contains("components.tft_bus"), "{text}");
        assert!(!text.contains("Option<"), "{text}");
        assert!(!text.contains(".take()"), "{text}");

        // Every device is built inside `Devices::new`.
        let boot = main(&board, &parts, &case.meta, &case.opts).unwrap();
        assert!(boot.contains("match Devices::new(&mut registry)"), "{boot}");
        assert!(!boot.contains("Components"), "{boot}");
    }

    /// Every device on an embassy board is stored in an `AsyncMutex`, so the
    /// one `device!` macro awaits a lock for a synchronous part just as it does
    /// for an async one, and the old aliases still resolve.
    #[test]
    fn an_embassy_device_is_locked_through_a_guard() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &[]);
        let mut board = board_with_i2c(&case);
        board.devices.push(device("ssd1306", &[]));

        let parts = collect(&board);
        let text = generated(&parts, &case.meta).unwrap();
        assert!(
            text.contains("pub ssd1306: espforge_runtime::AsyncMutex<espforge_drivers::ssd1306>"),
            "{text}"
        );
        assert!(
            text.contains("espforge_runtime::AsyncMutex::new("),
            "{text}"
        );
        assert!(!text.contains("core::cell::RefCell"), "{text}");

        let lib = lib(&case.meta, true).unwrap();
        assert!(lib.contains("lock()"), "{lib}");
        assert!(lib.contains("await"), "{lib}");
        assert!(lib.contains("macro_rules! async_device"), "{lib}");
        assert!(lib.contains("macro_rules! component"), "{lib}");
        assert!(!lib.contains("borrow_mut"), "{lib}");
        assert!(!lib.contains("& mut $ctx .devices"), "{lib}");
    }

    /// A synchronous device with deferred init on an embassy board is stored
    /// as `AsyncMutex<Deferred<...>>`: the same lock spelling as every other
    /// device, with a synchronous factory inside.
    #[test]
    fn an_embassy_deferred_sync_device_is_locked_and_deferred() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &[]);
        let mut board = board_with_i2c(&case);
        let Device::Bus(mut bus_device) = device("ssd1306", &[]) else {
            panic!("the fixture is a bus device");
        };
        bus_device.driver.init = espforge_elements::model::Init::Deferred;
        let BusTransport::I2c(i2c) = &mut bus_device.transport else {
            panic!("the fixture is on an I2C bus");
        };
        i2c.endpoint = Some(
            espforge_elements::model::RustType::parse("espforge_drivers::I2cEndpoint")
                .expect("an endpoint type"),
        );
        board.devices.push(Device::Bus(bus_device));

        let text = generated(&collect(&board), &case.meta).unwrap();
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains(
                "espforge_runtime::AsyncMutex< espforge_runtime::Deferred<Ssd1306Factory>,"
            ),
            "{text}"
        );
        assert!(
            text.contains("impl espforge_runtime::DeferredInit for Ssd1306Factory"),
            "{text}"
        );
        assert!(!text.contains("core::cell::RefCell"), "{text}");
    }

    #[test]
    fn a_component_free_board_still_prints_empty_structs() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let board = Board {
            meta: case.meta.clone(),
            resources: ResourceGraph::default(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        };
        let text = generated(&collect(&board), &case.meta).unwrap();
        assert!(!text.contains("Components"), "{text}");
        assert!(text.contains("pub struct Devices {}"), "{text}");
        assert!(text.contains("pub struct Services {}"), "{text}");
        assert!(!text.contains("Services::new"), "{text}");
    }

    fn empty_board(case: &crate::codegen::test_support::Case) -> Board {
        Board {
            meta: case.meta.clone(),
            resources: ResourceGraph::default(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// `empty_board`, plus the I2C bus at slot 0 that the `device` fixture
    /// binds to: a test that emits the field needs a bus to build.
    fn board_with_i2c(case: &crate::codegen::test_support::Case) -> Board {
        let mut board = empty_board(case);
        let owner = Owner::new(OwnerKind::I2c, Ident::new("i2c0", 0..0));
        let sda = board
            .resources
            .claim(Pin(6), owner.clone(), 0..0, PinUse::Bidirectional)
            .expect("sda is free");
        let scl = board
            .resources
            .claim(Pin(7), owner, 0..0, PinUse::Bidirectional)
            .expect("scl is free");
        board.resources.add_bus(espforge_elements::model::Bus::I2c {
            id: Ident::new("i2c0", 0..0),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
        });
        board
    }

    /// `<logger>` emits the rule table and the logger static in `generated.rs`,
    /// and `main()` installs it before anything else runs.
    #[test]
    fn a_logger_project_installs_the_generated_filter() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut meta = case.meta.clone();
        meta.logger = Some(espforge_elements::LoggerConfig {
            level: LogLevel::Info,
            drivers: Some(LogLevel::Debug),
            network: None,
            hal: Some(LogLevel::Warn),
        });
        let board = empty_board(&case);
        let parts = collect(&board);

        let text = generated(&parts, &meta).unwrap();
        assert!(
            text.contains("pub static LOG_FILTERS: &[espforge_runtime::FilterRule]"),
            "{text}"
        );
        // The printer wraps a long item across lines; compare without
        // whitespace so the assertions pin the data, not the layout.
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("prefix: \"espforge_drivers\", level: log::LevelFilter::Debug"),
            "{text}"
        );
        // `hal` is `warn`, the default of the category, not the default level
        // (`info`), so it has a rule of its own.
        assert!(
            flat.contains("prefix: \"esp_hal\", level: log::LevelFilter::Warn"),
            "{text}"
        );
        assert!(
            flat.contains(
                "espforge_runtime::FilteredLogger::new( log::LevelFilter::Info, LOG_FILTERS,"
            ),
            "{text}"
        );

        let boot = main(&board, &parts, &meta, &case.opts).unwrap();
        let boot_flat: String = boot.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            boot_flat.contains("espforge_runtime::logger::init(&hello_world::generated::LOGGER)"),
            "{boot}"
        );
        assert!(!boot.contains("init_logger_from_env"), "{boot}");
    }

    /// `<logger level="none" />` installs nothing: the emitter also strips the
    /// `log` features, so there is no logger to point at.
    #[test]
    fn a_silent_project_installs_no_logger() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut meta = case.meta.clone();
        meta.logger = Some(espforge_elements::LoggerConfig {
            level: LogLevel::Off,
            ..Default::default()
        });
        let board = empty_board(&case);
        let parts = collect(&board);

        let text = generated(&parts, &meta).unwrap();
        assert!(!text.contains("LOG_FILTERS"), "{text}");
        assert!(!text.contains("LOGGER:"), "{text}");

        let boot = main(&board, &parts, &meta, &case.opts).unwrap();
        assert!(boot.contains("logger disabled"), "{boot}");
        assert!(!boot.contains("init_logger_from_env"), "{boot}");
        assert!(!boot.contains("logger::init"), "{boot}");
    }

    /// A project without `<logger>` keeps the logger it always had.
    #[test]
    fn a_project_without_a_logger_keeps_the_env_logger() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let board = empty_board(&case);
        let parts = collect(&board);
        let text = generated(&parts, &case.meta).unwrap();
        assert!(!text.contains("LOG_FILTERS"), "{text}");
        let boot = main(&board, &parts, &case.meta, &case.opts).unwrap();
        assert!(
            boot.contains("esp_println::logger::init_logger_from_env()"),
            "{boot}"
        );
    }

    /// The heap line, spelled exactly as the legacy `wifi_station` golden has
    /// it, in the position it has there: the radio allocates, and the
    /// allocator must exist before `PeripheralRegistry::new` takes the
    /// peripherals apart.
    #[test]
    fn wifi_installs_the_heap_the_radio_needs() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &["wifi"]);
        let board = empty_board(&case);
        let text = main(&board, &collect(&board), &case.meta, &case.opts).unwrap();
        assert!(
            text.contains("esp_alloc::heap_allocator!(size : 72 * 1024);"),
            "{text}"
        );
        let init = text.find("esp_hal::init(").unwrap();
        let heap = text.find("heap_allocator!").unwrap();
        let registry = text.find("PeripheralRegistry::new(").unwrap();
        assert!(init < heap && heap < registry, "{text}");
    }

    #[test]
    fn a_project_without_a_radio_allocates_nothing() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &[]);
        let board = empty_board(&case);
        let text = main(&board, &collect(&board), &case.meta, &case.opts).unwrap();
        assert!(!text.contains("heap_allocator"), "{text}");
    }

    /// A TLS client needs a bigger heap than the radio alone: `embedded-tls`
    /// keeps crypto scratch and record buffers.
    #[test]
    fn an_https_client_bumps_the_heap() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &["wifi"]);
        let mut board = empty_board(&case);
        board
            .services
            .push(service("https_client", "https-client", Some(98304)));
        let text = main(&board, &collect(&board), &case.meta, &case.opts).unwrap();
        assert!(
            text.contains("esp_alloc::heap_allocator!(size : 96 * 1024);"),
            "{text}"
        );
    }

    /// A secure WebSocket client needs the same `embedded-tls` scratch as
    /// HTTPS, so it bumps the heap too; the plain one does not.
    #[test]
    fn a_wss_client_bumps_the_heap_but_a_ws_client_does_not() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &["wifi"]);
        let mut board = empty_board(&case);
        board.services.push(service("ws_client", "ws-client", None));
        let text = main(&board, &collect(&board), &case.meta, &case.opts).unwrap();
        assert!(
            text.contains("esp_alloc::heap_allocator!(size : 72 * 1024);"),
            "{text}"
        );

        board.services.clear();
        board
            .services
            .push(service("wss_client", "wss-client", Some(98304)));
        let text = main(&board, &collect(&board), &case.meta, &case.opts).unwrap();
        assert!(
            text.contains("esp_alloc::heap_allocator!(size : 96 * 1024);"),
            "{text}"
        );
    }
}
