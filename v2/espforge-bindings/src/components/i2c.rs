// `i2c` component driver (ADR-006). Wires an I2C peripheral by value. Lives
// under `components/` alongside the other reusable capability drivers.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct I2cDriver;

pub static I2C: I2cDriver = I2cDriver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &I2C;

impl Driver for I2cDriver {
    fn kind(&self) -> &str {
        "i2c"
    }
    fn tier(&self) -> Tier {
        Tier::Component
    }

    fn type_name(&self) -> &str {
        "I2cBus"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { bus: $i2c_master } -> claims the I2C peripheral by value.
        // Resolve the claimed peripheral to its esp_hal field (ADR-008) and pull
        // the bus's sda/scl/frequency from the IR. The runtime `I2cBus::build`
        // returns `Result` (ConfigError); we `.expect()` in generated `setup`
        // with a component-specific message (design §20.7) and `.into_async()`
        // under embassy (§20.1).
        let field = inst
            .claims
            .first()
            .map(|name| codegen::peripheral_field(&ctx.peripherals, &name.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let (sda, scl, freq) = inst
            .claims
            .first()
            .and_then(|name| ctx.peripherals.iter().find(|p| p.name == name.name))
            .and_then(|p| p.bus.as_ref())
            .and_then(|b| match b {
                espforge_model::ir::BusInit::I2c(i) => Some(i),
                _ => None,
            })
            .map(|i| {
                (
                    i.sda.unwrap_or(0).to_string(),
                    i.scl.unwrap_or(0).to_string(),
                    i.frequency_khz.unwrap_or(100),
                )
            })
            .unwrap_or_else(|| ("0".to_string(), "0".to_string(), 100));
        let build = ctx.backend.i2c_master(&field, &sda, &scl, freq);
        let mut expr = format!("{build}.expect(\"{id}: invalid I2C config (check frequency_kHz)\")", id = inst.id);
        if ctx.is_embassy {
            expr.push_str(".into_async()");
        }
        Construction::for_instance(inst, expr)
    }
}
