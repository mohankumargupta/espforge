//! The project as a whole: layering the project's overlay files over the
//! generated set ([`layer_overlay`], [`OverlayFile`], [`Rendered`]) and the
//! `README.txt` ([`readme`]).
//!
//! The generated files themselves come from [`crate::codegen`]; this module is
//! what happens after them. Every file the user controls lives next to the
//! `.espf` and arrives here as a [`OverlayFile`], so `build/` stays disposable
//! output: nothing in it is ever read back.

use anyhow::{Context as _, Result};
use espforge_elements::{Board, ResolvedOptions};

use crate::artifact::{Artifact, Asset};
use crate::codegen;

/// One overlay file from the project directory, with its path relative to the
/// `.espf`.
#[derive(Debug, Clone, Copy)]
pub struct OverlayFile<'a> {
    pub path: &'a str,
    pub contents: &'a [u8],
}

/// A seeded custom chip whose declaration cannot be used: a name that is not a
/// legal Wokwi part suffix, or a diagram part that names a different chip. Typed
/// so the CLI can report it as a user error rather than an internal one.
#[derive(Debug)]
pub struct ChipError {
    /// The example file the problem is in (`chip.json` / `diagram.json`).
    pub file: &'static str,
    pub message: String,
}

impl std::fmt::Display for ChipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}`: {}", self.file, self.message)
    }
}

impl std::error::Error for ChipError {}

/// A user overlay file that cannot be used: `Cargo.extra.toml` that is not
/// valid TOML, or an overlay that is not valid UTF-8.
#[derive(Debug)]
pub struct OverlayFileError {
    /// The overlay file, as the user named it (`Cargo.extra.toml`).
    pub file: String,
    pub message: String,
}

impl std::fmt::Display for OverlayFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}`: {}", self.file, self.message)
    }
}

impl std::error::Error for OverlayFileError {}

/// Everything a render produces: text files and binary assets.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub artifacts: Vec<Artifact>,
    pub assets: Vec<Asset>,
    /// Overlay decisions worth reporting to the user.
    pub warnings: Vec<String>,
}

/// Layer the project's overlay files over an already-generated project:
///
/// * `diagram.json` / `wokwi.toml` replace the generated ones, with their
///   `{{ board }}`, `{{ serialMonitor }}`, `{{ chip }}` and
///   `{{ <device>.pin }}` placeholders resolved by [`codegen::overlay`]. They
///   only apply when the `wokwi` option emitted the generated versions.
/// * `chip.json` and `chip.wasm` are always copied (when `wokwi` is on);
///   the `chip/` source directory only when `keep-zig-source` is on. An
///   example without `chip/` makes that a no-op.
///   A seeded chip also appends a `[[chip]]` section (see
///   [`codegen::wokwi::chip_section`]) to the emitted `wokwi.toml`, whichever
///   one was emitted: the generated one or the project's own. Its name comes
///   from `chip.json`, must be a legal part suffix, and every `chip-*` part in
///   the diagram has to name the same chip (see [`check_chip_parts`]).
/// * `partitions.csv`, `rust-toolchain.toml`, `.cargo/config.toml` and
///   `.cargo/esp-config.toml` replace the generated files of the same names.
///   A `partitions.csv` also puts `--partition-table partitions.csv` on the
///   espflash runner, unless the user replaced `.cargo/config.toml` outright.
/// * `Cargo.extra.toml` is merged into the generated `Cargo.toml` (see
///   [`codegen::cargo::merge`]): its dependencies, patches, profiles and lints
///   are added, and espforge's own keys stay authoritative.
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

    // The chip's name lives in the example's `chip.json`, the file Wokwi reads.
    // It is resolved before rendering so `{{ chip }}`, the `[[chip]]` section
    // and the diagram part all come from that one value.
    let chip_name: Option<String> = if wokwi {
        overlay
            .iter()
            .find(|f| f.path == "chip.json")
            .and_then(|f| std::str::from_utf8(f.contents).ok())
            .and_then(codegen::wokwi::chip_name)
    } else {
        None
    };
    if let Some(name) = &chip_name {
        codegen::wokwi::validate_chip_name(name).map_err(|message| ChipError {
            file: "chip.json",
            message,
        })?;
    }

    let mut have_chip = false;
    let mut have_partitions = false;
    let mut own_cargo_config = false;
    for f in overlay {
        match f.path {
            "diagram.json" | "wokwi.toml" => {
                if !wokwi {
                    continue;
                }
                if let Some(a) = artifacts.iter_mut().find(|a| a.path == f.path) {
                    let text = std::str::from_utf8(f.contents)
                        .with_context(|| format!("example `{}` is not valid UTF-8", f.path))?;
                    a.content =
                        codegen::overlay::render(text, f.path, board, chip_name.as_deref())?;
                }
            }
            "chip.json" | "chip.wasm" if wokwi => {
                have_chip = true;
                put(f, artifacts, assets);
            }
            p if p.starts_with("chip/") && wokwi && keep_zig => put(f, artifacts, assets),
            "partitions.csv" => {
                have_partitions = true;
                put(f, artifacts, assets);
            }
            ".cargo/config.toml" => {
                own_cargo_config = true;
                put(f, artifacts, assets);
            }
            "rust-toolchain.toml" | ".cargo/esp-config.toml" => put(f, artifacts, assets),
            "Cargo.extra.toml" => {
                merge_cargo_extra(f, artifacts, &mut rendered.warnings)?;
            }
            _ => {}
        }
    }

    // A seeded custom chip needs a `[[chip]]` section in the emitted
    // `wokwi.toml` so the `chip-<name>` part in diagram.json resolves.
    if have_chip && let Some(a) = artifacts.iter_mut().find(|a| a.path == "wokwi.toml") {
        a.content
            .push_str(&codegen::wokwi::chip_section(chip_name.as_deref()));
    }
    if let Some(name) = &chip_name
        && let Some(a) = artifacts.iter().find(|a| a.path == "diagram.json")
    {
        check_chip_parts(&a.content, name)?;
    }

    // A project with its own partition table is flashed with it. A user who
    // replaced `.cargo/config.toml` owns the runner line themselves.
    if have_partitions
        && !own_cargo_config
        && let Some(a) = artifacts
            .iter_mut()
            .find(|a| a.path == ".cargo/config.toml")
    {
        a.content = codegen::config::with_partition_table(&a.content)?;
    }

    Ok(())
}

