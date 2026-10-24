//! The emitted-file plan, in Rust.
//!
//! Today `metadata.toml` + `project::plan` decide which template files are
//! emitted, under which name, with which ownership. This module is the typed
//! replacement: one list of output paths and ownership rules, with conditions
//! as Rust predicates over [`ResolvedOptions`].
//!
//! It is deliberately *not wired* into rendering yet. Increment 1 proves it
//! agrees with the metadata-driven plan for every option combination the
//! goldens use; the backends switch to it in increments 2–3, and then
//! `metadata.toml` and `project::plan` are deleted.
//!
//! The optional files (`ci`, editor configs, `Embed.toml`, `hello_test.rs`)
//! are absent from the template tree, so the current plan never emits them.
//! They are not in this list either; inventing them would change output.

use espforge_model::Ownership;
use espforge_options::ResolvedOptions;

/// One file the project emits, before it has any content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Template that currently produces it (`Cargo.toml.tmpl` for Cargo.toml).
    /// The typed backend replaces this with a content source in increment 2.
    pub template: &'static str,
    /// Path relative to the generated project root.
    pub out_path: &'static str,
    pub ownership: Ownership,
}

/// Every file this project emits, in template-path order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    pub files: Vec<PlannedFile>,
}

impl FilePlan {
    /// Plan for `opts`. The only live condition today is `wokwi`; the other
    /// `metadata.toml` entries point at files that do not exist in the tree.
    pub fn for_project(opts: &ResolvedOptions) -> Self {
        let mut files = vec![
            PlannedFile {
                template: ".cargo/config.toml",
                out_path: ".cargo/config.toml",
                ownership: Ownership::SeedOnce,
            },
            PlannedFile {
                template: ".cargo/esp-config.toml",
                out_path: ".cargo/esp-config.toml",
                ownership: Ownership::SeedOnce,
            },
            PlannedFile {
                template: "Cargo.toml.tmpl",
                out_path: "Cargo.toml",
                ownership: Ownership::Owned,
            },
            PlannedFile {
                template: "build.rs",
                out_path: "build.rs",
                ownership: Ownership::Owned,
            },
            PlannedFile {
                template: "rust-toolchain.toml",
                out_path: "rust-toolchain.toml",
                ownership: Ownership::SeedOnce,
            },
            PlannedFile {
                template: "src/bin/main.rs",
                out_path: "src/bin/main.rs",
                ownership: Ownership::Owned,
            },
            PlannedFile {
                template: "src/generated.rs",
                out_path: "src/generated.rs",
                ownership: Ownership::Owned,
            },
            PlannedFile {
                template: "src/lib.rs",
                out_path: "src/lib.rs",
                ownership: Ownership::Owned,
            },
        ];
        if opts.is_on("wokwi") {
            files.push(PlannedFile {
                template: "diagram.json",
                out_path: "diagram.json",
                ownership: Ownership::Owned,
            });
            files.push(PlannedFile {
                template: "wokwi.toml",
                out_path: "wokwi.toml",
                ownership: Ownership::Owned,
            });
        }
        files.sort_by_key(|file| file.template);
        Self { files }
    }

    pub fn paths(&self) -> impl Iterator<Item = &'static str> {
        self.files.iter().map(|file| file.out_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::context::{RuntimeSource, build_context};
    use crate::metadata::{metadata, parse_metadata};
    use crate::project::plan;
    use crate::template_env::environment;
    use espforge_model::{Console, Meta, Runtime};
    use espforge_options::{Answers, DerivedInputs, resolve};

    fn options(defaults: bool, flags: &[(&str, &str)]) -> ResolvedOptions {
        let meta = Meta {
            name: "plan test".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        };
        let mut answers = Answers::empty(0..0);
        answers.defaults = defaults;
        for (name, value) in flags {
            answers
                .apply_flag(espforge_options::embedded_schema(), name, value)
                .unwrap();
        }
        resolve(&meta, &answers, &DerivedInputs::default()).unwrap()
    }

    /// The metadata-driven plan for the same options, as comparable tuples.
    fn metadata_plan(opts: &ResolvedOptions) -> Vec<(String, String, Ownership)> {
        let meta = Meta {
            name: "plan test".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        };
        let env = environment().unwrap();
        let md = metadata().unwrap();
        let ctx = build_context(&meta, opts, &RuntimeSource::Published).unwrap();
        let mut planned: Vec<_> = plan(&env, &md, &ctx)
            .unwrap()
            .into_iter()
            .map(|p| (p.template, p.out_path, p.ownership))
            .collect();
        planned.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        planned
    }

    fn typed_plan(opts: &ResolvedOptions) -> Vec<(String, String, Ownership)> {
        let mut planned: Vec<_> = FilePlan::for_project(opts)
            .files
            .into_iter()
            .map(|p| (p.template.to_string(), p.out_path.to_string(), p.ownership))
            .collect();
        planned.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        planned
    }

    #[test]
    fn the_typed_plan_matches_metadata_for_golden_option_sets() {
        let cases = [
            ("defaults", options(true, &[])),
            ("no wokwi", options(true, &[("wokwi", "false")])),
            ("nothing on", options(false, &[])),
            ("zig source", options(true, &[("keep-zig-source", "true")])),
            ("wokwi explicit", options(true, &[("wokwi", "true")])),
        ];
        for (name, opts) in cases {
            assert_eq!(typed_plan(&opts), metadata_plan(&opts), "{name}");
        }
    }

    #[test]
    fn the_plan_has_every_live_output() {
        let opts = options(true, &[("wokwi", "true")]);
        let paths: Vec<_> = FilePlan::for_project(&opts).paths().collect();
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
        }
    }

    #[test]
    fn metadata_still_rejects_the_glob_it_cannot_express() {
        // The typed plan does not need globs, but the old parser stays until
        // increment 3 deletes it; this keeps its contract covered meanwhile.
        assert!(
            parse_metadata("ownership = [ { glob = \"src/*.rs\", own = \"owned\" } ]").is_err()
        );
    }
}
