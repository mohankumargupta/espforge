//! `i2c` component driver (ADR-006). Wires an I2C peripheral by value.

use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{Peripheral, ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct I2cDriver;

pub static I2C: I2cDriver = I2cDriver;

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
        // Resolve the claimed peripheral to its esp_hal field (ADR-008).
        let field = inst
            .claims
            .first()
            .map(|name| peripheral_field(&ctx.peripherals, &name.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        Construction {
            field: sanitize(&inst.id),
            expr: format!("espforge_runtime::components::I2cBus::new(registry.peripherals.{field})"),
        }
    }
}

fn peripheral_field(peripherals: &[Peripheral], name: &str) -> String {
    peripherals
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.field.clone())
        .unwrap_or_else(|| "unreachable!()".to_string())
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
