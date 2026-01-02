#![no_std]
#![no_main]

// based on esp-generate generator version: 1.0.1

use esp_backtrace as _;
use esp_hal::main;

// Import app and Context from the library crate
use {{ crate_name }}::{Context, app};

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default();
    let _peripherals = esp_hal::init(config);

    let mut ctx = Context::new();

    // Call setup
    app::setup(&mut ctx);

    loop {
        // Call forever loop
        app::forever(&mut ctx);
    }
}
