//! Wokwi simulation assets: `diagram.json` (with `{{ variable }}` tokens resolved
//! from the project YAML / IR) and a generated `wokwi.toml` pointing
//! `elf`/`firmware` at the compiled binary.
//!
//! The `diagram.json` template (copied verbatim by `setup` into the project
//! root) uses Jinja2-style templates so the wiring is obvious:
//!
//! - `{{ board }}`           — the wokwi board type for the target chip, e.g.
//!   `board-esp32-c3-devkitm-1`.
//! - `{{ gpioN }}`           — a peripheral ref from the YAML, e.g.
//!   `{{ gpio2 }}` resolves to `GPIO18` (the physical pin).
//! - `{{ component.pin }}`   — a component instance + its logical pin, e.g.
//!   `{{ red_led.pin }}` resolves to the GPIO the `red_led` component drives.
//! - `{{ serialMonitor }}`   — the wokwi serial monitor part ID (`serialMonitor`).
//!
//! `build` resolves every `{{ token }}` and writes a clean,
//! token-free `diagram.json` into `out` (always overriding).

use anyhow::Result;
use espforge_model::ir::DeviceTree;
use minijinja::Environment;
use std::collections::HashMap;
use std::path::Path;

/// Wokwi board type per esp32 target chip.
fn board_type(target: &str) -> &'static str {
    match target {
        "esp32" => "board-esp32-devkitc-v4",
        "esp32s2" => "board-esp32-s2-devkitc-1",
        "esp32s3" => "board-esp32-s3-devkitc-1",
        "esp32c2" => "board-esp32-c2-devkitm-1",
        "esp32c3" => "board-esp32-c3-devkitm-1",
        "esp32c6" => "board-esp32-c6-devkitc-1",
        "esp32h2" => "board-esp32-h2-devkitm-1",
        _ => "board-esp32-c3-devkitm-1",
    }
}

/// Cargo target triple per esp32 target chip (matches what `cargo build`
/// produces under the esp toolchains).
fn target_triple(target: &str) -> &'static str {
    match target {
        "esp32" => "xtensa-esp32-none-elf",
        "esp32s2" => "xtensa-esp32s2-none-elf",
        "esp32s3" => "xtensa-esp32s3-none-elf",
        "esp32c2" => "riscv32imc-unknown-none-elf",
        "esp32c3" => "riscv32imc-unknown-none-elf",
        "esp32c6" => "riscv32imac-unknown-none-elf",
        "esp32h2" => "riscv32imac-unknown-none-elf",
        _ => "riscv32imc-unknown-none-elf",
    }
}

/// Build a template context from the DeviceTree containing all resolvable values.
///
/// Component instance fields (`.pin`/`.field`/`.gpio`) are inserted as a real
/// nested object per instance, not a flat `"id.field"` string key: Jinja-style
/// `{{ red_led.pin }}` resolves `red_led` as a variable and then does
/// attribute access `.pin` on it — it does NOT look up the literal string
/// `"red_led.pin"`. A flat map has no `red_led` key at all, so `red_led` was
/// `Undefined` and `.pin` on `Undefined` is a hard render error under
/// minijinja's default (non-lenient) undefined behavior.
fn build_context(ir: &DeviceTree) -> minijinja::Value {
    let target = ir.meta.target.as_deref().unwrap_or("esp32c3");
    let mut ctx: HashMap<String, minijinja::Value> = HashMap::new();

    // Board type
    ctx.insert(
        "board".to_string(),
        minijinja::Value::from(board_type(target).to_string()),
    );

    // Serial monitor (wokwi built-in)
    ctx.insert(
        "serialMonitor".to_string(),
        minijinja::Value::from("serialMonitor".to_string()),
    );

    // Peripheral references: $gpioN -> pin number
    for p in &ir.peripherals {
        ctx.insert(p.name.clone(), minijinja::Value::from(p.number.to_string()));
    }

    // Component instances: {{ component.pin }} / .field / .gpio -> a nested
    // object per instance, so template attribute access resolves correctly.
    for inst in &ir.instances {
        if let Some(pin) = inst.pins.first() {
            let pin_num = pin.number.to_string();
            let gpio_name = format!("GPIO{}", pin.number);
            let mut fields: HashMap<String, String> = HashMap::new();
            // Default .pin resolves to bare pin number (what wokwi expects)
            fields.insert("pin".to_string(), pin_num);
            // .field / .gpio yields the esp_hal peripheral name (GPIO18)
            fields.insert("field".to_string(), gpio_name.clone());
            fields.insert("gpio".to_string(), gpio_name);
            ctx.insert(inst.id.clone(), minijinja::Value::from(fields));
        }
    }

    minijinja::Value::from(ctx)
}

