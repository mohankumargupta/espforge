use espf::context::{ResolvedDevice, RuntimeContext};

pub fn init(device: &ResolvedDevice, _ctx: &RuntimeContext) -> Result<(), String> {
    // println!(
    //     "📺 [Driver Initialised] Type: ILI9341 Display | ID: '{}' | Resolved Binding Internal Handle: {}",
    //     device.raw_node.id, device.id_handle.0
    // );
    Ok(())
}
