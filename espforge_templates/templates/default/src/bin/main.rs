#![no_std]
#![no_main]

// based on esp-generate generator version: 1.0.1

use esp_backtrace as _;
use esp_hal::main;

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default();
    let _peripherals = esp_hal::init(config);

    //setup goes here

    loop {
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_millis(500) {
             //forever goes here
        }
    }
}
