//! `led` component driver (ADR-006). Lives under `components/` alongside the
//! other reusable capability drivers.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

/// A `Led` instance. `active_low` comes from `with.active_low` (default false).
#[derive(Debug)]
pub struct LedDriver;

pub static LED: LedDriver = LedDriver;

impl Driver for LedDriver {
    fn kind(&self) -> &str {
        "led"
    }
    fn tier(&self) -> Tier {
        Tier::Component
    }

    fn type_name(&self) -> &str {
        "Led"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { pin: $GPIO18, active_low: false }
        let active_low = codegen::bool_or(inst, "active_low", false);
        let pin = inst
            .pins
            .first()
            .map(|p| p.number.to_string())
            .unwrap_or_else(|| "unreachable!()".into());
        // The backend builds the polarity-aware Output from the moved-in GPIO
        // peripheral; we hand it the bare GPIO field number.
        let output = ctx.backend.gpio_output(&pin, active_low);
        Construction::for_instance(
            inst,
            ctx.backend.ctor(Tier::Component, "Led", &[output, active_low.to_string()]),
        )
    }
}
