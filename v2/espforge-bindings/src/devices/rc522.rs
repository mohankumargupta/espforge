// `rc522` device driver (ADR-006). Lives under `devices/`. A terminal device
// that shares an `spi` component by value (a `Copy` `SpiBus` handle) plus a
// private CS pin (wrapped as `espforge_runtime::components::SpiDevice`), and
// optionally claims an `rst` pin by value (ADR-003/008). Phase 1 only:
// transport plus reset and defaults.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{DepKind, ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct Rc522Driver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &Rc522Driver;

impl Driver for Rc522Driver {
    fn kind(&self) -> &str {
        "rc522"
    }
    fn tier(&self) -> Tier {
        Tier::Device
    }

    fn type_name(&self) -> &str {
        // Concrete production alias (`Rc522` itself is generic over
        // `embedded-hal` traits for host testability; the alias pins the
        // `esp-hal` types so generated field types need no parameters).
        "EspRc522"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { spi: $main_spi, cs: $pin_cs, rst: $pin_rst? }
        // `rst` is optional (mirrors esphome's optional `reset_pin`).
        let spi_field = inst
            .deps
            .iter()
            .find(|d| d.kind == DepKind::Instance)
            .map(|d| codegen::sanitize(&d.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let cs = ctx
            .backend
            .gpio_output(&codegen::gpio_field_from_with(ctx, inst, "cs"), true);
        // `SpiDevice::new` takes the bus `Copy` handle, the device's private CS
        // (as an `Output`), and the shared `Delay` (Copy).
        let spi_device = format!(
            "espforge_runtime::components::SpiDevice::<esp_hal::Blocking>::new(components.{spi_field}, {cs}, delay)"
        );
        let rst = if inst.with.get("rst").is_some() {
            format!(
                "Some({})",
                ctx.backend
                    .gpio_output(&codegen::gpio_field_from_with(ctx, inst, "rst"), false)
            )
        } else {
            "None".to_string()
        };
        Construction::for_instance(
            inst,
            ctx.backend.ctor(
                Tier::Device,
                "EspRc522",
                &[spi_device, rst, "delay".to_string()],
            ),
        )
    }
}
