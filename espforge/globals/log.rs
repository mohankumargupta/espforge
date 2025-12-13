use core::fmt::{Display, LowerHex};

pub struct EspforgeLog;

impl EspforgeLog {
    pub fn new() -> Self {
        esp_println::logger::init_logger_from_env();
        esp_println::print!("\x1b[20h"); // needed for wokwi
        Self
    }

    pub fn info<T: Display>(&self, msg: T) {
        log::info!("{}", msg);
    }

    pub fn print_hex<T: LowerHex>(&self, msg: T) {
        log::info!("0x{:x}", msg);
    }
}

