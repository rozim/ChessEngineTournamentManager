//! Play a single game between two engines and record the result.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Color, EnPassantMode, KnownOutcome, Outcome, Position};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::config::{SearchLimit, WeakenConfig};
use crate::engine::{Candidate, Engine, SearchRequest, SearchResult, MATE_CP};

/// Hard ceiling on plies, so a pathological game can never run forever.
const MAX_PLIES: u32 = 600;

/// Configurable game adjudication: an early-draw rule and an early-resign
/// (loss) rule, applied after every move.
#[derive(Copy, Clone, Debug)]
pub struct Adjudication {
    pub draw: DrawRule,
    pub resign: ResignRule,
    /// Log per-move adjudication diagnostics to stderr.
    pub debug: bool,
}

/// A game is declared a draw once it has reached `move_number`, and both
/// engines' reported scores have stayed within ±`band_cp` centipawns for
/// `required_plies` consecutive plies. Any score outside the band (including a
/// mate score) resets the streak.
#[derive(Copy, Clone, Debug)]
pub struct DrawRule {
    pub enabled: bool,
    pub move_number: u32,
    pub band_cp: i32,
    pub required_plies: u32,
}

/// A game is declared a loss for an engine once that engine's own reported
/// score has stayed at or below -`cp` centipawns for `required_plies`
/// consecutive plies. Any better score resets the streak.
#[derive(Copy, Clone, Debug)]
pub struct ResignRule {
    pub enabled: bool,
    pub cp: i32,
    pub required_plies: u32,
}

/// Whether a reported score (centipawns) counts as "near equality".
fn within_band(score_cp: i32, band_cp: i32) -> bool {
    score_cp.abs() <= band_cp
}

/// A score magnitude at/above this is treated as a (near-)mate; never weaken.
fn is_mate(score_cp: i32) -> bool {
    score_cp.abs() >= MATE_CP
}

/// Pick the move to actually play. Normally the engine's best move, but if
/// weakening is enabled and the position is balanced (best score near 0, not a
/// mate), occasionally return a decent alternative within the score margin.
fn choose_move<'a>(
    result: &'a SearchResult,
    weaken: Option<WeakenConfig>,
    rng: &mut StdRng,
) -> &'a str {
    let best = result.best.as_str();
    let w = match weaken {
        Some(w) if w.probability > 0.0 => w,
        _ => return best,
    };
    if result.candidates.len() < 2 {
        return best;
    }
    let best_score = match result.candidates[0].score {
        Some(s) => s,
        None => return best,
    };
    // Only weaken in balanced, non-mate positions.
    if is_mate(best_score) || best_score.abs() > w.balance_cp {
        return best;
    }
    // Eligible alternatives: not the best line, with a score within the margin.
    let acceptable: Vec<&Candidate> = result.candidates[1..]
        .iter()
        .filter(|c| c.score.is_some_and(|s| best_score - s <= w.margin_cp))
        .collect();
    if acceptable.is_empty() {
        return best;
    }
    // Randomized decision to deviate this move.
    if rng.random_range(0.0..1.0) >= w.probability {
        return best;
    }
    pick(&acceptable, best_score, w.temperature, rng).mv.as_str()
}

/// Choose one eligible alternative: uniformly when `temperature` is 0, else
/// softmax-weighted by how little it loses against the best score.
fn pick<'a>(
    acceptable: &[&'a Candidate],
    best_score: i32,
    temperature: f64,
    rng: &mut StdRng,
) -> &'a Candidate {
    if temperature <= 0.0 {
        return acceptable[rng.random_range(0..acceptable.len())];
    }
    let weights: Vec<f64> = acceptable
        .iter()
        .map(|c| {
            let loss = (best_score - c.score.unwrap()) as f64;
            (-loss / temperature).exp()
        })
        .collect();
    let total: f64 = weights.iter().sum();
    let mut r = rng.random_range(0.0..total);
    for (i, weight) in weights.iter().enumerate() {
        r -= weight;
        if r <= 0.0 {
            return acceptable[i];
        }
    }
    acceptable[acceptable.len() - 1]
}

/// The outcome of a single game from White's point of view.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameResult {
    WhiteWins,
    BlackWins,
    Draw,
}

