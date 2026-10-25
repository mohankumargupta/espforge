//! The project as a whole: layering an example's files over the generated set
//! ([`layer_overlay`], [`OverlayFile`], [`Rendered`]) and the `README.txt`
//! ([`readme`]).
//!
//! The generated files themselves come from [`crate::codegen`]; this module is
//! what happens after them and before the ownership manifest.

use anyhow::{Context as _, Result};
use espforge_model::{Artifact, Asset, Board, Ownership};
use espforge_options::ResolvedOptions;

use crate::codegen;

/// One file of an example, with its path relative to the example directory.
#[derive(Debug, Clone, Copy)]
pub struct OverlayFile<'a> {
    pub path: &'a str,
    pub contents: &'a [u8],
}

/// Everything a render produces: text files and binary assets.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub artifacts: Vec<Artifact>,
    pub assets: Vec<Asset>,
}

/// Layer an example's files over an already-generated project:
///
/// * `diagram.json` / `wokwi.toml` replace the generated ones, with their
///   `{{ board }}`, `{{ serialMonitor }}` and `{{ <component>.pin }}`
///   placeholders resolved by [`codegen::overlay`]. They only apply when the
///   `wokwi` option emitted the generated versions.
/// * `chip.json` and `chip.wasm` are always copied (when `wokwi` is on);
///   the `chip/` source directory only when `keep-zig-source` is on. An
///   example without `chip/` makes that a no-op. These are seed-once, so
///   rebuilding a chip locally is never clobbered.
///   A seeded chip also appends a `[[chip]]` section (see
///   [`crate::codegen::wokwi`]) to the emitted `wokwi.toml`, whichever one was
///   emitted: the generated one or the example's own.
/// * Everything else (`example.espf.txt`, `app/`, ...) is the CLI's business
///   and ignored here.
pub fn layer_overlay(
    board: &Board,
    opts: &ResolvedOptions,
    rendered: &mut Rendered,
    overlay: &[OverlayFile],
) -> Result<()> {
    let artifacts = &mut rendered.artifacts;
    let assets = &mut rendered.assets;
    let wokwi = opts.is_on("wokwi");
    let keep_zig = opts.is_on("keep-zig-source");

    let mut chip_json: Option<&[u8]> = None;
    let mut have_chip = false;
    for f in overlay {
        match f.path {
            "diagram.json" | "wokwi.toml" => {
                if let Some(a) = artifacts.iter_mut().find(|a| a.path == f.path) {
                    let text = std::str::from_utf8(f.contents)
                        .with_context(|| format!("example `{}` is not valid UTF-8", f.path))?;
                    a.content = codegen::overlay::render(text, f.path, board)?;
                }
            }
            "chip.json" | "chip.wasm" if wokwi => {
                have_chip = true;
                if f.path == "chip.json" {
                    chip_json = Some(f.contents);
                }
                seed(f, artifacts, assets);
            }
            p if p.starts_with("chip/") && wokwi && keep_zig => seed(f, artifacts, assets),
            _ => {}
        }
    }

    // A seeded custom chip needs a `[[chip]]` section in the emitted
    // `wokwi.toml` so the `chip-<name>` part in diagram.json resolves.
    if have_chip && let Some(a) = artifacts.iter_mut().find(|a| a.path == "wokwi.toml") {
        let json = chip_json.and_then(|b| std::str::from_utf8(b).ok());
        a.content.push_str(&codegen::wokwi::chip_section(json));
    }

    Ok(())
}

/// Copy an overlay file as seed-once: text becomes an artifact, binary
/// (and anything `.wasm`) an asset.
fn seed(f: &OverlayFile, artifacts: &mut Vec<Artifact>, assets: &mut Vec<Asset>) {
    match std::str::from_utf8(f.contents) {
        Ok(text) if !f.path.ends_with(".wasm") => {
            artifacts.push(Artifact::seed_once(f.path, text));
        }
        _ => assets.push(Asset::seed_once(f.path, f.contents)),
    }
}

