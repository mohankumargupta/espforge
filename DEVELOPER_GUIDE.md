# Espforge Developer Guide

A practical reference for contributors and developers unfamiliar with the codebase. Covers the
architecture, key concepts, and step-by-step walkthroughs for common tasks like adding new devices
and components.

---

## Table of Contents

1. [Codebase Orientation](#1-codebase-orientation)
2. [The std vs no_std Boundary](#2-the-std-vs-no_std-boundary)
3. [Key Concepts](#3-key-concepts)
4. [Adding a New Device Plugin](#4-adding-a-new-device-plugin)
5. [Adding a New Component Plugin](#5-adding-a-new-component-plugin)
6. [The Plugin System Explained](#6-the-plugin-system-explained)
7. [YAML Configuration and Code Generation Flow](#7-yaml-configuration-and-code-generation-flow)
8. [Common Pitfalls](#8-common-pitfalls)

---

## 1. Codebase Orientation

The workspace is split into crates with distinct responsibilities. Understanding which layer a crate
belongs to is the most important thing to grasp first.

```
espforge_configuration    — Shared data model and plugin trait definitions
espforge_macros           — Procedural macros (ComponentPlugin / DevicePlugin derive)
espforge_components_builder — Component plugin implementations (Button, LED, SPI, I2C, UART, HTTP)
espforge_devices_builder  — Device plugin implementations (SSD1306, ILI9341, FT6206)
espforge_codegen          — Orchestrates code generation from the parsed config model
espforge                  — CLI binary (compile, examples, doctor commands)
espforge_platform         — Runtime HAL abstractions compiled for the ESP32 target
espforge_components       — Runtime component implementations compiled for the ESP32 target
espforge_devices          — Runtime device implementations compiled for the ESP32 target
espforge_common           — Shared config struct types used by both host and target crates
espforge_esp32metadata    — Static chip and board metadata (wokwi board IDs, heap sizes, etc.)
espforge_examples         — Embedded example project files (include_dir)
```

A quick mental model: everything ending in `_builder` runs on your **development machine** and
generates Rust code. Everything in `espforge_platform`, `espforge_components`, and
`espforge_devices` runs on the **ESP32 chip**.

---

## 2. The std vs no_std Boundary

This is the most important rule in the codebase and the most common source of confusion.

### Host-side crates (full std available)

These crates only run during the build/codegen phase on your development machine:

- `espforge_configuration`
- `espforge_codegen`
- `espforge_components_builder`
- `espforge_devices_builder`
- `espforge_macros`
- `espforge`

You can freely use `std::collections::HashMap`, `std::string::String`, `anyhow`, `proc_macro2`,
and `std::fmt::Display` here.

### Target-side crates (no_std)

These crates are compiled for the ESP32 and must not use `std`:

- `espforge_platform`
- `espforge_components`
- `espforge_devices`

**Rules for target-side crates:**

| Need | Use instead of |
|------|---------------|
| `std::fmt::Display` / `std::fmt::Formatter` | `core::fmt::Display` / `core::fmt::Formatter` |
| `std::string::String` | `alloc::string::String` (requires `extern crate alloc`) |
| `std::vec::Vec` | `alloc::vec::Vec` |
| `std::collections::HashMap` | Not available without `std`; use `heapless::FnvIndexMap` or restructure |

`core` is always available in no_std. `alloc` is available when a global allocator is configured
(which espforge projects do via `esp-alloc`).

### The fmt shortcut

A pattern you will see throughout target-side crates:

```rust
use core::fmt;

impl fmt::Display for MyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "...")
    }
}
```

`core::fmt` and `std::fmt` expose identical APIs — the `write!` macro works the same in both.

---

## 3. Key Concepts

### DeviceRef\<T\>

Defined in `espforge_configuration/src/refs.rs`. A typed wrapper around a YAML reference string
such as `$spi2` or `$pin_dc`.

```rust
pub struct DeviceRef<T> {
    raw: String,   // already stripped of the leading '$'
    _kind: PhantomData<T>,
}
```

The phantom type parameter carries the *kind* of resource being referenced:

- `DeviceRef<ComponentRef>` — a reference to a named component (e.g. `$main_spi`)
- `DeviceRef<PinRef>` — a reference to a named GPIO pin (e.g. `$pin_dc`)

**Normalization happens at deserialization.** When serde deserializes a `DeviceRef<T>` from YAML
the leading `$` is stripped automatically. You never need to call `.strip_prefix('$')` anywhere.

**Usage in a config struct:**

```rust
#[derive(Deserialize)]
pub struct MyDeviceConfig {
    pub bus:  DeviceRef<ComponentRef>,   // must reference a component
    pub dc:   DeviceRef<PinRef>,         // must reference a GPIO pin
}
```

### DependencyKind

Describes what kind of hardware resource a dependency refers to:

```rust
pub enum DependencyKind {
    Component,   // a named component (SpiDevice, I2cDevice, etc.)
    Device,      // a named device (ssd1306, ili9341, etc.)
    Peripheral,  // a raw hardware peripheral (spi2, i2c0, uart0, etc.)
    Pin,         // a GPIO pin
}
```

### GenerationContext

Passed to `Plugin::generate_code`. Contains everything a plugin needs to emit Rust tokens:

```rust
pub struct GenerationContext<'a> {
    pub instance_name: &'a str,               // e.g. "display"
    pub properties: &'a serde_yaml_ng::Value, // raw YAML for this device/component
    pub model: &'a EspforgeConfiguration,     // the full parsed config
    pub resolved_deps: &'a HashMap<String, ResolvedDependency>,
}
```

The two most useful methods on `GenerationContext`:

```rust
// Validate kind and retrieve a resolved dependency by name
ctx.dependency("spi_name", DependencyKind::Component)?;

// Validate kind AND parse the access path as a TokenStream in one call
let spi_tokens = ctx.dependency_access("spi_name", DependencyKind::Component)?;
```

### The Plugin Trait

```rust
pub trait Plugin: Sync + Send {
    fn name(&self) -> &'static str;
    fn kind(&self) -> PluginKind;                                        // Component or Device
    fn validate(&self, properties: &Value) -> Result<()> { Ok(()) }
    fn dependencies(&self, properties: &Value) -> Result<Vec<Dependency>> { Ok(vec![]) }
    fn required_features(&self) -> Vec<String> { vec![] }
    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode>;
}
```

Plugins are registered globally via the `inventory` crate. The `#[derive(DevicePlugin)]` and
`#[derive(ComponentPlugin)]` macros handle registration automatically.

---

## 4. Adding a New Device Plugin

A "device" in espforge sits on top of a component (e.g. an SSD1306 display sits on top of an
I2C component). Here is every file you need to touch.

### Step 1 — Add the runtime device implementation

Location: `espforge_devices/src/devices/<your_device>/`

Create `mod.rs` and `device.rs`. The device struct takes ownership of whatever the component
exposes (an I2C bus, SPI bus, GPIO pins, etc.) and wraps a driver crate.

```
espforge_devices/src/devices/
  my_device/
    mod.rs      ← pub mod device;
    device.rs   ← your MyDevice<I> struct and impl
```

Register it in `espforge_devices/src/devices/mod.rs`:

```rust
pub mod my_device;
```

If your device needs a new Cargo dependency (e.g. a driver crate), add it to
`espforge_devices/Cargo.toml` as an optional dependency and create a matching feature flag:

```toml
[dependencies]
my-driver = { version = "x.y", optional = true }

[features]
my_device = ["dep:my-driver"]
```

### Step 2 — Add the builder plugin

Location: `espforge_devices_builder/src/my_device.rs`

This is the host-side plugin that teaches espforge how to parse YAML config for your device,
declare its dependencies, and generate the Rust code that wires it up at runtime.

```rust
use anyhow::{Context, Result};
use espforge_configuration::plugin::{
    ComponentRef, Dependency, DependencyKind, DeviceRef, GeneratedCode, GenerationContext, PinRef,
};
use espforge_macros::DevicePlugin;
use quote::{format_ident, quote};
use serde::Deserialize;

// 1. Config struct — field types enforce what each YAML value must reference
#[derive(Deserialize, Debug, Clone)]
pub struct MyDeviceConfig {
    pub bus: DeviceRef<ComponentRef>,   // e.g. $my_i2c
    pub rst: DeviceRef<PinRef>,         // e.g. $pin_rst
}

fn parse_config(value: &serde_yaml_ng::Value) -> Result<MyDeviceConfig> {
    serde_yaml_ng::from_value(value.clone()).context("Invalid MyDevice configuration")
}

// 2. Plugin struct — the derive macro handles name(), kind(), and inventory registration
#[derive(DevicePlugin)]
#[plugin(name = "my_device", features = "my_device")]
pub struct MyDevicePlugin;

impl MyDevicePlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        parse_config(properties)?;
        Ok(())
    }

    fn resolve_dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config = parse_config(properties)?;
        Ok(vec![
            Dependency::component_ref(&config.bus),
            Dependency::pin_ref(&config.rst),
        ])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config = parse_config(ctx.properties).context("Failed to parse MyDevice properties")?;
        let field_ident = format_ident!("{}", ctx.instance_name);

        let bus_access = ctx.dependency_access(config.bus.as_str(), DependencyKind::Component)?;
        let rst_access = ctx.dependency_access(config.rst.as_str(), DependencyKind::Pin)?;

        Ok(GeneratedCode {
            field: quote! {
                pub #field_ident: espforge_devices::devices::my_device::device::MyDevice
            },
            init: quote! {
                let #field_ident = espforge_devices::devices::my_device::device::MyDevice::new(
                    #bus_access,
                    #rst_access,
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}
```

Register the new plugin in `espforge_devices_builder/src/lib.rs`:

```rust
pub mod my_device;

pub fn init() {
    // existing inits...
}
```

### Step 3 — Add a YAML example

Add an entry to an existing or new example YAML to verify the round-trip:

```yaml
devices:
  my_display:
    using: my_device
    with:
      bus: $i2c_master
      rst: $pin_rst
```

### Summary checklist

- [ ] `espforge_devices/src/devices/my_device/device.rs` — runtime struct
- [ ] `espforge_devices/src/devices/my_device/mod.rs` — `pub mod device;`
- [ ] `espforge_devices/src/devices/mod.rs` — `pub mod my_device;`
- [ ] `espforge_devices/Cargo.toml` — optional dep + feature flag
- [ ] `espforge_devices_builder/src/my_device.rs` — config struct + plugin
- [ ] `espforge_devices_builder/src/lib.rs` — `pub mod my_device;`

---

## 5. Adding a New Component Plugin

A "component" wraps a raw hardware peripheral (GPIO, SPI bus, I2C bus, UART) into a reusable
abstraction that devices can reference.

The process is nearly identical to adding a device, with two differences:

- Use `#[derive(ComponentPlugin)]` instead of `#[derive(DevicePlugin)]`
- The runtime implementation lives in `espforge_components/src/components/`
- The builder lives in `espforge_components_builder/src/`

### Summary checklist

- [ ] `espforge_components/src/components/my_component/mod.rs` — runtime struct
- [ ] `espforge_components/src/components/mod.rs` — `pub mod my_component;`
- [ ] `espforge_components/Cargo.toml` — optional dep + feature flag if needed
- [ ] `espforge_components_builder/src/my_component.rs` — config struct + plugin
- [ ] `espforge_components_builder/src/lib.rs` — `pub mod my_component;`

---

## 6. The Plugin System Explained

Plugins are discovered at runtime via the [`inventory`](https://crates.io/crates/inventory) crate.
This means there is no central registry you need to edit — registration is a side effect of
linking the plugin crate.

The `#[derive(DevicePlugin)]` macro expands to something like:

```rust
inventory::submit! {
    PluginRegistration(&MyDevicePlugin)
}
```

The `plugin(name = "...")` attribute controls the string that appears in YAML under `using:`.
If omitted, the derive macro infers the name by stripping `Plugin` from the struct name and
lowercasing the result (e.g. `ILI9341Plugin` → `ili9341`).

The `plugin(features = "...")` attribute lists the Cargo feature flags (comma-separated) that
will be added to `espforge_devices` or `espforge_components` in the generated `Cargo.toml`.

---

## 7. YAML Configuration and Code Generation Flow

When you run `espforge compile my_project.yaml`, here is what happens in order:

```
1. CLI (espforge/src/cli/commands/compile.rs)
        ↓
2. ConfigParser (espforge/src/parse/)
   Sections processed in priority order:
     ProjectInfoProvisioner  → reads [espforge] section
     PlatformProvisioner     → reads [esp32] section, validates pin conflicts
     ComponentProvisioner    → reads [components], validates against esp32 hardware
     DeviceProvisioner       → reads [devices]
        ↓
3. EspforgeConfiguration (in-memory model)
        ↓
4. Code generation (espforge_codegen)
   - DependencyResolver      → topological sort of component/device graph
   - CodegenContext           → drives token generation in dependency order
   - Plugin::generate_code() → called per component and device instance
        ↓
5. File output
   - src/generated.rs        → PeripheralRegistry, Components, Devices structs
   - src/lib.rs              → module declarations + helper macros
   - src/bin/main.rs         → entry point (blocking or embassy)
   - Cargo.toml              → dependencies + feature flags merged in
   - diagram.json            → wokwi board placeholder replaced
```

The generated `PeripheralRegistry` owns all hardware peripherals. `Components` takes a
`&'static mut PeripheralRegistry` and consumes peripherals out of it. `Devices` takes a
`&'static mut Components` reference and wires up higher-level device abstractions.

---

## 8. Common Pitfalls

### Forgetting to strip the `$` prefix (old code)

Any code predating the `DeviceRef<T>` refactor may still use raw `String` fields and manual
`.strip_prefix('$').unwrap_or(...)` calls. When touching these files, migrate them to
`DeviceRef<ComponentRef>` or `DeviceRef<PinRef>` as appropriate — normalization then happens
automatically at deserialization.

### Using `std` types in target-side crates

If you see a compile error like `error[E0433]: failed to resolve: use of undeclared crate or
module std` on a target build, the cause is almost always a stray `std::` import in
`espforge_platform`, `espforge_components`, or `espforge_devices`. Replace with `core::` (or
`alloc::` for heap types).

### Plugin name mismatch

If `espforge compile` reports `Unknown device driver: my_device`, check that:

1. The `#[plugin(name = "my_device")]` attribute matches the `using:` value in YAML exactly.
2. The builder crate is linked — verify `pub mod my_device;` exists in `lib.rs`.

### Dependency kind mismatch

`GenerationContext::dependency_access` validates that the resolved dependency has the expected
`DependencyKind`. If you pass `DependencyKind::Component` for a field that references a pin, you
will get a runtime error during compilation. Use `DeviceRef<ComponentRef>` vs `DeviceRef<PinRef>`
in your config struct to make this a compile-time error instead.

### Feature flags not propagated

If the generated project fails to compile with a missing trait impl or missing type, check that:

1. Your plugin's `#[plugin(features = "...")]` lists the correct feature name.
2. The feature is declared in `espforge_devices/Cargo.toml` or `espforge_components/Cargo.toml`.
3. The feature enables the right optional dependency.

### Pin conflicts

`PlatformProvisioner` validates that no two hardware resources share the same GPIO pin. If you
see `Pin conflicts detected` during `espforge compile`, two entries in the `esp32:` section of
your YAML are using the same physical pin number.

