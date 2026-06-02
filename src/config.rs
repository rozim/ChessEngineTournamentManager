//! Engine JSON configuration files.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// The search limit for one engine, as written in its JSON config. The `mode`
/// field selects exactly one variant; the remaining fields are mode-specific.
///
/// This is required in every engine config (there is no default) and is
/// validated strictly on load: exactly one mode must be chosen, only that
/// mode's fields may be present, and the values must be in range (see
/// [`LimitConfig::try_from`]).
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawLimit")]
pub enum LimitConfig {
    /// Clock with a base time (seconds) and per-move increment (seconds). The
    /// engine receives a `go wtime/btime/winc/binc` command and is the only
    /// kind of engine for which the manager keeps time (and can forfeit).
    Time { seconds: u64, increment: f64 },
    /// Fixed node count per move (`go nodes N`).
    Nodes { nodes: u64 },
    /// Fixed search depth per move (`go depth D`).
    Depth { depth: u32 },
}

/// Flat deserialization helper. Every recognized field is captured here so we
/// can enforce — in [`LimitConfig::try_from`] — that exactly the fields
/// belonging to the chosen `mode` are present. `deny_unknown_fields` also
/// rejects typos and stray keys.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimit {
    mode: String,
    seconds: Option<u64>,
    increment: Option<f64>,
    nodes: Option<u64>,
    depth: Option<u32>,
}

impl TryFrom<RawLimit> for LimitConfig {
    type Error = String;

