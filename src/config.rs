//! Engine JSON configuration files.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// The search limit for one engine, as written in its JSON config. The `mode`
/// field selects the variant; the remaining fields are mode-specific.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum LimitConfig {
    /// Clock with a base time and per-move increment. The engine receives a
    /// `go wtime/btime/winc/binc` command and is the only kind of engine for
    /// which the manager keeps time (and can forfeit on the clock).
    Time {
        #[serde(default = "default_seconds")]
        seconds: u64,
        #[serde(default = "default_increment")]
        increment: f64,
    },
    /// Fixed node count per move (`go nodes N`).
    Nodes { nodes: u64 },
    /// Fixed search depth per move (`go depth D`).
    Depth { depth: u32 },
}

fn default_seconds() -> u64 {
    60
}

fn default_increment() -> f64 {
    0.1
}

impl Default for LimitConfig {
    fn default() -> Self {
        LimitConfig::Time {
            seconds: default_seconds(),
            increment: default_increment(),
        }
    }
}

/// A validated, normalized per-engine search limit used at runtime.
#[derive(Debug, Clone, Copy)]
pub enum SearchLimit {
    /// Base clock and increment, both in milliseconds.
    Time { base_ms: u64, inc_ms: u64 },
    Nodes(u64),
    Depth(u32),
}

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
    /// Per-engine search limit. Defaults to a 60s + 0.1s time control when
    /// omitted.
    #[serde(default)]
    pub limit: LimitConfig,
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

    /// Validate and normalize this engine's configured search limit.
    pub fn search_limit(&self) -> Result<SearchLimit> {
        match self.limit {
            LimitConfig::Time { seconds, increment } => {
                if seconds == 0 {
                    bail!("engine '{}': time limit seconds must be greater than zero", self.name);
                }
                if !increment.is_finite() || increment < 0.0 {
                    bail!(
                        "engine '{}': time increment must be a finite, non-negative number",
                        self.name
                    );
                }
                Ok(SearchLimit::Time {
                    base_ms: seconds.saturating_mul(1000),
                    inc_ms: (increment * 1000.0).round() as u64,
                })
            }
            LimitConfig::Nodes { nodes } => {
                if nodes == 0 {
                    bail!("engine '{}': node limit must be greater than zero", self.name);
                }
                Ok(SearchLimit::Nodes(nodes))
            }
            LimitConfig::Depth { depth } => {
                if depth == 0 {
                    bail!("engine '{}': depth limit must be greater than zero", self.name);
                }
                Ok(SearchLimit::Depth(depth))
            }
        }
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

    /// A compact one-line description of this engine's search configuration,
    /// used for the `X-White-Configuration` / `X-Black-Configuration` PGN tags.
    pub fn pgn_configuration(&self) -> String {
        let limit = match &self.limit {
            LimitConfig::Time { seconds, increment } => format!("time {seconds}s+{increment}s"),
            LimitConfig::Nodes { nodes } => format!("nodes {nodes}"),
            LimitConfig::Depth { depth } => format!("depth {depth}"),
        };
        if self.options.is_empty() {
            limit
        } else {
            let opts = self
                .option_strings()
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{limit}; options: {opts}")
        }
    }
}
