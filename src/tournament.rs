//! Orchestrate the round-robin, drive games (optionally in parallel), and
//! report standings.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use anyhow::Result;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::config::EngineConfig;
use crate::elo::Standing;
use crate::engine::Engine;
use crate::game::{play_game, Adjudication, GameResult};
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

/// One scheduled game. Game and match numbers are assigned deterministically up
/// front, so output is correctly labeled regardless of completion order.
struct GameTask {
    game_no: u32,
    match_no: u32,
    /// 1 or 2 — which game of its mini-match (for the PGN Round tag).
    game_in_match: u8,
    white_idx: usize,
    black_idx: usize,
    /// Index into the shared opening set.
    opening_idx: usize,
}

/// Choose the shared set of opening positions — one per mini-match, used by
/// every pair of engines. Sampled without replacement when possible, so the
/// openings are distinct; deterministic for a given `seed`.
fn select_openings(positions: &[String], count: u32, seed: u64) -> Vec<String> {
    let mut rng = StdRng::seed_from_u64(seed);
    let n = positions.len();
    let count = count as usize;
    let mut chosen = Vec::with_capacity(count);
    if count <= n {
        let mut seen = HashSet::new();
        while chosen.len() < count {
            let idx = rng.random_range(0..n);
            if seen.insert(idx) {
                chosen.push(positions[idx].clone());
            }
        }
    } else {
        // More mini-matches than available positions: allow repeats.
        for _ in 0..count {
            chosen.push(positions[rng.random_range(0..n)].clone());
        }
    }
    chosen
}

/// Build the full, ordered list of games: every pair plays `mini_matches`
/// mini-matches (two color-swapped games each), all sharing the opening set.
fn build_tasks(n_engines: usize, mini_matches: u32) -> Vec<GameTask> {
    let mut tasks = Vec::new();
    let mut game_no = 0;
    let mut match_no = 0;
    for i in 0..n_engines {
        for j in (i + 1)..n_engines {
            for m in 0..mini_matches {
                match_no += 1;
                for (k, &(white_idx, black_idx)) in [(i, j), (j, i)].iter().enumerate() {
                    game_no += 1;
                    tasks.push(GameTask {
                        game_no,
                        match_no,
                        game_in_match: k as u8 + 1,
                        white_idx,
                        black_idx,
                        opening_idx: m as usize,
                    });
                }
            }
        }
    }
    tasks
}

/// State shared between worker threads, guarded by a single mutex so each
/// finished game's standings update, stdout line, and PGN write are atomic.
struct Shared {
    pgn: PgnWriter,
    standings: Vec<Standing>,
}

/// Run the whole tournament and return the final standings.
pub fn run(
    configs: &[EngineConfig],
    positions: &[String],
    mini_matches: u32,
    date: &str,
    seed: u64,
    concurrency: usize,
    adj: Adjudication,
) -> Result<Vec<Standing>> {
    let openings = select_openings(positions, mini_matches, seed);
    let tasks = build_tasks(configs.len(), mini_matches);
    let n_tasks = tasks.len();
    let num_games = n_tasks;
    let num_matches = n_tasks / 2; // two games per mini-match
    let workers = concurrency.max(1).min(n_tasks.max(1));

    let config_pgn: Vec<String> = configs.iter().map(|c| c.pgn_configuration()).collect();

    let shared = Mutex::new(Shared {
        pgn: PgnWriter::create(Path::new("match.pgn"))?,
        standings: configs.iter().map(|c| Standing::new(&c.name)).collect(),
    });
    let cursor = AtomicUsize::new(0);

    std::thread::scope(|scope| -> Result<()> {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(scope.spawn(|| -> Result<()> {
                // Each worker runs its own set of engine processes; hash tables
                // are cleared per game via ucinewgame. Engines are shut down by
                // Engine's Drop impl when this closure returns (any path).
                let mut engines: Vec<Engine> =
                    configs.iter().map(Engine::start).collect::<Result<_>>()?;

                loop {
                    let t = cursor.fetch_add(1, Ordering::Relaxed);
                    if t >= n_tasks {
                        break;
                    }
                    let task = &tasks[t];
                    let opening = &openings[task.opening_idx];

                    let record = {
                        let (white, black) =
                            pair_mut(&mut engines, task.white_idx, task.black_idx);
                        play_game(white, black, opening, adj, task.game_no)?
                    };

                    let white_id = engines[task.white_idx].id_name.clone();
                    let black_id = engines[task.black_idx].id_name.clone();
                    let white_name = &configs[task.white_idx].name;
                    let black_name = &configs[task.black_idx].name;
                    let wt = record.time_used[0].as_secs_f64();
                    let bt = record.time_used[1].as_secs_f64();
                    let plies = record.sans.len();

                    // Finish the game atomically: standings, stdout, PGN.
                    let mut guard = shared.lock().expect("shared state mutex poisoned");
                    let sh = &mut *guard;
                    match record.result {
                        GameResult::WhiteWins => {
                            sh.standings[task.white_idx].wins += 1;
                            sh.standings[task.black_idx].losses += 1;
                        }
                        GameResult::BlackWins => {
                            sh.standings[task.black_idx].wins += 1;
                            sh.standings[task.white_idx].losses += 1;
                        }
                        GameResult::Draw => {
                            sh.standings[task.white_idx].draws += 1;
                            sh.standings[task.black_idx].draws += 1;
                        }
                    }
                    println!(
                        "game {gno}/{ngames} match {mno}/{nmatches}: {white_name} (W) vs {black_name} (B) -> {res} [{term}] | {plies} moves | W {wt:.2}s B {bt:.2}s | {opening}",
                        gno = task.game_no,
                        ngames = num_games,
                        mno = task.match_no,
                        nmatches = num_matches,
                        res = record.result.phrase(),
                        term = record.termination.description(),
                    );
                    sh.pgn.write_game(
                        "Chess Engine Tournament",
                        &format!("{}.{}", task.match_no, task.game_in_match),
                        date,
                        white_name,
                        black_name,
                        &white_id,
                        &black_id,
                        opening,
                        &config_pgn[task.white_idx],
                        &config_pgn[task.black_idx],
                        &record,
                    )?;
                }
                Ok(())
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread panicked")?;
        }
        Ok(())
    })?;

    Ok(shared.into_inner().expect("shared state mutex poisoned").standings)
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
