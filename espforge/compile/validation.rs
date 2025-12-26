use crate::config::EspforgeConfiguration;
use crate::nibblers::{NibblerDispatcher, NibblerStatus};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn load_and_validate(path: &Path) -> Result<EspforgeConfiguration> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    
    let config: EspforgeConfiguration = serde_yaml_ng::from_str(&content)
        .context("Failed to parse YAML configuration")?;

    validate_with_nibblers(&config)?;
    Ok(config)
}

fn validate_with_nibblers(config: &EspforgeConfiguration) -> Result<()> {
    let dispatcher = NibblerDispatcher::new();
    let results = dispatcher.process_config(config);
    
    let mut error_log = String::new();
    let mut has_errors = false;

    for res in results {
        if res.status == NibblerStatus::Error { has_errors = true; }
        if !res.findings.is_empty() {
            error_log.push_str(&format!("== {} ==\n", res.nibbler_name));
            for finding in res.findings {
                let icon = if res.status == NibblerStatus::Error { "❌" } else { "⚠️" };
                error_log.push_str(&format!("  {} {}\n", icon, finding));
            }
        }
    }

    if has_errors {
        eprintln!("{}", error_log);
        anyhow::bail!("Configuration validation failed.");
    }
    Ok(())
}
