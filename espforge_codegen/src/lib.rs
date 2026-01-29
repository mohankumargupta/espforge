use espforge_common::EspforgeConfiguration;
use crate::context::CodegenContext;
use anyhow::{Result, Context as _};
use quote::quote;
use syn;
use prettyplease;
use quote::format_ident;

pub mod builders;
pub mod component_builders;
pub mod generator;
pub mod scaffold;
pub mod context;
pub mod resolver;

pub use scaffold::esp_generate;

pub fn generate_lib_source() -> Result<String> {
    let tokens = quote! {
        #![no_std]
        pub mod app;
        pub mod generated;
        pub use generated::Context;
    };

    let syntax_tree = syn::parse2(tokens).context("Failed to parse generated library structure")?;
    Ok(prettyplease::unparse(&syntax_tree))
}

pub fn generate_entry_point_source(model: &EspforgeConfiguration) -> Result<String> {
    let crate_name = model.get_name().replace('-', "_");
    let crate_ident = format_ident!("{}", crate_name);

    let tokens = quote! {
        #![no_std]
        #![no_main]

        use espforge_platform::esp_hal;
        use esp_backtrace as _;
        
        use #crate_ident::generated::{Context, PeripheralRegistry, Components, Devices};
        use espforge_platform::logger::Logger;
        use espforge_platform::delay::Delay;

        esp_bootloader_esp_idf::esp_app_desc!();

        #[esp_hal::main]
        fn main() -> ! {
            esp_println::logger::init_logger_from_env();
            esp_println::print!("\x1b[20h");
            // System Initialization
            let peripherals = esp_hal::init(esp_hal::Config::default());

            // Platform Drivers
            let mut delay = Delay::new();
            let logger = Logger::new();

            // Resource Provisioning
            let registry = PeripheralRegistry::new(peripherals);
            
            // Component & Device Initialization
            let mut components = Components::new(&registry);
            let devices = Devices::new(&registry, &mut components, &mut delay);

            // Context creation (borrowing registry)
            let mut ctx = Context {
                logger,
                delay,
                registry: &registry,
                components,
                devices
            };

            #crate_ident::app::setup(&mut ctx);

            loop {
                #crate_ident::app::forever(&mut ctx);
            }
        }
    };

    let syntax_tree = syn::parse2(tokens).context("Failed to parse generated main.rs")?;
    Ok(prettyplease::unparse(&syntax_tree))
}

pub fn generate_components_source(model: &EspforgeConfiguration) -> Result<String> {
    let ctx = CodegenContext::build(model)?;
    let tokens = ctx.generate()?;
    
    let output = quote! {
        use espforge_platform::esp_hal;
        use core::cell::RefCell;
        use espforge_platform::gpio::{GPIOInput, GPIOOutput};
        use espforge_platform::esp_hal::gpio::{AnyPin, InputPin, OutputPin, Pin};
        use espforge_platform::esp_hal::delay::Delay as HalDelay;
        
        use espforge_platform::bus::{SpiDevice, I2cDevice};
        use embedded_hal_bus::spi::RefCellDevice as SpiRefCellDevice;
        use embedded_hal_bus::i2c::RefCellDevice as I2cRefCellDevice;
        
        use espforge_platform::esp_hal::spi::master::Spi;
        use espforge_platform::esp_hal::i2c::master::I2c;
        use espforge_platform::esp_hal::Blocking;
        use espforge_platform::esp_hal::gpio::Output;
        use espforge_components::components::button::ButtonConfig;
        
        #tokens
    };

    if let Err(e) = syn::parse2::<syn::File>(output.clone()) {
        println!("---------------------------------------------------");
        println!("DEBUG: Generated Code causing syntax error:");
        println!("Error: {}", e);
        println!("{}", output);
        println!("---------------------------------------------------");
        return Err(anyhow::anyhow!("Failed to parse generated tokens: {}", e));
    }

    let syntax_tree = syn::parse2(output).context("Failed to parse generated tokens")?;
    Ok(prettyplease::unparse(&syntax_tree))
}