/// `README.txt`: says what is machine-owned and what is yours.
pub fn readme(project_name: &str, artifacts: &[Artifact], assets: &[Asset]) -> Artifact {
    let mut owned: Vec<&str> = artifacts
        .iter()
        .filter(|a| a.ownership == Ownership::Owned)
        .map(|a| a.path.as_str())
        .collect();
    let mut seeded: Vec<&str> = artifacts
        .iter()
        .filter(|a| a.ownership == Ownership::SeedOnce)
        .map(|a| a.path.as_str())
        .chain(
            assets
                .iter()
                .filter(|a| a.ownership == Ownership::SeedOnce)
                .map(|a| a.path.as_str()),
        )
        .collect();
    owned.push("README.txt");
    owned.sort_unstable();
    seeded.sort_unstable();
    let list = |v: &[&str]| {
        if v.is_empty() {
            "  (none)".to_string()
        } else {
            v.iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    Artifact::owned(
        "README.txt",
        format!(
            "espforge generated project: {project_name}\n\
==========================================\n\
\n\
SOURCE OF TRUTH\n\
  This directory is GENERATED from an .espf file. Edit the .espf (and your\n\
  src/app.rs next to it), then re-run `espforge build`.\n\
\n\
MACHINE-OWNED (do not edit; regenerated every build):\n\
{}\n\
\n\
SEEDED ONCE (written the first time, then yours; never overwritten):\n\
{}\n\
\n\
REGENERATE:\n\
  espforge build <your.espf> --out <this dir>\n",
            list(&owned),
            list(&seeded)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::case;
    use crate::runtime_source::RuntimeSource;
    use espforge_model::{
        Component, Ident, Led, Owner, OwnerKind, Pin, PinUse, ResourceGraph, Runtime,
    };

    fn project(flags: &[(&str, &str)]) -> (Board, ResolvedOptions) {
        let case = case("esp32c3", Runtime::Blocking, true, flags, &[]);
        let mut resources = ResourceGraph::new();
        let pin = resources
            .claim(
                Pin(18),
                Owner::new(OwnerKind::Led, Ident::new("red-led", 0..0)),
                0..0,
                PinUse::Output,
            )
            .unwrap();
        let board = Board {
            meta: case.meta.clone(),
            resources,
            components: vec![Component::Led(Led {
                id: Ident::new("red-led", 0..0),
                pin,
                active_low: false,
            })],
            services: Vec::new(),
        };
        (board, case.opts)
    }

    fn generated(board: &Board, opts: &ResolvedOptions, overlay: &[OverlayFile]) -> Rendered {
        let artifacts = codegen::generate(board, &board.meta, opts, &RuntimeSource::Published)
            .unwrap_or_else(|e| panic!("{e:#}"));
        let mut rendered = Rendered {
            artifacts,
            assets: Vec::new(),
        };
        layer_overlay(board, opts, &mut rendered, overlay).unwrap();
        rendered
    }

    fn file<'a>(r: &'a Rendered, path: &str) -> &'a Artifact {
        r.artifacts
            .iter()
            .find(|a| a.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
    }

    fn overlay_files(files: &[(&'static str, &'static [u8])]) -> Vec<OverlayFile<'static>> {
        files
            .iter()
            .map(|(path, contents)| OverlayFile { path, contents })
            .collect()
    }

    const EXAMPLE_DIAGRAM: &str = "{\n  \"version\": 1,\n  \"parts\": [\n    { \"type\": \"{{ board }}\", \"id\": \"esp\", \"attrs\": { \"gpio\": \"{{ red_led.pin }}\" } }\n  ],\n  \"connections\": [ [ \"esp:TX\", \"{{ serialMonitor }}:RX\", \"\", [] ] ]\n}\n";

    #[test]
    fn an_example_diagram_replaces_the_generated_one_with_tokens_resolved() {
        let (board, opts) = project(&[]);
        let overlay = overlay_files(&[("diagram.json", EXAMPLE_DIAGRAM.as_bytes())]);
        let rendered = generated(&board, &opts, &overlay);
        let diagram = &file(&rendered, "diagram.json").content;
        let json: serde_json::Value = serde_json::from_str(diagram).unwrap();
        assert_eq!(json["parts"][0]["type"], "board-esp32-c3-devkitm-1");
        assert_eq!(json["parts"][0]["attrs"]["gpio"], "18");
        assert_eq!(json["connections"][0][1], "$serialMonitor:RX");
        assert!(!diagram.contains("{{"), "{diagram}");
    }

    #[test]
    fn an_unknown_token_fails_the_layer_with_a_typed_error() {
        let (board, opts) = project(&[]);
        let overlay = overlay_files(&[("diagram.json", b"{{ nope }}")]);
        let artifacts =
            codegen::generate(&board, &board.meta, &opts, &RuntimeSource::Published).unwrap();
        let mut rendered = Rendered {
            artifacts,
            assets: Vec::new(),
        };
        let err = layer_overlay(&board, &opts, &mut rendered, &overlay).unwrap_err();
        assert!(
            err.downcast_ref::<codegen::overlay::OverlayError>()
                .is_some(),
            "{err:#}"
        );
    }

    #[test]
    fn chip_json_and_wasm_are_always_copied_but_zig_source_is_optional() {
        let (board, opts) = project(&[]);
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "chip"}"#),
            ("chip.wasm", b"\0asm"),
            ("chip/main.zig", b"const std = @import(\"std\");"),
        ]);
        let off = generated(&board, &opts, &overlay);
        assert!(file(&off, "chip.json").content.contains("\"name\""));
        assert!(off.assets.iter().any(|a| a.path == "chip.wasm"));
        assert!(off.artifacts.iter().all(|a| a.path != "chip/main.zig"));

        // with `keep-zig-source`, the sources come along
        let with_sources = case(
            "esp32c3",
            Runtime::Blocking,
            true,
            &[("keep-zig-source", "true")],
            &[],
        );
        let on = generated(&board, &with_sources.opts, &overlay);
        assert!(on.artifacts.iter().any(|a| a.path == "chip/main.zig"));
    }

    #[test]
    fn a_seeded_chip_appends_the_chip_section_to_wokwi_toml() {
        let (board, opts) = project(&[]);
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "mcp23017"}"#),
            ("chip.wasm", b"\0asm"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        let wokwi = &file(&rendered, "wokwi.toml").content;
        assert!(wokwi.contains("[[chip]]"), "{wokwi}");
        assert!(wokwi.contains("name = 'mcp23017'"), "{wokwi}");
        assert!(wokwi.contains("binary = 'chip.wasm'"), "{wokwi}");
    }

    #[test]
    fn no_chip_means_no_chip_section_in_wokwi_toml() {
        let (board, opts) = project(&[]);
        let rendered = generated(&board, &opts, &[]);
        assert!(!file(&rendered, "wokwi.toml").content.contains("[[chip]]"));
    }

    #[test]
    fn wokwi_off_drops_the_example_wokwi_files_too() {
        let (board, opts) = project(&[("wokwi", "false")]);
        let overlay = overlay_files(&[
            ("diagram.json", EXAMPLE_DIAGRAM.as_bytes()),
            ("chip.json", br#"{"name": "chip"}"#),
            ("chip.wasm", b"\0asm"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        assert!(rendered.artifacts.iter().all(|a| a.path != "diagram.json"));
        assert!(rendered.artifacts.iter().all(|a| a.path != "chip.json"));
        assert!(rendered.assets.is_empty());
    }

    #[test]
    fn keep_zig_source_is_a_no_op_without_a_chip_directory() {
        let (board, _) = project(&[]);
        let with_sources = case(
            "esp32c3",
            Runtime::Blocking,
            true,
            &[("keep-zig-source", "true")],
            &[],
        );
        let rendered = generated(&board, &with_sources.opts, &[]);
        assert!(rendered.assets.is_empty());
        assert!(
            rendered
                .artifacts
                .iter()
                .all(|a| !a.path.starts_with("chip"))
        );
    }

    #[test]
    fn non_wokwi_example_files_are_ignored() {
        let (board, opts) = project(&[]);
        let overlay = overlay_files(&[
            ("example.espf.txt", b"# espforge\n"),
            ("app/rust/app.rs", b"pub fn setup() {}\n"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        assert!(
            rendered
                .artifacts
                .iter()
                .all(|a| a.path != "app/rust/app.rs")
        );
        assert!(
            rendered
                .artifacts
                .iter()
                .all(|a| a.path != "example.espf.txt")
        );
    }

    #[test]
    fn readme_lists_owned_and_seeded_files() {
        let (board, opts) = project(&[]);
        let rendered = generated(&board, &opts, &[]);
        let readme = readme("hello_world", &rendered.artifacts, &rendered.assets);
        assert!(readme.content.contains("README.txt"));
        assert!(readme.content.contains("  - Cargo.toml"));
        assert!(readme.content.contains("  - rust-toolchain.toml"));
        assert!(
            readme.content.contains("MACHINE-OWNED"),
            "the two lists stay distinct: {}",
            readme.content
        );
    }
}
