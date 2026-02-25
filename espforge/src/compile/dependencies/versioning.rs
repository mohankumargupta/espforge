use anyhow::{Context, Result};
use std::collections::HashMap;
use toml_edit::DocumentMut;

const VERSIONS_TOML: &str = include_str!("../../../espforge_versions.toml");

pub struct VersionResolver {
    versions: HashMap<String, String>,
}

impl VersionResolver {
    pub fn new() -> Result<Self> {
        let doc: DocumentMut = VERSIONS_TOML
            .parse()
            .context("Failed to parse embedded espforge_versions.toml")?;

        let mut versions = HashMap::new();

        if let Some(espforge_table) = doc.get("espforge").and_then(|t| t.as_table()) {
            for (key, value) in espforge_table.iter() {
                if let Some(version) = value.as_str() {
                    versions.insert(key.to_string(), version.to_string());
                }
            }
        }

        Ok(Self { versions })
    }

    pub fn get(&self, key: &str) -> Result<String> {
        self.versions
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Missing version for {}", key))
    }

    // pub fn get_optional(&self, key: &str) -> Option<String> {
    //     self.versions.get(key).cloned()
    // }
}

impl Default for VersionResolver {
    fn default() -> Self {
        Self::new().expect("Failed to initialize version resolver")
    }
}
