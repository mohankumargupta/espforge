//! `led` component driver (ADR-006). Lives under `components/` alongside the
//! other reusable capability drivers.

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

    fn construct(&self, inst: &ResolvedInstance, _ctx: &GenContext) -> Construction {
        // with: { pin: $GPIO18, active_low: false }
        let active_low = inst
            .with
            .get("active_low")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let pin = inst
            .pins
            .first()
            .map(|p| format!("registry.peripherals.GPIO{}", p.number))
            .unwrap_or_else(|| "unreachable!()".into());
        // The driver builds the Output from the moved-in GPIO peripheral.
        Construction {
            field: sanitize(&inst.id),
            expr: format!(
                "espforge_runtime::components::Led::new(\n                    esp_hal::gpio::Output::new({pin}, esp_hal::gpio::Level::Low, esp_hal::gpio::OutputConfig::default()),\n                    {active_low})"
            ),
        }
    }
}

fn sanitize(id: &str) -> String {
    let mut out = String::new();
    for (i, c) in id.chars().enumerate() {
        if c.is_alphanumeric() && (i == 0 && c.is_alphabetic() || i > 0) {
            out.push(c);
        } else if i > 0 {
            out.push('_');
        }
    }
    if out.is_empty() {
        out = "inst".into();
    }
    out
}