    fn try_from(raw: RawLimit) -> Result<Self, String> {
        match raw.mode.as_str() {
            "time" => {
                if raw.nodes.is_some() || raw.depth.is_some() {
                    return Err("time mode does not allow 'nodes' or 'depth'".into());
                }
                let seconds = raw.seconds.ok_or("time mode requires 'seconds'")?;
                let increment = raw.increment.ok_or("time mode requires 'increment'")?;
                if seconds == 0 {
                    return Err("time 'seconds' must be greater than zero".into());
                }
                if !(increment.is_finite() && increment >= 0.0) {
                    return Err("time 'increment' must be a finite number >= 0".into());
                }
                Ok(LimitConfig::Time { seconds, increment })
            }
            "nodes" => {
                if raw.seconds.is_some() || raw.increment.is_some() || raw.depth.is_some() {
                    return Err("nodes mode does not allow 'seconds', 'increment', or 'depth'".into());
                }
                let nodes = raw.nodes.ok_or("nodes mode requires 'nodes'")?;
                if nodes == 0 {
                    return Err("'nodes' must be greater than zero".into());
                }
                Ok(LimitConfig::Nodes { nodes })
            }
            "depth" => {
                if raw.seconds.is_some() || raw.increment.is_some() || raw.nodes.is_some() {
                    return Err("depth mode does not allow 'seconds', 'increment', or 'nodes'".into());
                }
                let depth = raw.depth.ok_or("depth mode requires 'depth'")?;
                if depth == 0 {
                    return Err("'depth' must be greater than zero".into());
                }
                Ok(LimitConfig::Depth { depth })
            }
            other => Err(format!(
                "unknown mode '{other}', expected 'time', 'nodes', or 'depth'"
            )),
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
    /// Per-engine search limit. Required — every config must specify exactly
    /// one of time, nodes, or depth.
    pub limit: LimitConfig,
    /// Optional move-weakening: occasionally play a decent non-best move in
    /// balanced positions. Absent = full strength.
    #[serde(default)]
    pub weaken: Option<WeakenConfig>,
}

/// Optional per-engine handicap: in roughly balanced positions, with some
/// probability, play a good-but-not-best move (chosen from the engine's
/// MultiPV candidates) to slightly weaken it. See [`EngineConfig::validate`].
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeakenConfig {
    /// Per-move probability of deviating from the best move (0.0–1.0).
    #[serde(default = "default_weaken_probability")]
    pub probability: f64,
    /// An alternative is eligible only if its score is within this many
    /// centipawns of the best move's score (keeps deviations "decent").
    #[serde(default = "default_weaken_margin")]
    pub margin_cp: i32,
    /// Number of candidate moves to consider; sets the engine's `MultiPV`.
    #[serde(default = "default_weaken_candidates")]
    pub candidates: u32,
    /// Only weaken when the best move's score is within ±this of 0 (balanced).
    #[serde(default = "default_weaken_balance")]
    pub balance_cp: i32,
    /// 0 = pick uniformly among eligible alternatives; > 0 = softmax weight by
    /// closeness to the best score (smaller temperature favors better moves).
    #[serde(default = "default_weaken_temperature")]
    pub temperature: f64,
}

fn default_weaken_probability() -> f64 {
    0.15
}
fn default_weaken_margin() -> i32 {
    30
}
fn default_weaken_candidates() -> u32 {
    4
}
fn default_weaken_balance() -> i32 {
    50
}
fn default_weaken_temperature() -> f64 {
    0.0
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

    /// Normalize this engine's configured search limit for use at runtime.
    /// Range validation already happened when the config was parsed (see
    /// [`LimitConfig::try_from`]), so this conversion cannot fail.
    pub fn search_limit(&self) -> SearchLimit {
        match self.limit {
            LimitConfig::Time { seconds, increment } => SearchLimit::Time {
                base_ms: seconds.saturating_mul(1000),
                inc_ms: (increment * 1000.0).round() as u64,
            },
            LimitConfig::Nodes { nodes } => SearchLimit::Nodes(nodes),
            LimitConfig::Depth { depth } => SearchLimit::Depth(depth),
        }
    }

    /// Validate the optional weakening settings.
    pub fn validate(&self) -> Result<()> {
        if let Some(w) = self.weaken {
            if !w.probability.is_finite() || !(0.0..=1.0).contains(&w.probability) {
                bail!("engine '{}': weaken.probability must be between 0 and 1", self.name);
            }
            if w.candidates < 2 {
                bail!("engine '{}': weaken.candidates must be at least 2", self.name);
            }
            if w.margin_cp < 0 {
                bail!("engine '{}': weaken.margin_cp must not be negative", self.name);
            }
            if w.balance_cp < 0 {
                bail!("engine '{}': weaken.balance_cp must not be negative", self.name);
            }
            if !w.temperature.is_finite() || w.temperature < 0.0 {
                bail!("engine '{}': weaken.temperature must be finite and non-negative", self.name);
            }
        }
        Ok(())
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
    /// used for the `XWhiteConfiguration` / `XBlackConfiguration` PGN tags.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<EngineConfig, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn time_limit_normalizes_to_ms() {
        let c = parse(r#"{"path":"/x","name":"x","limit":{"mode":"time","seconds":60,"increment":0.1}}"#).unwrap();
        match c.search_limit() {
            SearchLimit::Time { base_ms, inc_ms } => {
                assert_eq!(base_ms, 60_000);
                assert_eq!(inc_ms, 100);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn nodes_and_depth_parse() {
        let n = parse(r#"{"path":"/x","name":"x","limit":{"mode":"nodes","nodes":1000}}"#).unwrap();
        assert!(matches!(n.search_limit(), SearchLimit::Nodes(1000)));
        let d = parse(r#"{"path":"/x","name":"x","limit":{"mode":"depth","depth":12}}"#).unwrap();
        assert!(matches!(d.search_limit(), SearchLimit::Depth(12)));
    }

    #[test]
    fn rejects_two_modes_in_one_limit() {
        assert!(parse(r#"{"path":"/x","name":"x","limit":{"mode":"nodes","nodes":1,"depth":2}}"#).is_err());
    }

    #[test]
    fn rejects_missing_required_field() {
        assert!(parse(r#"{"path":"/x","name":"x","limit":{"mode":"time","seconds":1}}"#).is_err());
    }

    #[test]
    fn rejects_zero_nodes_and_unknown_mode() {
        assert!(parse(r#"{"path":"/x","name":"x","limit":{"mode":"nodes","nodes":0}}"#).is_err());
        assert!(parse(r#"{"path":"/x","name":"x","limit":{"mode":"clock","seconds":5}}"#).is_err());
    }

    #[test]
    fn weaken_defaults_then_validates() {
        let c = parse(r#"{"path":"/x","name":"x","limit":{"mode":"depth","depth":5},"weaken":{}}"#).unwrap();
        let w = c.weaken.unwrap();
        assert_eq!(w.candidates, 4);
        assert_eq!(w.balance_cp, 50);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn weaken_invalid_values_rejected() {
        let bad = parse(r#"{"path":"/x","name":"x","limit":{"mode":"depth","depth":5},"weaken":{"candidates":1}}"#).unwrap();
        assert!(bad.validate().is_err());
        let bad2 = parse(r#"{"path":"/x","name":"x","limit":{"mode":"depth","depth":5},"weaken":{"probability":2.0}}"#).unwrap();
        assert!(bad2.validate().is_err());
    }

    #[test]
    fn weaken_unknown_field_rejected() {
        assert!(parse(r#"{"path":"/x","name":"x","limit":{"mode":"depth","depth":5},"weaken":{"prob":0.1}}"#).is_err());
    }
}