/// Merge the user's `Cargo.extra.toml` into the generated `Cargo.toml`.
///
/// The generated document stays authoritative for the keys espforge writes;
/// everything else in the extra file is kept. The extra file itself is never
/// written to the output.
fn merge_cargo_extra(
    f: &OverlayFile,
    artifacts: &mut [Artifact],
    warnings: &mut Vec<String>,
) -> Result<()> {
    let text = std::str::from_utf8(f.contents).map_err(|_| OverlayFileError {
        file: f.path.to_string(),
        message: "is not valid UTF-8".to_string(),
    })?;
    let Some(cargo) = artifacts.iter_mut().find(|a| a.path == "Cargo.toml") else {
        return Ok(());
    };
    let merged =
        codegen::cargo::merge(Some(text), &cargo.content).map_err(|e| OverlayFileError {
            file: f.path.to_string(),
            message: format!("{e:#}"),
        })?;
    cargo.content = merged.content;
    warnings.extend(
        merged
            .warnings
            .into_iter()
            .map(|warning| format!("Cargo.extra.toml: {warning}")),
    );
    Ok(())
}

/// Every `chip-*` part in an example diagram must name the project's custom
/// chip: Wokwi resolves the part through `wokwi.toml`'s single `[[chip]]`, so a
/// different suffix is a diagram that cannot load.
fn check_chip_parts(diagram: &str, name: &str) -> Result<()> {
    // A diagram that is not JSON is Wokwi's problem, not espforge's: the
    // overlay copies it verbatim and nothing here parses the format.
    let Ok(json) = serde_json::from_str::<serde_json::Value>(diagram) else {
        return Ok(());
    };
    let parts = json.get("parts").and_then(|p| p.as_array());
    for part in parts.into_iter().flatten() {
        let Some(kind) = part.get("type").and_then(|t| t.as_str()) else {
            continue;
        };
        if let Some(other) = kind.strip_prefix("chip-")
            && other != name
        {
            return Err(ChipError {
                file: "diagram.json",
                message: format!(
                    "part type `{kind}` does not match the chip name `{name}` in chip.json"
                ),
            }
            .into());
        }
    }
    Ok(())
}

