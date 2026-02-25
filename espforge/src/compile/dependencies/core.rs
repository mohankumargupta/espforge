use anyhow::{Context, Result};
use toml_edit::{DocumentMut, InlineTable, Item, Value};

use super::versioning::VersionResolver;

const ESPFORGE_REPO: &str = "https://github.com/mohankumargupta/espforge";

pub struct CoreDependencies;

impl CoreDependencies {
    pub fn add(doc: &mut DocumentMut, versions: &VersionResolver) -> Result<()> {
        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        let use_git = std::env::var("ESPFORGE_USE_GIT").is_ok();

        let platform_version = versions.get("espforge_platform")?;
        let components_version = versions.get("espforge_components")?;
        let devices_version = versions.get("espforge_devices")?;

        let platform_dep = Self::create_dependency(&platform_version, use_git);
        let components_dep = Self::create_dependency(&components_version, use_git);
        let devices_dep = Self::create_dependency(&devices_version, use_git);

        target_deps.insert("espforge_platform", platform_dep);
        target_deps.insert("espforge_components", components_dep);
        target_deps.insert("espforge_devices", devices_dep);

        Ok(())
    }

    fn create_dependency(version: &str, use_git: bool) -> Item {
        if use_git {
            let mut table = InlineTable::new();
            table.insert("git", Value::from(ESPFORGE_REPO));
            table.insert("branch", Value::from("dev"));
            Item::from(table)
        } else {
            let mut table = InlineTable::new();
            table.insert("version", Value::from(version));
            Item::from(table)
        }
    }
}
