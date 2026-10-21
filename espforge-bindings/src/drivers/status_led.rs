use espf::context::{ActivePeripheral, ResolvedDevice, RuntimeContext};

pub fn init(device: &ResolvedDevice, ctx: &RuntimeContext) -> Result<(), String> {
    // Direct integer-offset array lookup. No string searching!
    let component = ctx.active_components[device.component_handle.0 as usize]
        .as_ref()
        .ok_or_else(|| {
            format!(
                "❌ [status_led] Device '{}' bound to an unresolved component slot",
                device.raw_node.id
            )
        })?;

    let peripheral = ctx.active_peripherals[component.parent_peripheral.0 as usize]
        .as_ref()
        .ok_or_else(|| {
            format!(
                "❌ [status_led] Device '{}' component has no active parent peripheral",
                device.raw_node.id
            )
        })?;

    if !matches!(peripheral, ActivePeripheral::Gpio) {
        return Err(format!(
            "❌ [status_led] Device '{}' expects a GPIO component, got a non-GPIO parent bus",
            device.raw_node.id
        ));
    }

    // println!(
    //     "🚀 [Driver Initialised] Type: Status LED | ID: '{}' (Handle: {}) | Bound to GPIO component slot: {}",
    //     device.raw_node.id, device.id_handle.0, device.component_handle.0
    // );
    //
    Ok(())
}
