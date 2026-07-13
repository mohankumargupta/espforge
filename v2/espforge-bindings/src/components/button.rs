// `button` component driver (ADR-006). Lives under `components/` alongside the
// other reusable capability drivers.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

/// A `Button` instance. `pull_up` comes from `with.pull_up` (default false).
#[derive(Debug)]
pub struct ButtonDriver;

pub static BUTTON: ButtonDriver = ButtonDriver;

/// Registry entry for this driver (ADR-006/§9b). The build script references
/// `button::DRIVER` so it never has to guess the per-driver const name.
pub const DRIVER: &'static dyn Driver = &BUTTON;

impl Driver for ButtonDriver {
    fn kind(&self) -> &str {
        "button"
    }
    fn tier(&self) -> Tier {
        Tier::Component
    }

    fn type_name(&self) -> &str {
        "Button"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { pin: $GPIO9, pull_up: true }
        let pull_up = codegen::bool_or(inst, "pull_up", false);
        let pin = inst
            .pins
            .first()
            .map(|p| format!("GPIO{}", p.number))
            .unwrap_or_else(|| "unreachable!()".into());
        // The backend builds the input from the moved-in GPIO peripheral; the
        // field name is the esp_hal `GPIO{n}` member.
        let input = ctx.backend.gpio_input(&pin, pull_up);
        Construction::for_instance(
            inst,
            ctx.backend.ctor(Tier::Component, "Button", &[input, pull_up.to_string()]),
        )
    }
}
