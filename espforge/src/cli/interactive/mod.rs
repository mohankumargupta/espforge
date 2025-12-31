pub mod catalog;
pub mod dialoguer;

pub use catalog::{ChipCatalog, ExampleCatalog};
pub use dialoguer::DialoguerPrompter;

use miette::Result;

pub trait Prompter {
    fn select_example(&self) -> Result<String>;
    fn prompt_project_name(&self, default: &str) -> Result<String>;
    fn select_chip(&self) -> Result<String>;
    fn confirm_overwrite(&self, dir_name: &str) -> Result<bool>;
}
