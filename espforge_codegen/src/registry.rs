use espforge_configuration::plugin::{Plugin, PluginKind, PluginRegistration};

pub fn find_plugin(name: &str) -> Option<&'static dyn Plugin> {
    inventory::iter::<PluginRegistration>
        .into_iter()
        .find(|reg| reg.0.name() == name)
        .map(|reg| reg.0)
}

pub fn find_component_plugin(name: &str) -> Option<&'static dyn Plugin> {
    find_plugin(name).filter(|p| p.kind() == PluginKind::Component)
}

pub fn find_device_plugin(name: &str) -> Option<&'static dyn Plugin> {
    find_plugin(name).filter(|p| p.kind() == PluginKind::Device)
}

pub fn all_plugins() -> impl Iterator<Item = &'static dyn Plugin> {
    inventory::iter::<PluginRegistration>
        .into_iter()
        .map(|reg| reg.0)
}
