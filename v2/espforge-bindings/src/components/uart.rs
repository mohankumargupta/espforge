// `uart` component driver (ADR-006). Wires a UART peripheral by value. Lives
// under `components/` alongside the other reusable capability drivers.

use espforge_model::codegen;
use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{ResolvedInstance, Tier};
use espforge_model::value::{Artifact, Diag};

#[derive(Debug)]
pub struct UartDriver;

pub static UART: UartDriver = UartDriver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &UART;

impl Driver for UartDriver {
    fn kind(&self) -> &str {
        "uart"
    }
    fn tier(&self) -> Tier {
        Tier::Component
    }

    fn type_name(&self) -> &str {
        "UartDevice"
    }

    fn type_name_for(&self, _inst: &ResolvedInstance, ctx: &GenContext) -> String {
        let dm = if ctx.is_embassy {
            "esp_hal::Async"
        } else {
            "esp_hal::Blocking"
        };
        format!("UartDevice<{dm}>")
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Construction {
        // with: { bus: $uart1 } -> claims the UART peripheral by value. Resolve
        // the claimed peripheral to its esp_hal field and pull tx/rx/baudrate
        // from the IR (model refactor C: typed UartInit). The YAML key is
        // `baudrate` (design §20.6); the IR `UartInit.baud` carries it.
        // `UartDevice::build` is fallible (ConfigError) -> `.expect()` here
        // (§20.7); `.into_async()` under embassy (§20.1).
        let field = inst
            .claims
            .first()
            .map(|name| codegen::peripheral_field(&ctx.peripherals, &name.name))
            .unwrap_or_else(|| "unreachable!()".to_string());
        let (tx, rx, baud) = inst
            .claims
            .first()
            .and_then(|name| ctx.peripherals.iter().find(|p| p.name == name.name))
            .and_then(|p| p.bus.as_ref())
            .and_then(|b| match b {
                espforge_model::ir::BusInit::Uart(u) => Some(u),
                _ => None,
            })
            .map(|u| {
                (
                    u.tx.unwrap_or(0).to_string(),
                    u.rx.unwrap_or(0).to_string(),
                    u.baud.unwrap_or(115_200).to_string(),
                )
            })
            .unwrap_or_else(|| {
                (
                    "0".to_string(),
                    "0".to_string(),
                    "115200".to_string(),
                )
            });
        let mut expr = format!(
            "{build}.expect(\"{id}: invalid UART config (check baudrate)\")",
            id = inst.id,
            build = ctx.backend.uart(&field, &tx, &rx, baud.parse().unwrap_or(115_200)),
        );
        if ctx.is_embassy {
            expr.push_str(".into_async()");
        }
        Construction::for_instance(inst, expr)
    }
}
