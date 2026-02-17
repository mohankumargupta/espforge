use super::common::format_generated_source;
use super::constants::origins;
use crate::allocators::AllocatorGenerator;
use anyhow::{Context, Result};
use espforge_configuration::{EspforgeConfiguration, RuntimeMode};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Generates the main entry point (main.rs) for the project
pub fn generate_entry_point_source(model: &EspforgeConfiguration) -> Result<String> {
    let generator: Box<dyn EntryPointGenerator> = match model.get_runtime() {
        RuntimeMode::Embassy => Box::new(EmbassyEntryPoint),
        RuntimeMode::None => Box::new(BlockingEntryPoint),
    };

    let code = generator
        .generate(model)
        .context("Failed to generate entry point code")?;

    Ok(format_generated_source(&code, origins::ENTRY_POINT))
}

trait EntryPointGenerator {
    fn generate(&self, model: &EspforgeConfiguration) -> Result<String>;
}

struct BlockingEntryPoint;

impl EntryPointGenerator for BlockingEntryPoint {
    fn generate(&self, model: &EspforgeConfiguration) -> Result<String> {
        let common = CommonEntryPointCode::new(model);
        let allocators = AllocatorGenerator::generate(model);
        
        // Extract fields from common
        let imports = common.imports;
        let static_cells = common.static_cells;
        let init_logger = common.init_logger;
        let init_registry = common.init_registry;
        let init_components = common.init_components;
        let init_devices = common.init_devices;
        let init_context = common.init_context;

        let tokens = quote! {
            #![no_std]
            #![no_main]

            use esp_backtrace as _;
            #imports
            use static_cell::StaticCell;

            #static_cells

            #[esp_hal::main]
            fn main() -> ! {
                #init_logger

                // Initialize peripherals
                let peripherals = esp_hal::init(esp_hal::Config::default());

                #allocators

                #init_registry
                #init_components
                #init_devices
                #init_context

                // Run user setup
                app::setup(&mut ctx);

                // Run user loop forever
                loop {
                    app::forever(&mut ctx);
                }
            }
        };

        let syntax_tree = syn::parse2(tokens)
            .context("Failed to parse blocking entry point syntax")?;

        Ok(prettyplease::unparse(&syntax_tree))
    }
}

struct EmbassyEntryPoint;

impl EntryPointGenerator for EmbassyEntryPoint {
    fn generate(&self, model: &EspforgeConfiguration) -> Result<String> {
        let common = CommonEntryPointCode::new(model);
        let allocators = AllocatorGenerator::generate(model);
        
        // Extract fields from common
        let imports = common.imports;
        let static_cells = common.static_cells;
        let init_logger = common.init_logger;
        let init_registry = common.init_registry;
        let init_embassy_runtime = common.init_embassy_runtime;
        let init_components = common.init_components;
        let init_devices = common.init_devices;
        let init_context = common.init_context;

        let tokens = quote! {
            #![no_std]
            #![no_main]

            use esp_backtrace as _;
            use embassy_executor::Spawner;
            #imports
            use static_cell::StaticCell;
            use esp_hal::interrupt::software::SoftwareInterruptControl;
            use esp_hal::timer::timg::TimerGroup;

            #static_cells

            #[esp_rtos::main]
            async fn main(spawner: Spawner) {
                #init_logger

                // Initialize peripherals
                let peripherals = esp_hal::init(esp_hal::Config::default());

                #allocators

                #init_registry
                #init_embassy_runtime
                #init_components
                #init_devices
                #init_context

                // Run user setup
                app::setup(&mut ctx, spawner).await;

                // Run user loop forever
                loop {
                    app::forever(&mut ctx).await;
                }
            }
        };

        let syntax_tree = syn::parse2(tokens)
            .context("Failed to parse embassy entry point syntax")?;

        Ok(prettyplease::unparse(&syntax_tree))
    }
}

struct CommonEntryPointCode {
    imports: TokenStream,
    static_cells: TokenStream,
    init_logger: TokenStream,
    init_registry: TokenStream,
    init_components: TokenStream,
    init_devices: TokenStream,
    init_context: TokenStream,
    init_embassy_runtime: TokenStream,
}

impl CommonEntryPointCode {
    fn new(model: &EspforgeConfiguration) -> Self {
        let crate_name = model.get_name().replace('-', "_");
        let crate_ident = format_ident!("{}", crate_name);

        let imports = quote! {
            use #crate_ident::*;
        };

        let static_cells = quote! {
            // Static storage cells - initialized once at startup
            static REGISTRY_CELL: StaticCell<PeripheralRegistry> = StaticCell::new();
            static COMPONENTS_CELL: StaticCell<Components> = StaticCell::new();
            static DEVICES_CELL: StaticCell<Devices> = StaticCell::new();
        };

        let init_logger = quote! {
            esp_println::logger::init_logger_from_env();
            esp_println::print!("\x1b[20h");
        };

        let init_registry = quote! {
            // Initialize registry with static lifetime
            let registry = REGISTRY_CELL.init(PeripheralRegistry::new(peripherals));
            unsafe { REGISTRY = registry as *mut _; }
        };

        let init_components = quote! {
            // Initialize components with static lifetime
            // Components take ownership from registry
            let components = COMPONENTS_CELL.init(
                Components::new(unsafe { &mut *REGISTRY })
            );
            unsafe { COMPONENTS = components as *mut _; }
        };

        let init_devices = quote! {
            // Initialize devices with static lifetime
            // Devices use components and take pins from registry
            let mut delay = espforge_platform::delay::Delay::new();
            let devices = DEVICES_CELL.init(
                Devices::new(
                    unsafe { &mut *COMPONENTS },
                    unsafe { &mut *REGISTRY },
                    &mut delay
                )
            );
            unsafe { DEVICES = devices as *mut _; }
        };

        let init_context = quote! {
            // Create context (only contains utilities)
            let logger = espforge_platform::logger::Logger::new();
            let mut ctx = Context {
                logger,
                delay,
            };
        };

        let init_embassy_runtime = quote! {
            let sw_int_raw = registry.sw_interrupt.borrow_mut().take().expect("SW_INTERRUPT missing");
            let timg0_raw = registry.timg0.borrow_mut().take().expect("timg0 missing");
            let sw_int = SoftwareInterruptControl::new(sw_int_raw);
            let timg0 = TimerGroup::new(timg0_raw);
            esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
        };

        Self {
            imports,
            static_cells,
            init_logger,
            init_registry,
            init_components,
            init_devices,
            init_context,
            init_embassy_runtime,
        }
    }
}
