// `ssd1306` device driver (ADR-006). Lives under `devices/`. A terminal device
// that shares an `i2c` component by reference and claims reset/DC pins by value
// (ADR-003/008).

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{DepKind, ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct Ssd1306Driver;

pub static SSD1306: Ssd1306Driver = Ssd1306Driver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &SSD1306;

impl Driver for Ssd1306Driver {
    fn kind(&self) -> &str {
        "ssd1306"
    }
    fn tier(&self) -> Tier {
        Tier::Device
    }

    fn type_name(&self) -> &str {
        "Ssd1306"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { component: $i2c_master, address: 0x3C, width: 128, height: 64 }
        // `component` is moved by value into the device (SSD1306 owns the bus
        // exclusively over I2C, matching v1's `SSD1306Device::new(i2c)`).
        // `I2cBus` is a `Copy` handle, so this is a pointer bitcopy (ADR-008).
        let _ = ctx; // no pins needed for this driver
        let component_field = inst
            .deps
            .iter()
            .find(|d| d.kind == DepKind::Instance)
            .map(|d| codegen::sanitize(&d.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        // `address` is a `with` value (e.g. 0x3C); default to the common SSD1306
        // address if absent.
        let address = inst
            .with
            .get("address")
            .and_then(|v| v.as_i64())
            .unwrap_or(0x3C) as u8;
        Construction::for_instance(
            inst,
            ctx.backend.ctor(
                Tier::Device,
                "Ssd1306",
                &[
                    format!("components.{component_field}"),
                    format!("{address}_u8"),
                ],
            ),
        )
    }
}