/// Render a diagram.json template using minijinja.
fn render_diagram(template: &str, ctx: &minijinja::Value) -> Result<String> {
    let env = Environment::new();
    let template = env
        .template_from_str(template)
        .map_err(|e| anyhow::anyhow!("failed to parse template: {e}"))?;
    let rendered = template
        .render(ctx)
        .map_err(|e| anyhow::anyhow!("failed to render template: {e}"))?;
    Ok(rendered)
}

/// Copy the project's `diagram.json` (if present) into `out`, expanding
/// `{{ variable }}` tokens. Always overwrites the build copy (it is
/// generated output).
pub fn resolve_diagram(project_dir: &Path, out: &Path, ir: &DeviceTree) -> Result<()> {
    let src = project_dir.join("diagram.json");
    if !src.exists() {
        return Ok(());
    }
    let template = std::fs::read_to_string(&src)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", src.display()))?;
    let ctx = build_context(ir);
    let resolved = render_diagram(&template, &ctx)?;
    std::fs::write(out.join("diagram.json"), resolved)
        .map_err(|e| anyhow::anyhow!("failed to write diagram.json: {e}"))?;
    Ok(())
}

/// Generate `wokwi.toml` in `out` pointing `elf`/`firmware` at the compiled
/// binary (`target/<triple>/<profile>/<name>`), bare binary (no `.elf`). Always
/// overwrites. The binary `name` is the package name from the generated
/// `Cargo.toml` (the real source of truth for what `cargo build` emits), not
/// re-derived from the IR, so the path always matches the file Cargo produces.
pub fn write_wokwi_toml(
    out: &Path,
    ir: &DeviceTree,
    profile: &str,
    chip_name: Option<&str>,
) -> Result<()> {
    let target = ir.meta.target.as_deref().unwrap_or("esp32c3");
    let triple = target_triple(target);
    let name = package_name(out).unwrap_or_else(|| {
        ir.meta
            .name
            .clone()
            .unwrap_or_else(|| "espforge_project".into())
            .replace('-', "_")
    });
    let bin = format!("target/{triple}/{profile}/{name}");
    let mut content = format!(
        "[wokwi]\n\
         version = 1\n\
         gdbServerPort = 3333\n\
         elf = \"{bin}\"\n\
         firmware = \"{bin}\"\n"
    );
    // A Wokwi custom chip (carried into `out/chip/`) is referenced by a
    // `[[chip]]` section: `name` -> part type `chip-<name>` in diagram.json,
    // `binary` points at the wasm. The JSON pin description must share the
    // wasm's basename, so `chip/chip.wasm` pairs with `chip/chip.json`.
    if let Some(chip) = chip_name {
        content.push_str(&format!(
            "\n[[chip]]\n\
             name = '{chip}'\n\
             binary = 'chip/chip.wasm'\n"
        ));
    }
    std::fs::write(out.join("wokwi.toml"), content)
        .map_err(|e| anyhow::anyhow!("failed to write wokwi.toml: {e}"))?;
    Ok(())
}

/// Read the `[package] name` from the generated `Cargo.toml` in `out`. Returns
/// `None` if the file is missing or has no package name (caller falls back to
/// the IR name). Lightweight line scan — no TOML parser needed.
fn package_name(out: &Path) -> Option<String> {
    let cargo_toml = out.join("Cargo.toml");
    let text = std::fs::read_to_string(&cargo_toml).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && trimmed.starts_with("name") {
            if let Some(eq) = trimmed.find('=') {
                let value = trimmed[eq + 1..].trim().trim_matches('"');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}