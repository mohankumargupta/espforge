// `spi` component driver (ADR-006). Wires an SPI peripheral by value. Lives
// under `components/` alongside the other reusable capability drivers.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct SpiDriver;

pub static SPI: SpiDriver = SpiDriver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &SPI;

impl Driver for SpiDriver {
    fn kind(&self) -> &str {
        "spi"
    }
    fn tier(&self) -> Tier {
        Tier::Component
    }

    fn type_name(&self) -> &str {
        "SpiBus"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { bus: $spi2 } -> claims the SPI peripheral by value. Resolve
        // the claimed peripheral to its esp_hal field and pull the bus's
        // pins/mode/frequency from the IR (model refactor C: typed SpiInit).
        let field = inst
            .claims
            .first()
            .map(|name| codegen::peripheral_field(&ctx.peripherals, &name.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let (mosi, miso, sclk, cs, mode, freq) = inst
            .claims
            .first()
            .and_then(|name| ctx.peripherals.iter().find(|p| p.name == name.name))
            .and_then(|p| p.bus.as_ref())
            .and_then(|b| match b {
                espforge_model::ir::BusInit::Spi(s) => Some(s),
                _ => None,
            })
            .map(|s: &espforge_model::ir::SpiInit| {
                (
                    s.mosi.unwrap_or(0).to_string(),
                    s.miso.unwrap_or(0).to_string(),
                    s.sclk.unwrap_or(0).to_string(),
                    s.cs,
                    s.mode.unwrap_or(0) as u32,
                    s.frequency_khz.unwrap_or(100).to_string(),
                )
            })
            .unwrap_or_else(|| {
                (
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    None,
                    0u32,
                    "100".to_string(),
                )
            });
        let cs = cs.map(|n| n.to_string());
        let build = ctx.backend.spi_master(
            &field,
            &mosi,
            &miso,
            &sclk,
            cs.as_deref(),
            mode as u8,
            freq.parse().unwrap_or(100),
        );
        // Allocate the owned `Spi` once in a static `RefCell` (v1 idiom, ADR-008)
        // and surface a `Copy` `SpiBus` handle into the `Components` field.
        let cell = format!(
            "{{ static {id}_SPI_CELL: static_cell::StaticCell<core::cell::RefCell<esp_hal::spi::master::Spi<'static, esp_hal::Blocking>>> = static_cell::StaticCell::new(); espforge_runtime::components::SpiBus::from_ref({id}_SPI_CELL.init(core::cell::RefCell::new({build}))) }}",
            id = codegen::sanitize(&inst.id),
            build = build,
        );
        Construction::for_instance(inst, cell)
    }
}
