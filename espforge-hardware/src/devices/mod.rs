//! `# devices` lowering.
//!
//! Nothing here is buildable yet: any `# devices` entry is refused with a
//! clear "not supported yet". This module owns that guard so `lib.rs`
//! stays a thin orchestrator; when the first device lands, its draft and
//! `lower_*`/`attach_*` halves go here following the
//! peripherals/components split.

use std::collections::HashMap;

use espf::context::ParsedNode;
use espf::diag::EspfError;

/// Reject any `# devices` entry: `build` cannot generate code for devices
/// yet (only GPIO/LED/button, I2C/SPI/UART bus components, and `<wifi>`).
pub fn reject_if_present(
    sections: &HashMap<String, Vec<ParsedNode>>,
) -> Result<(), Vec<EspfError>> {
    if sections
        .get("devices")
        .is_some_and(|v| !v.is_empty())
    {
        return Err(vec![EspfError::at_span(
            0..0,
            "unsupported devices",
            "`# devices` is not supported by `build` yet (only <gpio> peripherals with <led> and <button> components, <i2c> peripherals with <i2c_component> components, <spi> peripherals with <spi_component> components, and the <wifi> service)",
        )]);
    }
    Ok(())
}
