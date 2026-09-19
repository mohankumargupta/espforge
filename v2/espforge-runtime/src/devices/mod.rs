//! Runtime implementations of terminal Devices: Ssd1306, Ili9341, ...
//!
//! Module gated by its `espforge-runtime` feature (design §19.1).

#[cfg(feature = "ssd1306")]
pub mod ssd1306;
#[cfg(feature = "ili9341")]
pub mod ili9341;
#[cfg(feature = "rc522")]
pub mod rc522;

#[cfg(feature = "ssd1306")]
pub use ssd1306::Ssd1306;
#[cfg(feature = "ili9341")]
pub use ili9341::Ili9341;
#[cfg(feature = "rc522")]
pub use rc522::Rc522;
// Production alias (concrete `esp-hal` types); `esp-hal` exists only on
// riscv32/xtensa, so the alias is target-gated. Emitted by
// `espforge-bindings` as the generated field/ctor type.
#[cfg(all(
    feature = "rc522",
    any(target_arch = "riscv32", target_arch = "xtensa")
))]
pub use rc522::EspRc522;
