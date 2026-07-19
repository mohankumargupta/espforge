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

    fn type_name_for(&self, inst: &ResolvedInstance, ctx: &GenContext) -> String {
        let dm = if ctx.is_embassy {
            "esp_hal::Async"
        } else {
            "esp_hal::Blocking"
        };
        let has_cs = inst.with.get("cs").and_then(|v| v.as_u64()).is_some();
        if has_cs {
            format!("SpiDevice<{dm}>")
        } else {
            format!("SpiBus<{dm}>")
        }
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { bus: $spi2, cs: 9 } -> claims the SPI peripheral by value and
        // (optionally) a CS pin. The bus is built without CS; CS is device-local
        // and lives in `SpiDevice` (design §20.5). Each component declaring a
        // `cs` pin gets its own `SpiDevice` wrapping the shared bus.
        let field = inst
            .claims
            .first()
            .map(|name| codegen::peripheral_field(&ctx.peripherals, &name.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let (mosi, miso, sclk, mode, freq, cs) = inst
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
                    s.mode.unwrap_or(0),
                    s.frequency_khz.unwrap_or(100),
                    // CS comes from the *component* spec, not the bus (§20.5).
                    inst.with.get("cs").and_then(|v| v.as_u64()).map(|n| n as u32),
                )
            })
            .unwrap_or_else(|| {
                (
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    0u8,
                    100,
                    None,
                )
            });
        let build = ctx.backend.spi_master(&field, &mosi, &miso, &sclk, mode, freq);
        // `SpiBus::build` returns the correct `Dm` variant for this build (the
        // `embassy` feature selects the async impl), so no `.into_async()` (§20).
        let bus_expr = format!(
            "{build}.expect(\"{id}: invalid SPI config (check frequency_kHz/mode)\")",
            id = inst.id
        );
        // Wrap in a `SpiDevice` when the component declares a CS pin (§20.5).
        let expr = match cs {
            Some(cs_pin) => format!(
                "espforge_runtime::components::SpiDevice::new(\
                     {bus_expr}, \
                     esp_hal::gpio::Output::new(\
                         registry.peripherals.GPIO{cs_pin}, \
                         esp_hal::gpio::Level::Low, \
                         esp_hal::gpio::OutputConfig::default()\
                     ), \
                     ctx.delay \
                 )"
            ),
            None => bus_expr,
        };
        Construction::for_instance(inst, expr)
    }
}
