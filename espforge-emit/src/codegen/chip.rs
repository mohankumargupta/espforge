//! The chip facts the data backends need, resolved once per project.
//!
//! The backends read these from here, so `esp-metadata-generated` is consulted
//! in one place per build rather than once per file. (`chip.name`,
//! `chip.rust_target`, `chip.riscv` and `chip.xtensa` used to be context names
//! the template engine understood.)

use anyhow::{Context, Result};

/// One chip from `esp-metadata-generated`, as the emitted files describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// Chip id as ESPF spells it (`esp32c3`).
    pub name: String,
    /// Rust target triple (`riscv32imc-unknown-none-elf`).
    pub rust_target: String,
    /// True for Xtensa chips (`esp32`, `esp32s2`, `esp32s3`).
    pub is_xtensa: bool,
}

impl Chip {
    /// Resolve `name`; unknown chips are an error, not a fallback.
    pub fn resolve(name: &str) -> Result<Self> {
        let chip = esp_metadata_generated::Chip::iter()
            .find(|chip| chip.name() == name)
            .with_context(|| format!("unknown chip `{name}`"))?;
        Ok(Self {
            name: chip.name().to_string(),
            rust_target: chip.target().to_string(),
            is_xtensa: chip.is_xtensa(),
        })
    }

    pub fn xtensa(&self) -> bool {
        self.is_xtensa
    }

    pub fn riscv(&self) -> bool {
        !self.is_xtensa
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_riscv_chip_has_a_target_and_is_not_xtensa() {
        let chip = Chip::resolve("esp32c3").unwrap();
        assert_eq!(chip.name, "esp32c3");
        assert_eq!(chip.rust_target, "riscv32imc-unknown-none-elf");
        assert!(chip.riscv());
        assert!(!chip.xtensa());
    }

    #[test]
    fn an_xtensa_chip_is_flagged_as_such() {
        let chip = Chip::resolve("esp32s3").unwrap();
        assert_eq!(chip.rust_target, "xtensa-esp32s3-none-elf");
        assert!(chip.xtensa());
        assert!(!chip.riscv());
    }

    #[test]
    fn unknown_chips_are_an_error() {
        assert!(Chip::resolve("esp99").is_err());
    }
}