/// Put an overlay file in place: text becomes an artifact, binary (and anything
/// `.wasm`) an asset. An existing file with the same path is replaced.
fn put(f: &OverlayFile, artifacts: &mut Vec<Artifact>, assets: &mut Vec<Asset>) {
    match std::str::from_utf8(f.contents) {
        Ok(text) if !f.path.ends_with(".wasm") => {
            match artifacts.iter_mut().find(|a| a.path == f.path) {
                Some(a) => a.content = text.to_string(),
                None => artifacts.push(Artifact::new(f.path, text)),
            }
        }
        _ => match assets.iter_mut().find(|a| a.path == f.path) {
            Some(a) => a.bytes = f.contents.to_vec(),
            None => assets.push(Asset::new(f.path, f.contents)),
        },
    }
}

/// `README.txt`: says where the project's source of truth is.
pub fn readme(project_name: &str) -> Artifact {
    Artifact::new(
        "README.txt",
        format!(
            "espforge generated project: {project_name}\n\
==========================================\n\
\n\
SOURCE OF TRUTH\n\
  This directory is GENERATED and disposable: delete it, or `rm -rf build`,\n\
  at any time. Edit the .espf, your src/app.rs, and the optional overlay files\n\
  next to it (diagram.json, wokwi.toml, partitions.csv, rust-toolchain.toml,\n\
  .cargo/*.toml, Cargo.extra.toml), then re-run `espforge build`.\n\
\n\
REGENERATE:\n\
  espforge build <your.espf> --out <this dir>\n"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::case;
    use crate::runtime_source::RuntimeSource;
    use espforge_elements::Runtime;

    fn board_case() -> (Board, ResolvedOptions) {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let board = Board {
            meta: case.meta.clone(),
            resources: espforge_elements::ResourceGraph::default(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        };
        (board, case.opts)
    }

    fn generated(board: &Board, opts: &ResolvedOptions, overlay: &[OverlayFile]) -> Rendered {
        let artifacts = codegen::generate(board, &board.meta, opts, &RuntimeSource::Published)
            .unwrap_or_else(|e| panic!("{e:#}"));
        let mut rendered = Rendered {
            artifacts,
            assets: Vec::new(),
            warnings: Vec::new(),
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

    fn layer_error(
        board: &Board,
        opts: &ResolvedOptions,
        overlay: &[OverlayFile],
    ) -> anyhow::Error {
        let artifacts =
            codegen::generate(board, &board.meta, opts, &RuntimeSource::Published).unwrap();
        let mut rendered = Rendered {
            artifacts,
            assets: Vec::new(),
            warnings: Vec::new(),
        };
        layer_overlay(board, opts, &mut rendered, overlay).expect_err("the layer must fail")
    }

    const EXAMPLE_DIAGRAM: &str = "{\n  \"version\": 1,\n  \"parts\": [\n    { \"type\": \"{{ board }}\", \"id\": \"esp\", \"attrs\": { \"flashSize\": \"16\" } }\n  ],\n  \"connections\": [ [ \"esp:TX\", \"{{ serialMonitor }}:RX\", \"\", [] ] ]\n}\n";

    #[test]
    fn an_example_diagram_replaces_the_generated_one_with_tokens_resolved() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[("diagram.json", EXAMPLE_DIAGRAM.as_bytes())]);
        let rendered = generated(&board, &opts, &overlay);
        let diagram = &file(&rendered, "diagram.json").content;
        let json: serde_json::Value = serde_json::from_str(diagram).unwrap();
        assert_eq!(json["parts"][0]["type"], "board-esp32-c3-devkitm-1");
        assert_eq!(json["connections"][0][1], "$serialMonitor:RX");
        assert!(!diagram.contains("{{"), "{diagram}");
    }

    #[test]
    fn an_unknown_token_fails_the_layer_with_a_typed_error() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[("diagram.json", b"{{ nope }}")]);
        let artifacts =
            codegen::generate(&board, &board.meta, &opts, &RuntimeSource::Published).unwrap();
        let mut rendered = Rendered {
            artifacts,
            assets: Vec::new(),
            warnings: Vec::new(),
        };
        let err = layer_overlay(&board, &opts, &mut rendered, &overlay).unwrap_err();
        assert!(
            err.downcast_ref::<codegen::overlay::OverlayError>()
                .is_some(),
            "{err:#}"
        );
    }

    #[test]
    fn wokwi_off_drops_the_example_wokwi_files_too() {
        let (board, _) = board_case();
        let off = case(
            "esp32c3",
            Runtime::Blocking,
            true,
            &[("wokwi", "false")],
            &[],
        );
        let overlay = overlay_files(&[("diagram.json", EXAMPLE_DIAGRAM.as_bytes())]);
        let rendered = generated(&board, &off.opts, &overlay);
        assert!(rendered.artifacts.iter().all(|a| a.path != "diagram.json"));
        assert!(rendered.artifacts.iter().all(|a| a.path != "wokwi.toml"));
        assert!(rendered.assets.is_empty());
    }

    #[test]
    fn non_wokwi_example_files_are_ignored() {
        let (board, opts) = board_case();
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
    fn chip_json_and_wasm_are_copied_but_zig_source_is_optional() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "chip"}"#),
            ("chip.wasm", b"\0asm"),
            ("chip/chip.zig", b"// zig\n"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        assert!(file(&rendered, "chip.json").content.contains("\"name\""));
        assert!(rendered.assets.iter().any(|a| a.path == "chip.wasm"));
        assert!(rendered.artifacts.iter().all(|a| a.path != "chip/chip.zig"));

        let keep = case(
            "esp32c3",
            Runtime::Blocking,
            true,
            &[("keep-zig-source", "true")],
            &[],
        );
        let rendered = generated(&board, &keep.opts, &overlay);
        assert!(file(&rendered, "chip/chip.zig").content.contains("zig"));
    }

    #[test]
    fn a_seeded_chip_appends_the_chip_section_to_wokwi_toml() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "mcp23017"}"#),
            ("chip.wasm", b"\0asm"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        let wokwi = &file(&rendered, "wokwi.toml").content;
        assert!(wokwi.contains("[[chip]]"), "{wokwi}");
        assert!(wokwi.contains("name = 'mcp23017'"), "{wokwi}");
    }

    #[test]
    fn an_examples_own_wokwi_toml_gets_the_chip_section_too() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            ("wokwi.toml", b"[wokwi]\nversion = 1\n"),
            ("chip.json", br#"{"name": "chip"}"#),
            ("chip.wasm", b"\0asm"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        let wokwi = &file(&rendered, "wokwi.toml").content;
        assert!(wokwi.starts_with("[wokwi]\nversion = 1\n"), "{wokwi}");
        assert!(wokwi.contains("[[chip]]"), "{wokwi}");
    }

    #[test]
    fn the_chip_token_resolves_to_the_seeded_chip_name() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "i2cscanner"}"#),
            ("chip.wasm", b"\0asm"),
            (
                "diagram.json",
                br#"{"version": 1, "parts": [{"type": "chip-{{ chip }}", "id": "chip1"}]}"#,
            ),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        let diagram = &file(&rendered, "diagram.json").content;
        assert!(diagram.contains("chip-i2cscanner"), "{diagram}");
        let wokwi = &file(&rendered, "wokwi.toml").content;
        assert!(wokwi.contains("name = 'i2cscanner'"), "{wokwi}");
    }

    #[test]
    fn a_diagram_part_that_names_another_chip_is_refused() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "i2cscanner"}"#),
            ("chip.wasm", b"\0asm"),
            (
                "diagram.json",
                br#"{"version": 1, "parts": [{"type": "chip-chip", "id": "chip1"}]}"#,
            ),
        ]);
        let err = layer_error(&board, &opts, &overlay);
        let chip = err.downcast_ref::<ChipError>().expect("typed error");
        assert_eq!(chip.file, "diagram.json");
        assert!(chip.message.contains("chip-chip"), "{}", chip.message);
        assert!(chip.message.contains("i2cscanner"), "{}", chip.message);
    }

    #[test]
    fn an_invalid_chip_name_is_refused() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            ("chip.json", br#"{"name": "i2c_scanner"}"#),
            ("chip.wasm", b"\0asm"),
        ]);
        let err = layer_error(&board, &opts, &overlay);
        let chip = err.downcast_ref::<ChipError>().expect("typed error");
        assert_eq!(chip.file, "chip.json");
        assert!(
            chip.message.contains("^[a-z][a-z0-9]*$"),
            "{}",
            chip.message
        );
    }

    #[test]
    fn a_project_rust_toolchain_and_cargo_config_replace_the_generated_ones() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            (
                "rust-toolchain.toml",
                b"[toolchain]\nchannel = \"nightly\"\n",
            ),
            (".cargo/esp-config.toml", b"[env]\nESP_LOG=\"debug\"\n"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        assert!(
            file(&rendered, "rust-toolchain.toml")
                .content
                .contains("nightly")
        );
        assert!(
            file(&rendered, ".cargo/esp-config.toml")
                .content
                .contains("ESP_LOG=\"debug\"")
        );
        // No duplicate artifacts: the generated ones were replaced.
        assert_eq!(
            rendered
                .artifacts
                .iter()
                .filter(|a| a.path == "rust-toolchain.toml")
                .count(),
            1
        );
    }

    #[test]
    fn a_custom_partition_table_is_copied_and_reaches_the_runner() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[(
            "partitions.csv",
            b"# Name, Type, SubType, Offset, Size, Flags\nnvs, data, nvs, , 0x6000,\n",
        )]);
        let rendered = generated(&board, &opts, &overlay);
        assert!(file(&rendered, "partitions.csv").content.contains("nvs"));
        let config = &file(&rendered, ".cargo/config.toml").content;
        assert!(
            config.contains("--partition-table partitions.csv"),
            "{config}"
        );
    }

    #[test]
    fn a_project_cargo_config_keeps_its_own_runner() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[
            (
                ".cargo/config.toml",
                b"[target.riscv32imc-unknown-none-elf]\nrunner = \"probe-rs run --chip=esp32c3\"\n",
            ),
            ("partitions.csv", b"nvs, data, nvs, , 0x6000,\n"),
        ]);
        let rendered = generated(&board, &opts, &overlay);
        let config = &file(&rendered, ".cargo/config.toml").content;
        assert_eq!(
            config,
            "[target.riscv32imc-unknown-none-elf]\nrunner = \"probe-rs run --chip=esp32c3\"\n"
        );
    }

    #[test]
    fn cargo_extra_merges_into_the_generated_manifest() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[(
            "Cargo.extra.toml",
            b"[dependencies]\nserde = \"1\"\n\n[patch.crates-io]\nfoo = { path = \"../foo\" }\n",
        )]);
        let rendered = generated(&board, &opts, &overlay);
        let cargo = &file(&rendered, "Cargo.toml").content;
        assert!(cargo.contains("serde = \"1\""), "{cargo}");
        assert!(cargo.contains("static_cell"), "managed keys stay: {cargo}");
        assert!(cargo.contains("[patch.crates-io]"), "{cargo}");
        assert!(
            rendered
                .artifacts
                .iter()
                .all(|a| a.path != "Cargo.extra.toml"),
            "the extra file is not emitted"
        );
        assert!(rendered.warnings.is_empty(), "{:?}", rendered.warnings);
    }

    #[test]
    fn a_managed_key_written_in_the_extra_is_restored_with_a_warning() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[(
            "Cargo.extra.toml",
            b"[dependencies.esp-hal]\nversion = \"0.1.0\"\n",
        )]);
        let rendered = generated(&board, &opts, &overlay);
        let cargo = &file(&rendered, "Cargo.toml").content;
        assert!(cargo.contains("~1.2.2"), "{cargo}");
        assert_eq!(rendered.warnings.len(), 1, "{:?}", rendered.warnings);
        assert!(
            rendered.warnings[0].contains("dependencies.esp-hal"),
            "{:?}",
            rendered.warnings
        );
    }

    #[test]
    fn a_malformed_cargo_extra_is_a_typed_error() {
        let (board, opts) = board_case();
        let overlay = overlay_files(&[("Cargo.extra.toml", b"[dependencies")]);
        let err = layer_error(&board, &opts, &overlay);
        let extra = err.downcast_ref::<OverlayFileError>().expect("typed error");
        assert_eq!(extra.file, "Cargo.extra.toml");
    }

    #[test]
    fn readme_names_the_overlay_files_and_says_the_output_is_disposable() {
        let readme = readme("hello_world");
        assert!(readme.content.contains("hello_world"));
        assert!(readme.content.contains("disposable"));
        assert!(readme.content.contains("Cargo.extra.toml"));
        assert!(readme.content.contains("espforge build"));
    }
}
