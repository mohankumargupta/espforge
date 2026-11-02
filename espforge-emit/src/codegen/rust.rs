//! The Rust backend: `src/generated.rs`, `src/lib.rs`, `src/bin/main.rs` and
//! `build.rs`, emitted from `Board` with `quote!` and printed by
//! `prettyplease`.
//!
//! Comments are inserted by [`super::comments`] after printing; the AST cannot
//! carry them. The four files are owned by the project (regenerated on every
//! build).
//!
//! `generated()` walks `board.components` and calls each component's
//! `Lowered::emit` through an `EmitCx`; it has no arm per element. With zero
//! components it emits exactly the empty `Components`, `Devices {}` and
//! `Services {}` of the helloworld golden.

use anyhow::{Context, Result};
use espforge_elements::element::EmitParts;
use espforge_elements::{Board, EmitCx, Lowered, MainCtx, Meta, ResolvedOptions, Runtime};
use proc_macro2::TokenStream;
use proc_macro2::Literal;
use quote::format_ident;

use super::comments::{self, Comment};
use crate::artifact::{Artifact, Ownership};

/// Every Rust artifact, in a deterministic order.
///
/// The elements are asked to emit **once**, here: the same `EmitParts` feed
/// `generated.rs` (fields, initialisers, prelude, safety notes) and `main.rs`
/// (startup statements). A component cannot be asked to emit twice.
pub fn generate(board: &Board, meta: &Meta, opts: &ResolvedOptions) -> Result<Vec<Artifact>> {
    let parts = collect(board);
    Ok(vec![
        Artifact {
            path: "build.rs".into(),
            content: build_rs(meta)?,
            ownership: Ownership::Owned,
        },
        Artifact {
            path: "src/bin/main.rs".into(),
            content: main(board, &parts, meta, opts)?,
            ownership: Ownership::Owned,
        },
        Artifact {
            path: "src/generated.rs".into(),
            content: generated(&parts, meta)?,
            ownership: Ownership::Owned,
        },
        Artifact {
            path: "src/lib.rs".into(),
            content: lib(meta, !board.devices.is_empty())?,
            ownership: Ownership::Owned,
        },
    ])
}

fn print(tokens: TokenStream) -> Result<String> {
    let file: syn::File = syn::parse2(tokens).context("generated Rust does not parse")?;
    Ok(prettyplease::unparse(&file))
}

fn generated(parts: &EmitParts, _meta: &Meta) -> Result<String> {
    let fields = &parts.fields;
    let inits = &parts.inits;
    let prelude = &parts.prelude;
    let device_fields = &parts.device_fields;
    let device_inits = &parts.device_inits;
    let service_fields = &parts.service_fields;
    let service_inits = &parts.service_inits;

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
                pub fn new(
                    registry: &mut PeripheralRegistry,
                    components: &mut Components,
                ) -> Self {
                    Self {
                        #(#device_inits)*
                    }
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
                pub fn new(components: &Components) -> Self {
                    Self {
                        #(#service_inits)*
                    }
                }
            }
        }
    };

    let tokens = quote::quote! {
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
                #(#prelude)*
                Self {
                    #(#inits)*
                }
            }
        }

        #devices

        #services
    };

    let mut text = comments::prepend(
        print(tokens)?,
        &["GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file."],
    );
    text = comments::apply(text, &safety_comments(&parts.safety))?;
    // Re-parse: an inserted comment must never change what the file means.
    syn::parse_file(&text).context("commented generated.rs does not parse")?;
    Ok(text)
}

/// Fill the `Components` and `Services` fields and initialisers, and collect
/// the `main()` startup statements, by asking every component and service to
/// emit itself. There is no `match` on `Component` here: an element's own file
/// owns every token it prints.
///
/// Components emit before services, so a service's `components.<network>`
/// reference is to a field declared above it.
fn collect(board: &Board) -> EmitParts {
    let mut cx = EmitCx::with_main(MainCtx::default());
    for component in &board.components {
        component.emit(board, &mut cx);
    }
    for device in &board.devices {
        device.emit(board, &mut cx);
    }
    for service in &board.services {
        service.emit(board, &mut cx);
    }
    cx.into_parts()
}

/// Turn the elements' `(anchor, line)` notes into comment blocks, in board
/// order.
fn safety_comments(safety: &[(String, Vec<String>)]) -> Vec<Comment> {
    safety
        .iter()
        .map(|(anchor, lines)| {
            if let [line] = lines.as_slice() {
                Comment::new(anchor.clone(), line.clone())
            } else {
                let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
                Comment::block(anchor.clone(), &refs)
            }
        })
        .collect()
}

