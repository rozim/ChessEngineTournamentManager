//! Command line argument parsing.

use std::path::PathBuf;

use clap::Parser;

/// Headless tournament manager for UCI chess engines.
///
/// Each engine's search limit (time, nodes, or depth) is configured per engine
/// in its JSON file, so there is no tournament-wide mode.
#[derive(Parser, Debug)]
#[command(
    name = "chess_tournament_manager",
    about = "Run a round-robin tournament between UCI chess engines.",
    long_about = None,
)]
pub struct Args {
    /// EPD file of starting positions (one position per line).
    #[arg(long, default_value = "openings.epd")]
    pub epd: PathBuf,

    /// Number of mini-matches to play for each pair of engines.
    /// A mini-match is two games from the same position with swapped colors.
    #[arg(long, default_value_t = 1)]
    pub mini_matches: u32,

    /// Seed for choosing the shared opening set. Every pair of engines plays
    /// the same openings. Omit for a fresh random seed (which is printed so a
    /// run can be reproduced with `--seed`).
    #[arg(long)]
    pub seed: Option<u64>,

    /// Number of games to play in parallel. Each parallel worker runs its own
    /// set of engine processes. Default 1 (sequential, deterministic output).
    #[arg(long, default_value_t = 1)]
    pub concurrency: usize,

    /// JSON configuration files, one per engine (two or more required).
    #[arg(required = true, num_args = 2..)]
    pub configs: Vec<PathBuf>,
}
