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

    /// Disable early-draw adjudication (it is enabled by default).
    #[arg(long)]
    pub no_early_draw: bool,

    /// Early draw: only adjudicate once this full-move number is reached.
    #[arg(long, default_value_t = 34)]
    pub early_draw_after: u32,

    /// Early draw: equality band in centipawns; both engines must report a
    /// score within ±this value.
    #[arg(long, default_value_t = 20)]
    pub early_draw_cp: i32,

    /// Early draw: number of consecutive full moves the band must hold.
    #[arg(long, default_value_t = 8)]
    pub early_draw_moves: u32,

    /// Disable early-resign (loss) adjudication (it is enabled by default).
    #[arg(long)]
    pub no_resign: bool,

    /// Early resign: an engine loses if its own score stays at or below
    /// -this many centipawns for the required number of moves.
    #[arg(long, default_value_t = 400)]
    pub resign_cp: i32,

    /// Early resign: number of consecutive full moves the losing score must hold.
    #[arg(long, default_value_t = 3)]
    pub resign_moves: u32,

    /// Log per-move adjudication diagnostics (scores and streak counters) to
    /// stderr, including when an early draw/resign fires.
    #[arg(long)]
    pub debug_adjudication: bool,

    /// Globally disable per-engine move weakening (the "weaken" JSON blocks),
    /// e.g. for a full-strength A/B run without editing configs.
    #[arg(long)]
    pub no_weaken: bool,

    /// JSON configuration files, one per engine (two or more required).
    #[arg(required = true, num_args = 2..)]
    pub configs: Vec<PathBuf>,
}
