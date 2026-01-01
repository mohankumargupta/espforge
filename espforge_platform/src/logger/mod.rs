use esp_println::logger::init_logger_from_env;

pub fn init() {
    init_logger_from_env();
}

pub struct Logger;

impl Logger {
    pub fn info(&self, msg: impl core::fmt::Display) {
        log::info!("{}", msg);
    }

    pub fn warn(&self, msg: impl core::fmt::Display) {
        log::warn!("{}", msg);
    }

    pub fn error(&self, msg: impl core::fmt::Display) {
        log::error!("{}", msg);
    }

    pub fn debug(&self, msg: impl core::fmt::Display) {
        log::debug!("{}", msg);
    }

    pub fn trace(&self, msg: impl core::fmt::Display) {
        log::trace!("{}", msg);
    }
}
