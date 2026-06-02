//! Orchestrate the round-robin, drive games, and report standings.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::config::EngineConfig;
use crate::elo::Standing;
use crate::engine::Engine;
use crate::game::{play_game, GameResult};
use crate::pgn::PgnWriter;

/// Borrow two distinct elements of a slice mutably at the same time.
fn pair_mut<T>(slice: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b, "cannot borrow the same element twice");
    if a < b {
        let (left, right) = slice.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = slice.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

/// Run the whole tournament: every pair of engines plays `mini_matches`
/// mini-matches (two color-swapped games each) from a shared starting
/// position drawn at random from `positions`.
pub fn run(
    configs: &[EngineConfig],
    positions: &[String],
    mini_matches: u32,
    date: &str,
) -> Result<Vec<Standing>> {
    // Start every engine once; hash tables are cleared per game via ucinewgame.
    let mut engines: Vec<Engine> = Vec::with_capacity(configs.len());
    for cfg in configs {
        engines.push(Engine::start(cfg)?);
    }

    // Per-engine configuration strings for the PGN X-*-Configuration tags.
    let config_pgn: Vec<String> = configs.iter().map(|c| c.pgn_configuration()).collect();

    let mut standings: Vec<Standing> =
        configs.iter().map(|c| Standing::new(&c.name)).collect();

    let mut pgn = PgnWriter::create(Path::new("match.pgn"))?;

    let mut game_no: u32 = 0;
    let mut match_no: u32 = 0;

    for i in 0..engines.len() {
        for j in (i + 1)..engines.len() {
            for _ in 0..mini_matches {
                match_no += 1;
                let opening = &positions[rand::random_range(0..positions.len())];

                // Two games: i-as-white then j-as-white.
                for &(white_idx, black_idx) in &[(i, j), (j, i)] {
                    game_no += 1;

                    let record = {
                        let (white, black) = pair_mut(&mut engines, white_idx, black_idx);
                        play_game(white, black, opening)?
                    };

                    // Update standings.
                    match record.result {
                        GameResult::WhiteWins => {
                            standings[white_idx].wins += 1;
                            standings[black_idx].losses += 1;
                        }
                        GameResult::BlackWins => {
                            standings[black_idx].wins += 1;
                            standings[white_idx].losses += 1;
                        }
                        GameResult::Draw => {
                            standings[white_idx].draws += 1;
                            standings[black_idx].draws += 1;
                        }
                    }

                    let white_name = &configs[white_idx].name;
                    let black_name = &configs[black_idx].name;
                    let wt = secs(record.time_used[0]);
                    let bt = secs(record.time_used[1]);
                    let plies = record.sans.len();

                    println!(
                        "game {game_no} match {match_no}: {white_name} (W) vs {black_name} (B) -> {result} [{term}] | {plies} moves | W {wt:.2}s B {bt:.2}s | {fen}",
                        result = record.result.phrase(),
                        term = record.termination.description(),
                        fen = opening,
                    );

                    pgn.write_game(
                        "Chess Engine Tournament",
                        &format!("{match_no}.{}", if white_idx == i { 1 } else { 2 }),
                        date,
                        white_name,
                        black_name,
                        &engines[white_idx].id_name,
                        &engines[black_idx].id_name,
                        opening,
                        &config_pgn[white_idx],
                        &config_pgn[black_idx],
                        &record,
                    )?;
                }
            }
        }
    }

    // Engines are shut down by `Engine`'s `Drop` impl when `engines` goes out
    // of scope here — which also covers the early-return error paths above.
    drop(engines);

    Ok(standings)
}

/// Print the final standings table, ordered by points (highest first).
pub fn print_standings(standings: &[Standing]) {
    let mut ranked: Vec<&Standing> = standings.iter().collect();
    ranked.sort_by(|a, b| {
        b.points()
            .partial_cmp(&a.points())
            .unwrap()
            .then(b.relative_elo().partial_cmp(&a.relative_elo()).unwrap())
    });

    println!("\nFinal standings:");
    println!(
        "{:>3}  {:<24} {:>6} {:>4} {:>4} {:>4} {:>7} {:>9}",
        "#", "Engine", "Pts", "W", "D", "L", "Games", "Rel.Elo"
    );
    for (rank, s) in ranked.iter().enumerate() {
        println!(
            "{:>3}  {:<24} {:>6.1} {:>4} {:>4} {:>4} {:>7} {:>+9.1}",
            rank + 1,
            s.name,
            s.points(),
            s.wins,
            s.draws,
            s.losses,
            s.games(),
            s.relative_elo(),
        );
    }
}
