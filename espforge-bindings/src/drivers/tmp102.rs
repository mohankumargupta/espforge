use espf::context::{ResolvedDevice, RuntimeContext};

pub fn init(device: &ResolvedDevice, ctx: &RuntimeContext) -> Result<(), String> {
    // Direct integer-offset array lookup. No string searching!
    let component = ctx.active_components[device.component_handle.0 as usize]
        .as_ref()
        .unwrap();

    // println!(
    //     "🚀 [Driver Initialised] Type: TMP102 | ID: '{}' (Handle: {}) | Connected via Component context slot: {} | Physical Address: 0x{:02X}",
    //     device.raw_node.id, device.id_handle.0, device.component_handle.0, component.address
    // );

    Ok(())
}
