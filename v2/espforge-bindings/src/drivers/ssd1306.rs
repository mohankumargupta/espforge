//! `ssd1306` device driver (ADR-006). A terminal device that shares an `i2c`
//! component by reference and claims reset/DC pins by value (ADR-003/008).

use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::ResolvedInstance;
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct Ssd1306Driver;

pub static SSD1306: Ssd1306Driver = Ssd1306Driver;

impl Driver for Ssd1306Driver {
    fn kind(&self) -> &str {
        "ssd1306"
    }
    fn tier(&self) -> espforge_model::ir::Tier {
        espforge_model::ir::Tier::Device
    }

    fn type_name(&self) -> &str {
        "Ssd1306"
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, _ctx: &GenContext) -> Construction {
        // with: { bus: $i2c_bus, reset: $GPIO16, dc: $GPIO5 }
        // `bus` is a shared component -> borrow its field by reference; reset/dc
        // are control pins moved in by value (wrapped as Output by the driver).
        let bus_field = inst
            .deps
            .iter()
            .find(|d| d.kind == espforge_model::ir::DepKind::Instance)
            .map(|d| sanitize(&d.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let reset = pin_field(inst, "reset");
        let dc = pin_field(inst, "dc");
        let out = |gpio: String| {
            format!(
                "esp_hal::gpio::Output::new(registry.peripherals.{gpio}, esp_hal::gpio::Level::Low, esp_hal::gpio::OutputConfig::default())"
            )
        };
        Construction {
            field: sanitize(&inst.id),
            expr: format!(
                "espforge_runtime::devices::Ssd1306::new(components.{bus_field}.bus(),\n                    {},\n                    {})",
                out(reset),
                out(dc)
            ),
        }
    }
}

/// Find a claimed pin's esp_hal field by its `with` key (e.g. "reset").
fn pin_field(inst: &ResolvedInstance, key: &str) -> String {
    // The catalog's `pins` list names the `with` key that holds a pin ref.
    // The IR resolved the pin into `inst.pins` in declaration order matching the
    // spec's `pins` list. We instead re-derive from the `with` value directly.
    let num = inst
        .with
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().strip_prefix("$GPIO"))
        .and_then(|n| n.parse::<u32>().ok())
        .or_else(|| {
            inst.with.get(key).and_then(|v| v.as_u64()).map(|n| n as u32)
        });
    match num {
        Some(n) => format!("GPIO{n}"),
        None => "unreachable!()".to_string(),
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
