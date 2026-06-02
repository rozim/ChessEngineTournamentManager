//! Command line argument parsing and validation.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, ValueEnum};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// Each engine has a clock with a base time and per-move increment.
    Time,
    /// Each engine searches a fixed number of nodes per move.
    Nodes,
}

/// Headless tournament manager for UCI chess engines.
#[derive(Parser, Debug)]
#[command(
    name = "chess_tournament_manager",
    about = "Run a round-robin tournament between UCI chess engines.",
    long_about = None,
)]
pub struct Args {
    /// Tournament mode: `time` (clock + increment) or `nodes` (fixed node count).
    #[arg(long, value_enum, default_value = "time")]
    pub mode: Mode,

    /// [time mode] Base time per game, in whole seconds.
    #[arg(long)]
    pub seconds: Option<u64>,

    /// [time mode] Increment added after each move, in seconds.
    #[arg(long)]
    pub increment: Option<f64>,

    /// [nodes mode] Node limit per engine move.
    #[arg(long)]
    pub nodes: Option<u64>,

    /// EPD file of starting positions (one position per line).
    #[arg(long, default_value = "openings.epd")]
    pub epd: PathBuf,

    /// Number of mini-matches to play for each pair of engines.
    /// A mini-match is two games from the same position with swapped colors.
    #[arg(long, default_value_t = 1)]
    pub mini_matches: u32,

    /// JSON configuration files, one per engine (two or more required).
    #[arg(required = true, num_args = 2..)]
    pub configs: Vec<PathBuf>,
}

/// Resolved, validated time-control settings.
#[derive(Copy, Clone, Debug)]
pub struct TimeControl {
    pub base_ms: u64,
    pub increment_ms: u64,
}

/// The search limit applied to every engine move, after validation.
#[derive(Copy, Clone, Debug)]
pub enum Limit {
    Time(TimeControl),
    Nodes(u64),
}

impl Args {
    /// Validate flag combinations and produce the effective search [`Limit`].
    pub fn limit(&self) -> Result<Limit> {
        match self.mode {
            Mode::Time => {
                if self.nodes.is_some() {
                    bail!("--nodes is not allowed in time mode");
                }
                let base_s = self.seconds.unwrap_or(60);
                let inc_s = self.increment.unwrap_or(0.1);
                if inc_s < 0.0 {
                    bail!("--increment must not be negative");
                }
                Ok(Limit::Time(TimeControl {
                    base_ms: base_s.saturating_mul(1000),
                    increment_ms: (inc_s * 1000.0).round() as u64,
                }))
            }
            Mode::Nodes => {
                if self.seconds.is_some() || self.increment.is_some() {
                    bail!("--seconds and --increment are not allowed in nodes mode");
                }
                let nodes = self
                    .nodes
                    .ok_or_else(|| anyhow::anyhow!("--nodes is required in nodes mode"))?;
                if nodes == 0 {
                    bail!("--nodes must be greater than zero");
                }
                Ok(Limit::Nodes(nodes))
            }
        }
    }
}
