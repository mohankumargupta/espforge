// `http` software-service driver (ADR-012 / ADR-013). Unlike hardware
// components it claims no peripheral and takes no `with:` bus ref — the
// singleton `Stack` is implicit infrastructure built by the emitter from the
// top-level `esp32.wifi` block. It only asserts the cross-cutting flags that
// force Embassy + the network stack, and emits a construction that takes the
// emitter-defined `NET_STACK` global by reference.

use espforge_model::driver::{Construction, Driver, GenContext};
use espforge_model::ir::{ResolvedInstance, Tier};
use espforge_model::value::Diag;

#[derive(Debug)]
pub struct HttpDriver;

pub static HTTP: HttpDriver = HttpDriver;

/// Registry entry for this driver (ADR-006/§9b).
pub const DRIVER: &'static dyn Driver = &HTTP;

impl Driver for HttpDriver {
    fn kind(&self) -> &str {
        "http"
    }
    fn tier(&self) -> Tier {
        Tier::Component
    }

    fn type_name(&self) -> &str {
        "Http"
    }

    fn runtime_features(&self) -> Vec<String> {
        vec!["http".to_string()]
    }

    fn flags(&self) -> espforge_model::driver::DriverFlags {
        espforge_model::driver::DriverFlags {
            is_embassy: true,
            has_wifi: true,
            needs_stack: true,
            has_alloc: true,
            needs_delay: false,
        }
    }

    fn generate(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Result<Vec<espforge_model::value::Artifact>, Diag> {
        Ok(vec![])
    }

    fn construct(&self, inst: &ResolvedInstance, _ctx: &GenContext) -> Construction {
        // The emitter builds `NET_STACK` (a `&'static embassy_net::Stack`) in
        // `main.rs` when `needs_stack` is set; we reference it by that name.
        let expr = "espforge_runtime::components::Http::new(NET_STACK)".to_string();
        Construction::for_instance(inst, expr)
    }
}
