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
        // with: { bus: $i2c_bus, reset: $GPIO16, dc: $GPIO5 }
        // `bus` is a shared component -> borrow its field by reference; reset/dc
        // are control pins moved in by value (wrapped as Output by the backend).
        let bus_field = inst
            .deps
            .iter()
            .find(|d| d.kind == DepKind::Instance)
            .map(|d| codegen::sanitize(&d.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        // Control pins are always active-high in this device, so render them
        // with the backend (polarity-aware) at idle-low (ADR-003). Resolve the
        // `$gpioN` ref through the peripheral table to its esp_hal field.
        let reset = ctx.backend.gpio_output(&codegen::gpio_field_from_with(ctx, inst, "reset"), false);
        let dc = ctx.backend.gpio_output(&codegen::gpio_field_from_with(ctx, inst, "dc"), false);
        Construction::for_instance(
            inst,
            ctx.backend.ctor(
                Tier::Device,
                "Ssd1306",
                &[format!("components.{bus_field}.bus()"), reset, dc],
            ),
        )
    }
}
