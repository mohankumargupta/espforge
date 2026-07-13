//! Wokwi simulation assets: `diagram.json` (with `$yaml_ref` tokens resolved
//! from the project YAML / IR) and a generated `wokwi.toml` pointing
//! `elf`/`firmware` at the compiled binary.
//!
//! The `diagram.json` template (copied verbatim by `setup` into the project
//! root) may use a small, YAML-tied token scheme so the wiring is obvious:
//!
//! - `$<peripheral>`   — a gpio/i2c/spi peripheral ref from the YAML, e.g.
//!   `$gpio2` resolves to `GPIO18` (the physical pin).
//! - `$<component>.<pin>` — a component instance + its logical pin, e.g.
//!   `$red_led.pin` resolves to the GPIO the `red_led` component drives.
//! - `$board`           — the wokwi board type for the target chip, e.g.
//!   `board-esp32-c3-devkitm-1`.
//! - `refs` (top-level, optional) — named aliases expanded first, e.g.
//!   `"led_pin": "$red_led.pin"` then used as `$led_pin`; board-literal pins
//!   are bound here too, e.g. `"gnd": "board:GND.2"`.
//!
//! `build` expands `refs`, resolves every `$token`, and writes a clean,
//! token-free `diagram.json` into `out` (always overriding). The template is
//! never fed to wokwi, so the extra `refs` key and `$tokens` are harmless.

use anyhow::Result;
use espforge_model::ir::DeviceTree;
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

/// Physical GPIO pin (e.g. `GPIO18`) for a peripheral ref name (`gpio2`) or a
/// `component.pin` expression, by looking the name up in the IR.
fn resolve_token(ir: &DeviceTree, token: &str) -> Option<String> {
    let target = ir.meta.target.as_deref().unwrap_or("esp32c3");
    if token == "board" {
        return Some(board_type(target).to_string());
    }
    // `component.<field>` form. The default `.pin` resolves to the bare physical
    // pin number (what wokwi's `board:` expects, e.g. `18`); `.field` yields the
    // esp_hal peripheral name (`GPIO18`) for the rare Rust-context diagram.
    if let Some((inst, field)) = token.split_once('.') {
        if let Some(instance) = ir.instances.iter().find(|i| i.id == inst) {
            if let Some(p) = instance.pins.first() {
                return Some(match field {
                    "field" | "gpio" => format!("GPIO{}", p.number),
                    _ => format!("{}", p.number),
                });
            }
        }
        return None;
    }
    // Bare peripheral ref name, e.g. `gpio2` -> bare physical pin number.
    ir.peripherals
        .iter()
        .find(|p| p.name == token)
        .map(|p| format!("{}", p.number))
}

/// Expand the top-level `refs` map (one level of indirection) and then resolve
/// every `$token` in the template text. Tokens that cannot be resolved are left
/// untouched (visible in the output as a signal something is misnamed).
fn substitute_tokens(ir: &DeviceTree, template: &str) -> String {
    // Read the `refs` map via serde (the template is valid JSON; wokwi ignores
    // unknown top-level keys, so the extra `refs` is harmless there).
    let mut refs: HashMap<String, String> = HashMap::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(template) {
        if let Some(map) = value.get("refs").and_then(|r| r.as_object()) {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    refs.insert(k.clone(), s.to_string());
                }
            }
        }
    }

    // Expand refs first (single pass; ref values should not reference other
    // refs to avoid loops).
    let mut text = template.to_string();
    for (k, v) in &refs {
        text = text.replace(&format!("${k}"), v);
    }
    // Then resolve remaining `$token` occurrences.
    let resolved = resolve_all_tokens(ir, &text);
    // The `refs` map is template-only metadata; strip it so the build copy is a
    // clean wokwi file (wokwi would ignore it anyway, but this keeps the output
    // tidy and token-free).
    strip_refs(&resolved)
}

/// Remove the top-level `"refs": { ... }` object from a JSON document, leaving
/// the rest of the text untouched. Consumes a preceding comma and trims the
/// whitespace run that followed it so the remaining JSON stays valid.
fn strip_refs(text: &str) -> String {
    let open = match text.find("\"refs\"") {
        Some(i) => i,
        None => return text.to_string(),
    };
    // Find the `{` that opens the refs object (after "refs":).
    let brace = match text[open..].find('{') {
        Some(j) => open + j,
        None => return text.to_string(),
    };
    let mut depth = 0i32;
    let mut end = brace;
    for (i, c) in text[brace..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace + i;
                    break;
                }
            }
            _ => {}
        }
    }
    // The segment to drop is `,` (or `, `) before "refs" through the closing
    // `}` of the object. Find the comma preceding "refs".
    let comma = text[..open].rfind(',');
    let drop_start = comma.unwrap_or(open);
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..drop_start]);
    // Skip the whitespace after the dropped segment up to the next non-space.
    let mut rest = text[end + 1..].char_indices();
    let after = rest
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(text[end + 1..].len());
    out.push_str(&text[end + 1 + after..]);
    out
}

/// Replace every `$token` in `text` with its resolved value (or leave it if
/// unresolved). A token is `$` followed by `[A-Za-z0-9_.]+`.
fn resolve_all_tokens(ir: &DeviceTree, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'.')
            {
                j += 1;
            }
            let token = &text[start..j];
            match resolve_token(ir, token) {
                Some(resolved) => out.push_str(&resolved),
                None => {
                    out.push('$');
                    out.push_str(token);
                }
            }
            i = j;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Copy the project's `diagram.json` (if present) into `out`, expanding `refs`
/// and resolving `$yaml_ref` tokens. Always overwrites the build copy (it is
/// generated output).
pub fn resolve_diagram(project_dir: &Path, out: &Path, ir: &DeviceTree) -> Result<()> {
    let src = project_dir.join("diagram.json");
    if !src.exists() {
        return Ok(());
    }
    let template = std::fs::read_to_string(&src)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", src.display()))?;
    let resolved = substitute_tokens(ir, &template);
    std::fs::write(out.join("diagram.json"), resolved)
        .map_err(|e| anyhow::anyhow!("failed to write diagram.json: {e}"))?;
    Ok(())
}

/// Generate `wokwi.toml` in `out` pointing `elf`/`firmware` at the compiled
/// binary (`target/<triple>/<profile>/<name>`), bare binary (no `.elf`). Always
/// overwrites. The binary `name` is the package name from the generated
/// `Cargo.toml` (the real source of truth for what `cargo build` emits), not
/// re-derived from the IR, so the path always matches the file Cargo produces.
pub fn write_wokwi_toml(out: &Path, ir: &DeviceTree, profile: &str) -> Result<()> {
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
