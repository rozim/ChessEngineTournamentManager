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

/// Enforce the cross-config rules: non-empty unique names, executable paths,
/// and a valid per-engine search limit.
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
        // Validate the configured time/nodes/depth limit up front.
        cfg.search_limit()?;
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

    println!(
        "Starting tournament: {} engines, {} mini-match(es) per pair, {} openings",
        configs.len(),
        args.mini_matches,
        positions.len(),
    );
    for cfg in &configs {
        println!("  {} -> {}", cfg.name, cfg.pgn_configuration());
    }

    let standings = tournament::run(&configs, &positions, args.mini_matches, date)?;

    tournament::print_standings(&standings);
    println!("\nGames written to match.pgn");
    Ok(())
}
