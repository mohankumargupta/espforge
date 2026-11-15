//! The emitted-file plan, in Rust.
//!
//! One list of output paths and the backend that produces each, with
//! conditions as Rust predicates over [`ResolvedOptions`]. This replaced
//! `metadata.toml` + the template planner, and is the only place the emitted
//! file list exists ([`super::generate`] walks it; a unit test pins that the
//! output matches).
//!
//! The optional files the old `metadata.toml` listed (`ci`, editor configs,
//! `Embed.toml`, `hello_test.rs`) never existed in the template tree, so they
//! were never emitted. They are not here either; inventing them would change
//! the output.

use espforge_elements::ResolvedOptions;

/// The backend that produces a planned file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// [`super::rust`] (one of the four Rust files).
    Rust,
    /// [`super::cargo`] (`Cargo.toml`).
    Cargo,
    /// [`super::config`] (`.cargo/config.toml`).
    CargoConfig,
    /// [`super::config`] (`.cargo/esp-config.toml`, static).
    EspConfig,
    /// [`super::config`] (`rust-toolchain.toml`).
    Toolchain,
    /// [`super::wokwi`] (`wokwi.toml`).
    Wokwi,
    /// [`super::diagram`] (`diagram.json`).
    Diagram,
    /// [`super::partitions`] (`partitions.csv`).
    Partitions,
}

/// One file the project emits, before it has any content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Path relative to the generated project root.
    pub out_path: &'static str,
    pub source: Source,
}

/// Every file this project emits, sorted by output path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    pub files: Vec<PlannedFile>,
}

impl FilePlan {
    /// Plan for `opts`. The only live condition is `wokwi`.
    pub fn for_project(opts: &ResolvedOptions) -> Self {
        let mut files = vec![
            PlannedFile {
                out_path: ".cargo/config.toml",
                source: Source::CargoConfig,
            },
            PlannedFile {
                out_path: ".cargo/esp-config.toml",
                source: Source::EspConfig,
            },
            PlannedFile {
                out_path: "Cargo.toml",
                source: Source::Cargo,
            },
            PlannedFile {
                out_path: "build.rs",
                source: Source::Rust,
            },
            PlannedFile {
                out_path: "rust-toolchain.toml",
                source: Source::Toolchain,
            },
            PlannedFile {
                out_path: "src/bin/main.rs",
                source: Source::Rust,
            },
            PlannedFile {
                out_path: "src/generated.rs",
                source: Source::Rust,
            },
            PlannedFile {
                out_path: "src/lib.rs",
                source: Source::Rust,
            },
        ];
        if opts.is_on("wokwi") {
            files.push(PlannedFile {
                out_path: "diagram.json",
                source: Source::Diagram,
            });
            files.push(PlannedFile {
                out_path: "wokwi.toml",
                source: Source::Wokwi,
            });
        }
        // `<ota />` owns the layout the updater writes into. A project with
        // its own `partitions.csv` replaces this file in the overlay layer.
        if opts.is_on("ota") {
            files.push(PlannedFile {
                out_path: "partitions.csv",
                source: Source::Partitions,
            });
        }
        files.sort_by_key(|file| file.out_path);
        Self { files }
    }

    pub fn paths(&self) -> impl Iterator<Item = &'static str> {
        self.files.iter().map(|file| file.out_path)
    }

    pub fn get(&self, out_path: &str) -> Option<&PlannedFile> {
        self.files.iter().find(|file| file.out_path == out_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::case;
    use espforge_elements::Runtime;

    fn options(defaults: bool, flags: &[(&str, &str)]) -> ResolvedOptions {
        case("esp32c3", Runtime::Blocking, defaults, flags, &[]).opts
    }

    #[test]
    fn the_plan_has_every_live_output_and_a_source_for_each() {
        let opts = options(true, &[("wokwi", "true")]);
        let plan = FilePlan::for_project(&opts);
        let paths: Vec<_> = plan.paths().collect();
        for path in [
            ".cargo/config.toml",
            ".cargo/esp-config.toml",
            "Cargo.toml",
            "build.rs",
            "diagram.json",
            "rust-toolchain.toml",
            "src/bin/main.rs",
            "src/generated.rs",
            "src/lib.rs",
            "wokwi.toml",
        ] {
            assert!(paths.contains(&path), "missing {path}: {paths:?}");
            assert!(plan.get(path).is_some(), "no source for {path}");
        }
        assert_eq!(paths.len(), 10);
        assert_eq!(
            plan.get("Cargo.toml").unwrap().source,
            Source::Cargo,
            "Cargo.toml comes from the typed backend"
        );
    }

    /// `<ota />` adds the partition table, and only then.
    #[test]
    fn ota_adds_the_partition_table() {
        let off = FilePlan::for_project(&options(true, &[]));
        assert!(off.get("partitions.csv").is_none(), "no <ota />, no table");

        let mut meta = crate::codegen::test_support::meta("esp32c3");
        meta.ota = Some(espforge_elements::OtaMeta {
            flash_size: espforge_elements::FlashSize::Mb4,
            rollback: true,
        });
        let opts = espforge_elements::resolve(
            &meta,
            &espforge_elements::Answers::empty(0..0),
            &espforge_elements::DerivedInputs::default(),
        )
        .unwrap();
        let on = FilePlan::for_project(&opts);
        assert_eq!(
            on.get("partitions.csv").map(|file| file.source),
            Some(Source::Partitions)
        );
    }

    #[test]
    fn wokwi_off_drops_the_two_wokwi_files() {
        let plan = FilePlan::for_project(&options(true, &[("wokwi", "false")]));
        assert!(plan.get("diagram.json").is_none());
        assert!(plan.get("wokwi.toml").is_none());
        assert!(plan.get("src/lib.rs").is_some());
    }

    #[test]
    fn the_plan_is_sorted_by_output_path() {
        let plan = FilePlan::for_project(&options(true, &[]));
        let mut sorted: Vec<_> = plan.paths().collect();
        sorted.sort_unstable();
        assert_eq!(plan.paths().collect::<Vec<_>>(), sorted);
    }
}
