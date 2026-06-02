//! Headless UCI chess engine tournament manager.

mod cli;
mod config;
mod elo;
mod engine;
mod game;
mod pgn;
mod positions;
mod tournament;

use anyhow::{bail, Result};
use clap::Parser;

use std::collections::HashSet;
use std::path::Path;

use crate::cli::Args;
use crate::config::EngineConfig;

/// Enforce the cross-config rules: non-empty unique names and executable paths.
/// (Each engine's search limit is validated when its config is parsed.)
fn validate_configs(configs: &[EngineConfig]) -> Result<()> {
    let mut seen: HashSet<&str> = HashSet::new();
    for cfg in configs {
        if cfg.name.trim().is_empty() {
            bail!("engine name must not be empty (path {})", cfg.path);
        }
        if !seen.insert(cfg.name.as_str()) {
            bail!("duplicate engine name '{}': names must be unique", cfg.name);
        }
        if !is_executable(Path::new(&cfg.path)) {
            bail!(
                "engine '{}': path '{}' is not an executable file",
                cfg.name,
                cfg.path
            );
        }
    }
    Ok(())
}

/// True if `path` is a regular file with an execute permission bit set.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Load engine configurations.
    let configs: Vec<EngineConfig> = args
        .configs
        .iter()
        .map(|p| EngineConfig::load(p))
        .collect::<Result<_>>()?;
    if configs.len() < 2 {
        bail!("at least two engine configurations are required");
    }
    validate_configs(&configs)?;

    // Load starting positions.
    let positions = positions::load_epd(&args.epd)?;

    // A fixed date is fine for a single tournament run.
    let date = "2026.06.01";

    // Resolve the opening seed; print it so the run can be reproduced.
    let seed = args.seed.unwrap_or_else(rand::random::<u64>);
    let concurrency = args.concurrency.max(1);

    let adjudication = game::Adjudication {
        enabled: !args.no_early_draw,
        move_number: args.early_draw_after,
        band_cp: args.early_draw_cp,
        required_plies: args.early_draw_moves.saturating_mul(2),
    };

    println!(
        "Starting tournament: {} engines, {} mini-match(es)/pair from a book of {} positions, seed {}, concurrency {}",
        configs.len(),
        args.mini_matches,
        positions.len(),
        seed,
        concurrency,
    );
    for cfg in &configs {
        println!("  {} -> {}", cfg.name, cfg.pgn_configuration());
    }
    if adjudication.enabled {
        println!(
            "Early-draw adjudication: after move {}, within +-{}cp for {} moves",
            adjudication.move_number, adjudication.band_cp, args.early_draw_moves,
        );
    } else {
        println!("Early-draw adjudication: disabled");
    }

    let standings = tournament::run(
        &configs,
        &positions,
        args.mini_matches,
        date,
        seed,
        concurrency,
        adjudication,
    )?;

    tournament::print_standings(&standings);
    println!("\nGames written to match.pgn");
    Ok(())
}