impl GameResult {
    /// PGN result tag.
    pub fn pgn(self) -> &'static str {
        match self {
            GameResult::WhiteWins => "1-0",
            GameResult::BlackWins => "0-1",
            GameResult::Draw => "1/2-1/2",
        }
    }

    /// Human-readable phrase for stdout.
    pub fn phrase(self) -> &'static str {
        match self {
            GameResult::WhiteWins => "white wins",
            GameResult::BlackWins => "black wins",
            GameResult::Draw => "draw",
        }
    }
}

/// Why the game ended (used for PGN comments and diagnostics).
#[derive(Copy, Clone, Debug)]
pub enum Termination {
    Checkmate,
    Stalemate,
    InsufficientMaterial,
    FiftyMoveRule,
    Repetition,
    EarlyDraw,
    EarlyResign,
    MaxPlies,
    TimeForfeit,
    IllegalMove,
}

impl Termination {
    pub fn description(self) -> &'static str {
        match self {
            Termination::Checkmate => "checkmate",
            Termination::Stalemate => "stalemate",
            Termination::InsufficientMaterial => "insufficient material",
            Termination::FiftyMoveRule => "fifty-move rule",
            Termination::Repetition => "threefold repetition",
            Termination::EarlyDraw => "early_draw",
            Termination::EarlyResign => "early_resign",
            Termination::MaxPlies => "move limit reached",
            Termination::TimeForfeit => "time forfeit",
            Termination::IllegalMove => "illegal move",
        }
    }
}

/// Everything produced by playing one game.
pub struct GameRecord {
    pub result: GameResult,
    pub termination: Termination,
    /// SAN move text for the PGN body.
    pub sans: Vec<String>,
    /// Wall-clock thinking time spent by White and Black.
    pub time_used: [Duration; 2],
    /// Fullmove number of the starting position.
    pub start_fullmove: u32,
    /// Side to move in the starting position.
    pub start_white_to_move: bool,
}

/// Repetition key: the parts of a FEN that define position identity
/// (board, side to move, castling rights, en-passant square).
fn repetition_key(pos: &Chess) -> String {
    let fen = Fen::from_position(pos, EnPassantMode::Legal).to_string();
    fen.split_whitespace().take(4).collect::<Vec<_>>().join(" ")
}

