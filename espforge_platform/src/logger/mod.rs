#[derive(Clone, Copy)]
pub struct Logger;

impl Logger {
   pub fn new() -> Self{
     Self{}
   }
   
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
