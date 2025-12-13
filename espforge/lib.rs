pub mod compile;
pub mod config;
pub mod export;
pub mod generate;
pub mod manifest;
pub mod nibblers;
pub mod resolver;
pub mod template_utils;

pub mod templates {
    include!(concat!(env!("OUT_DIR"), "/generated_templates.rs"));
}
