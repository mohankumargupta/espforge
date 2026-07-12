//! Wokwi simulation assets: `diagram.json` (with placeholders resolved from the
//! IR) and a generated `wokwi.toml` pointing `elf`/`firmware` at the compiled
//! binary. See the grilling notes — `setup` copies the example's assets
//! verbatim into the project root; `build` resolves placeholders and writes
//! fresh copies into `out` (always overriding, Grill 3).

use anyhow::Result;
use espforge_model::ir::DeviceTree;
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

/// The physical GPIO pin (e.g. `GPIO18`) driven by the first `led` instance,
/// used to resolve the `PLACEHOLDER_GPIO` token in the diagram. We read the
/// instance's own pin number (the physical board pin wokwi expects) rather than
/// the esp_hal peripheral `field`, which is also the physical pin but reached
/// indirectly. Falls back to the first LED-less resolved pin when no led.
fn led_gpio_field(ir: &DeviceTree) -> Option<String> {
    ir.instances
        .iter()
        .find(|i| i.kind == "led")
        .and_then(|i| i.pins.first())
        .or_else(|| {
            ir.instances
                .iter()
                .find(|i| !i.pins.is_empty())
                .and_then(|i| i.pins.first())
        })
        .map(|p| format!("GPIO{}", p.number))
}

/// Resolve the placeholder tokens in a `diagram.json` template against the IR.
/// Tokens: `PLACEHOLDER_WOKWI_BOARD`, `PLACEHOLDER_GPIO`,
/// `PLACEHOLDER_GND_BOTTOM_RIGHT`.
fn resolve_diagram_tokens(ir: &DeviceTree, template: &str) -> String {
    let target = ir.meta.target.as_deref().unwrap_or("esp32c3");
    let board = board_type(target);
    let gpio = led_gpio_field(ir).unwrap_or_else(|| "GPIO2".to_string());
    template
        .replace("PLACEHOLDER_WOKWI_BOARD", board)
        .replace("PLACEHOLDER_GPIO", &gpio)
        .replace("PLACEHOLDER_GND_BOTTOM_RIGHT", "GND.2")
}

/// Copy the project's `diagram.json` (if present) into `out`, resolving
/// placeholders. Always overwrites the build copy (it is generated output).
pub fn resolve_diagram(project_dir: &Path, out: &Path, ir: &DeviceTree) -> Result<()> {
    let src = project_dir.join("diagram.json");
    if !src.exists() {
        return Ok(());
    }
    let template = std::fs::read_to_string(&src)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", src.display()))?;
    let resolved = resolve_diagram_tokens(ir, &template);
    std::fs::write(out.join("diagram.json"), resolved)
        .map_err(|e| anyhow::anyhow!("failed to write diagram.json: {e}"))?;
    Ok(())
}

/// Generate `wokwi.toml` in `out` pointing `elf`/`firmware` at the compiled
/// binary (`target/<triple>/<profile>/<name>`), bare binary (no `.elf`,
/// Grill 2). Always overwrites (Grill 3).
pub fn write_wokwi_toml(out: &Path, ir: &DeviceTree, profile: &str) -> Result<()> {
    let target = ir.meta.target.as_deref().unwrap_or("esp32c3");
    let triple = target_triple(target);
    let name = ir
        .meta
        .name
        .clone()
        .unwrap_or_else(|| "espforge_project".into())
        .replace('-', "_");
    let bin = format!("target/{triple}/{profile}/{name}");
    let content = format!(
        "[wokwi]\n\
         version = 1\n\
         gdbServerPort = 3333\n\
         elf = \"{bin}\"\n\
         firmware = \"{bin}\"\n"
    );
    std::fs::write(out.join("wokwi.toml"), content)
        .map_err(|e| anyhow::anyhow!("failed to write wokwi.toml: {e}"))?;
    Ok(())
}