/// Index into per-color arrays.
fn idx(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

/// The starting clock for an engine, in milliseconds — present only for
/// time-limited engines, which are the only ones the manager times.
fn starting_clock(limit: SearchLimit) -> Option<u64> {
    match limit {
        SearchLimit::Time { base_ms, .. } => Some(base_ms),
        SearchLimit::Nodes(_) | SearchLimit::Depth(_) => None,
    }
}

/// Per-move increment for a color, used to fill the `winc`/`binc` fields when
/// telling a time-limited engine to search. A non-time-limited opponent has no
/// increment, so we fall back to `placeholder` (the mover's own increment) to
/// keep the reported clock symmetric.
fn increment_for(limit: SearchLimit, placeholder: u64) -> u64 {
    match limit {
        SearchLimit::Time { inc_ms, .. } => inc_ms,
        SearchLimit::Nodes(_) | SearchLimit::Depth(_) => placeholder,
    }
}

/// Play one full game. `white` and `black` are the engines for each color
/// (each carrying its own search limit); `start_fen` is the starting position.
pub fn play_game(
    white: &mut Engine,
    black: &mut Engine,
    start_fen: &str,
    adj: Adjudication,
    game_no: u32,
    seed: u64,
) -> Result<GameRecord> {
    let fen: Fen = start_fen
        .parse()
        .with_context(|| format!("parsing starting FEN '{start_fen}'"))?;
    let mut pos: Chess = fen
        .clone()
        .into_position(shakmaty::CastlingMode::Standard)
        .with_context(|| format!("building position from FEN '{start_fen}'"))?;

    let start_fullmove = pos.fullmoves().get();
    let start_white_to_move = pos.turn() == Color::White;

    // Fresh game: clear hash tables on both engines.
    white.new_game()?;
    black.new_game()?;

    let mut moves: Vec<String> = Vec::new();
    let mut sans: Vec<String> = Vec::new();
    let mut time_used = [Duration::ZERO; 2];

    // Track repetitions, counting the initial position.
    let mut seen: HashMap<String, u32> = HashMap::new();
    seen.insert(repetition_key(&pos), 1);

    // Each engine's own search limit, indexed by color.
    let limits = [white.limit, black.limit];

    // Per-color clocks (milliseconds), present only for time-limited engines.
    let mut clocks: [Option<u64>; 2] = [starting_clock(limits[0]), starting_clock(limits[1])];

    // Latest reported score per color and the running in-band streak, for
    // early-draw adjudication.
    let mut latest_score: [Option<i32>; 2] = [None, None];
    let mut in_band_plies: u32 = 0;
    // Consecutive plies each color's latest score has been at/below the resign
    // threshold, for early-resign adjudication.
    let mut losing_plies: [u32; 2] = [0, 0];

    // Per-game RNG for move weakening, seeded deterministically from the global
    // seed and game number so runs are reproducible regardless of concurrency.
    let mut weaken_rng =
        StdRng::seed_from_u64(seed ^ (game_no as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));

    let mut ply: u32 = 0;
    loop {
        // Board-rule terminations (mate, stalemate, insufficient material).
        match pos.outcome() {
            Outcome::Known(KnownOutcome::Decisive { winner }) => {
                let result = match winner {
                    Color::White => GameResult::WhiteWins,
                    Color::Black => GameResult::BlackWins,
                };
                return Ok(finish(result, Termination::Checkmate, sans, time_used, start_fullmove, start_white_to_move));
            }
            Outcome::Known(KnownOutcome::Draw) => {
                let term = if pos.is_stalemate() {
                    Termination::Stalemate
                } else {
                    Termination::InsufficientMaterial
                };
                return Ok(finish(GameResult::Draw, term, sans, time_used, start_fullmove, start_white_to_move));
            }
            Outcome::Unknown => {}
        }

        // Claimed draws that shakmaty does not assert automatically.
        if pos.halfmoves() >= 100 {
            return Ok(finish(GameResult::Draw, Termination::FiftyMoveRule, sans, time_used, start_fullmove, start_white_to_move));
        }
        if seen.get(&repetition_key(&pos)).copied().unwrap_or(0) >= 3 {
            return Ok(finish(GameResult::Draw, Termination::Repetition, sans, time_used, start_fullmove, start_white_to_move));
        }
        if ply >= MAX_PLIES {
            return Ok(finish(GameResult::Draw, Termination::MaxPlies, sans, time_used, start_fullmove, start_white_to_move));
        }

        let side = pos.turn();
        let mover_limit = limits[idx(side)];
        let (mover, opponent_result) = match side {
            Color::White => (&mut *white, GameResult::BlackWins),
            Color::Black => (&mut *black, GameResult::WhiteWins),
        };

        // Build the UCI search request appropriate to this engine's own limit.
        let request = match mover_limit {
            SearchLimit::Nodes(n) => SearchRequest::Nodes(n),
            SearchLimit::Depth(d) => SearchRequest::Depth(d),
            SearchLimit::Time { inc_ms: mover_inc, .. } => {
                // The mover is time-limited, so its clock is present. For a side
                // whose engine is not time-limited, report the mover's own clock
                // as a neutral stand-in.
                let self_ms = clocks[idx(side)].expect("time-limited engine has a clock");
                SearchRequest::Time {
                    wtime: clocks[idx(Color::White)].unwrap_or(self_ms),
                    btime: clocks[idx(Color::Black)].unwrap_or(self_ms),
                    winc: increment_for(limits[idx(Color::White)], mover_inc),
                    binc: increment_for(limits[idx(Color::Black)], mover_inc),
                }
            }
        };

        let result = mover.search(start_fen, &moves, &request)?;
        let elapsed = result.elapsed;
        time_used[idx(side)] += elapsed;
        // Track the engine's own (best-move) evaluation for adjudication.
        let principal = result.candidates.first().and_then(|c| c.score);
        if let Some(s) = principal {
            latest_score[idx(side)] = Some(s);
        }

        // Timekeeping applies only to the time-limited engine: deduct the
        // elapsed time from its clock and forfeit if it overran.
        if let SearchLimit::Time { inc_ms, .. } = mover_limit {
            let elapsed_ms = elapsed.as_millis() as u64;
            let clock = clocks[idx(side)].as_mut().expect("time-limited engine has a clock");
            if elapsed_ms > *clock {
                return Ok(finish(opponent_result, Termination::TimeForfeit, sans, time_used, start_fullmove, start_white_to_move));
            }
            *clock = *clock - elapsed_ms + inc_ms;
        }

        // Optionally substitute a decent non-best move to slightly weaken the
        // engine in balanced positions.
        let chosen = choose_move(&result, mover.weaken, &mut weaken_rng);

        // Parse and validate the move; anything illegal forfeits the game.
        let parsed = chosen
            .parse::<UciMove>()
            .ok()
            .and_then(|uci| uci.to_move(&pos).ok());
        let mv = match parsed {
            Some(mv) => mv,
            None => {
                return Ok(finish(opponent_result, Termination::IllegalMove, sans, time_used, start_fullmove, start_white_to_move));
            }
        };

        // Record the UCI move, then compute its SAN (with check/mate suffix)
        // and play it in a single pass — no position clone needed.
        moves.push(UciMove::from_move(mv, shakmaty::CastlingMode::Standard).to_string());
        sans.push(SanPlus::from_move_and_play_unchecked(&mut pos, mv).to_string());

        *seen.entry(repetition_key(&pos)).or_insert(0) += 1;
        ply += 1;

        // Update the early-resign streaks: consecutive plies each color's own
        // latest score has been at/below the resign threshold.
        for color in [Color::White, Color::Black] {
            let losing = matches!(latest_score[idx(color)], Some(cp) if cp <= -adj.resign.cp);
            if losing {
                losing_plies[idx(color)] += 1;
            } else {
                losing_plies[idx(color)] = 0;
            }
        }

        // Update the early-draw streak: consecutive plies both engines' latest
        // scores stayed within the equality band.
        let both_balanced = match (latest_score[0], latest_score[1]) {
            (Some(w), Some(b)) => {
                within_band(w, adj.draw.band_cp) && within_band(b, adj.draw.band_cp)
            }
            _ => false,
        };
        if both_balanced {
            in_band_plies += 1;
        } else {
            in_band_plies = 0;
        }

        if adj.debug {
            let fmt = |s: Option<i32>| s.map_or("none".to_string(), |cp| cp.to_string());
            eprintln!(
                "[adj g{game_no}] ply {ply} move {fullmove} {mover} moved score={this} | latest W={lw} B={lb} | resign W={rw} B={rb} (need {rp}) | drawstreak {ds} (need {dp})",
                fullmove = pos.fullmoves().get(),
                mover = if side == Color::White { "W" } else { "B" },
                this = fmt(principal),
                lw = fmt(latest_score[0]),
                lb = fmt(latest_score[1]),
                rw = losing_plies[0],
                rb = losing_plies[1],
                rp = adj.resign.required_plies,
                ds = in_band_plies,
                dp = adj.draw.required_plies,
            );
        }

        // Early-resign: a color loses if its losing streak is long enough.
        // Checked for both colors, since the streak can complete on the
        // opponent's ply.
        if adj.resign.enabled {
            for color in [Color::White, Color::Black] {
                if losing_plies[idx(color)] >= adj.resign.required_plies {
                    if adj.debug {
                        eprintln!(
                            "[adj g{game_no}] EARLY_RESIGN: {} loses (its score <= -{}cp for {} plies); latest W={} B={}",
                            if color == Color::White { "White" } else { "Black" },
                            adj.resign.cp,
                            losing_plies[idx(color)],
                            latest_score[0].map_or("none".to_string(), |c| c.to_string()),
                            latest_score[1].map_or("none".to_string(), |c| c.to_string()),
                        );
                    }
                    let result = match color {
                        Color::White => GameResult::BlackWins,
                        Color::Black => GameResult::WhiteWins,
                    };
                    return Ok(finish(result, Termination::EarlyResign, sans, time_used, start_fullmove, start_white_to_move));
                }
            }
        }

        // Early-draw: both engines balanced for long enough, past the move gate.
        if adj.draw.enabled
            && pos.fullmoves().get() >= adj.draw.move_number
            && in_band_plies >= adj.draw.required_plies
        {
            if adj.debug {
                eprintln!("[adj g{game_no}] EARLY_DRAW: both within +-{}cp for {} plies", adj.draw.band_cp, in_band_plies);
            }
            return Ok(finish(GameResult::Draw, Termination::EarlyDraw, sans, time_used, start_fullmove, start_white_to_move));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finish(
    result: GameResult,
    termination: Termination,
    sans: Vec<String>,
    time_used: [Duration; 2],
    start_fullmove: u32,
    start_white_to_move: bool,
) -> GameRecord {
    GameRecord {
        result,
        termination,
        sans,
        time_used,
        start_fullmove,
        start_white_to_move,
    }
}
