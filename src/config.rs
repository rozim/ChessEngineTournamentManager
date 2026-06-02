//! Engine JSON configuration files.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// One engine as described by a JSON configuration file.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    /// Path to the engine binary.
    pub path: String,
    /// Human-readable name, used for the PGN `White`/`Black` tags and stdout.
    pub name: String,
    /// UCI options to apply via `setoption` before play. Values may be
    /// strings, numbers, or booleans in the JSON; all are sent as text.
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
}

impl EngineConfig {
    /// Load and parse a single engine configuration file.
    pub fn load(path: &Path) -> Result<EngineConfig> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading engine config {}", path.display()))?;
        let cfg: EngineConfig = serde_json::from_str(&text)
            .with_context(|| format!("parsing engine config {}", path.display()))?;
        Ok(cfg)
    }

    /// Render the configured UCI option values as the strings expected by the
    /// `setoption name <id> value <x>` command.
    pub fn option_strings(&self) -> Vec<(String, String)> {
        self.options
            .iter()
            .map(|(k, v)| {
                let value = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    other => other.to_string(),
                };
                (k.clone(), value)
            })
            .collect()
    }
}
