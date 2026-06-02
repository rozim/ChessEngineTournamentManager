//! Play a single game between two engines and record the result.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Color, EnPassantMode, KnownOutcome, Outcome, Position};

use crate::config::SearchLimit;
use crate::engine::{Engine, SearchRequest};

/// Hard ceiling on plies, so a pathological game can never run forever.
const MAX_PLIES: u32 = 600;

/// Configurable early-draw adjudication. A game is declared a draw once it has
/// reached `move_number`, and both engines' reported scores have stayed within
/// ±`band_cp` centipawns for `required_plies` consecutive plies. Any score
/// outside the band (including a mate score) resets the streak.
#[derive(Copy, Clone, Debug)]
pub struct Adjudication {
    pub enabled: bool,
    pub move_number: u32,
    pub band_cp: i32,
    pub required_plies: u32,
}

/// Whether a reported score (centipawns) counts as "near equality".
fn within_band(score_cp: i32, band_cp: i32) -> bool {
    score_cp.abs() <= band_cp
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

        let (uci_str, elapsed, score) = mover.search(start_fen, &moves, &request)?;
        time_used[idx(side)] += elapsed;
        if let Some(s) = score {
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

        // Parse and validate the move; anything illegal forfeits the game.
        let parsed = uci_str
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

        // Early-draw adjudication: require both engines' latest scores to stay
        // within the equality band; any breakout resets the streak.
        let both_balanced = match (latest_score[0], latest_score[1]) {
            (Some(w), Some(b)) => within_band(w, adj.band_cp) && within_band(b, adj.band_cp),
            _ => false,
        };
        if both_balanced {
            in_band_plies += 1;
        } else {
            in_band_plies = 0;
        }
        if adj.enabled
            && pos.fullmoves().get() >= adj.move_number
            && in_band_plies >= adj.required_plies
        {
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