fn lib(meta: &Meta, has_devices: bool) -> Result<String> {
    // A device is a draw target: drawing needs `&mut`, so with devices the
    // macro hands one out. A project without devices keeps the `&` spelling it
    // has always had (the helloworld golden depends on it byte for byte).
    let device_borrow = if has_devices {
        quote::quote! { &mut }
    } else {
        quote::quote! { & }
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
                #device_borrow $ctx.devices.$name
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

fn main(board: &Board, parts: &EmitParts, meta: &Meta, opts: &ResolvedOptions) -> Result<String> {
    let crate_name = format_ident!("{}", meta.crate_name());
    let heap = heap_line(board, opts);
    let startup = &parts.startup;

    // The empty `Devices {}` keeps the helloworld golden byte-identical; a
    // declared device gets the constructor that takes its bus from
    // `Components`.
    let (devices, components_mut) = if parts.device_fields.is_empty() {
        (
            quote::quote! { let devices = Devices {}; },
            quote::quote! {},
        )
    } else {
        (
            quote::quote! { let devices = Devices::new(&mut registry, &mut components); },
            quote::quote! { mut },
        )
    };

    // `Services {}` without a `new` when nothing is declared keeps the
    // helloworld golden byte-identical; a declared service gets the
    // `Services::new(&components)` constructor instead.
    let services = if parts.service_fields.is_empty() {
        quote::quote! { let services = Services {}; }
    } else {
        quote::quote! { let services = Services::new(&components); }
    };

    let tokens = match meta.runtime {
        Runtime::Blocking => quote::quote! {
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

                let #components_mut components = Components::new(&mut registry);
                #devices
                #services

                let ctx = Context { logger, delay, components, devices, services };
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

                let #components_mut components = Components::new(&mut registry);
                #devices

                let timg0 = TimerGroup::new(registry.peripherals.TIMG0);
                esp_rtos::start(timg0.timer0, registry.peripherals.FROM_CPU_INTR0);

                #(#startup)*

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
        },
    };

    let mut text = comments::prepend(
        print(tokens)?,
        &[
            "GENERATED BY ESPFORGE — DO NOT EDIT.",
            "Source of truth is the project .espf file.",
        ],
    );
    let mut notes = vec![Comment::new(
        "components = Components::new(",
        "Built first: it borrows the whole registry, which the moves below break.",
    )];
    if meta.runtime == Runtime::Embassy {
        notes.push(Comment::block(
            "let logger = espforge_runtime::Logger::new();",
            &[
                "Hoisted above the component/device wiring because some drivers take",
                "`&ctx.delay` at construct time.",
            ],
        ));
        notes.push(Comment::new(
            "let services =",
            "Services need the scheduler (started above) and the spawner.",
        ));
    }
    text = comments::apply(text, &notes)?;
    Ok(text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::{case, service};
    use espforge_elements::components::recipe::RecipeComponent;
    use espforge_elements::{Component, Ident, Owner, OwnerKind, Pin, PinUse, ResourceGraph};

    /// `blink`: one LED on GPIO18.
    fn blink() -> (Board, Meta) {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = Board {
            meta: case.meta.clone(),
            resources: ResourceGraph::default(),
            components: Vec::new(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        };
        let id = Ident::new("red_led", 0..0);
        let pin = board
            .resources
            .claim(
                Pin(18),
                Owner::new(OwnerKind::Recipe("led"), id.clone()),
                0..0,
                PinUse::Output,
            )
            .expect("pin 18 is free");
        board
            .components
            .push(Component::Recipe(RecipeComponent::new(
                "led",
                id,
                pin,
                vec![false],
            )));
        (board, case.meta)
    }

    #[test]
    fn a_led_emits_its_field_init_and_safety_note() {
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
        assert!(
            text.contains("// SAFETY: espforge checked that no other component uses GPIO18."),
            "{text}"
        );
        assert!(text.contains("false,"), "{text}");
    }

    /// A declared display fills `Devices`, gets a constructor that takes its
    /// bus out of `Components`, and flips `components` to `mut` in `main`.
    #[test]
    fn a_display_fills_the_devices_struct() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let mut board = empty_board(&case);
        board.devices.push(espforge_elements::Device {
            id: Ident::new("oled", 0..0),
            tag: "ssd1306",
            bus_component: Ident::new("oled_bus", 0..0),
            feature: "ssd1306".into(),
            deps: vec!["embedded-graphics".into()],
            ty: "espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>".into(),
            new: "espforge_drivers::Ssd1306Display::new".into(),
            takes_bus: false,
            taken: None,
            pins: Vec::new(),
            args: vec![
                "espforge_drivers::Oled128x64".into(),
                "espforge_drivers::DisplayRotation::Rotate0".into(),
            ],
        });
        let parts = collect(&board);
        let text = generated(&parts, &case.meta).unwrap();
        assert!(
            text.contains(
                "pub oled: espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>,"
            ),
            "{text}"
        );
        assert!(text.contains("components.oled_bus"), "{text}");
        assert!(text.contains("impl Devices"), "{text}");

        let boot = main(&board, &parts, &case.meta, &case.opts).unwrap();
        assert!(
            boot.contains("let mut components = Components::new("),
            "{boot}"
        );
        assert!(
            boot.contains("let devices = Devices::new(&mut registry, &mut components);"),
            "{boot}"
        );
        // Drawing needs `&mut`, so the accessor hands one out.
        let lib = lib(&case.meta, true).unwrap();
        assert!(lib.contains("& mut $ctx .devices"), "{lib}");
    }

    #[test]
    fn a_component_free_board_still_prints_empty_structs() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let board = Board {
            meta: case.meta.clone(),
            resources: ResourceGraph::default(),
            components: Vec::new(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        };
        let text = generated(&collect(&board), &case.meta).unwrap();
        assert!(text.contains("pub struct Components {}"), "{text}");
        assert!(text.contains("pub struct Services {}"), "{text}");
        assert!(!text.contains("Services::new"), "{text}");
        assert!(!text.contains("SAFETY"), "{text}");
    }

    fn empty_board(case: &crate::codegen::test_support::Case) -> Board {
        Board {
            meta: case.meta.clone(),
            resources: ResourceGraph::default(),
            components: Vec::new(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        }
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
        board
            .services
            .push(service("ws_client", "ws-client", None));
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
