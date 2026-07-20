// `ili9341` device driver (ADR-006). Lives under `devices/`. A terminal device
// that shares an `spi` component by value (a `Copy` `SpiBus` handle) plus a
// private CS pin (wrapped as `espforge_runtime::components::SpiDevice`), and
// claims dc/rst pins by value (ADR-003/008). Mirrors v1's `ILI9341Plugin`.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{DepKind, ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct Ili9341Driver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &Ili9341Driver;

impl Driver for Ili9341Driver {
    fn kind(&self) -> &str {
        "ili9341"
    }
    fn tier(&self) -> Tier {
        Tier::Device
    }

    fn type_name(&self) -> &str {
        "Ili9341"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { spi: $main_spi, dc: $pin_dc, rst: $pin_rst, cs: $pin_cs }
        let spi_field = inst
            .deps
            .iter()
            .find(|d| d.kind == DepKind::Instance)
            .map(|d| codegen::sanitize(&d.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let dc = ctx.backend.gpio_output(&codegen::gpio_field_from_with(ctx, inst, "dc"), false);
        let rst = ctx.backend.gpio_output(&codegen::gpio_field_from_with(ctx, inst, "rst"), false);
        let cs = ctx.backend.gpio_output(&codegen::gpio_field_from_with(ctx, inst, "cs"), true);
        // `SpiDevice::new` takes the bus `Copy` handle, the device's private CS
        // (as an `Output`), and the shared `Delay` (Copy).
        let spi_device = format!(
            "espforge_runtime::components::SpiDevice::<esp_hal::Blocking>::new(components.{spi_field}, {cs}, delay)"
        );
        Construction::for_instance(
            inst,
            ctx.backend.ctor(Tier::Device, "Ili9341", &[spi_device, dc, rst]),
        )
    }
}
