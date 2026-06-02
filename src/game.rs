//! Play a single game between two engines and record the result.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Color, EnPassantMode, KnownOutcome, Outcome, Position};

use crate::cli::Limit;
use crate::engine::{Engine, SearchRequest};

/// Hard ceiling on plies, so a pathological game can never run forever.
const MAX_PLIES: u32 = 600;

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

/// Play one full game. `white` and `black` are the engines for each color;
/// `start_fen` is the EPD/FEN starting position.
pub fn play_game(
    white: &mut Engine,
    black: &mut Engine,
    start_fen: &str,
    limit: &Limit,
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

    // Per-color clocks, used only in time mode.
    let mut clocks: [u64; 2] = match limit {
        Limit::Time(tc) => [tc.base_ms, tc.base_ms],
        Limit::Nodes(_) => [0, 0],
    };

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
        let (mover, opponent_result) = match side {
            Color::White => (&mut *white, GameResult::BlackWins),
            Color::Black => (&mut *black, GameResult::WhiteWins),
        };

        // Build the search request for this move.
        let request = match limit {
            Limit::Nodes(n) => SearchRequest::Nodes(*n),
            Limit::Time(tc) => SearchRequest::Time {
                wtime: clocks[idx(Color::White)],
                btime: clocks[idx(Color::Black)],
                winc: tc.increment_ms,
                binc: tc.increment_ms,
            },
        };

        let (uci_str, elapsed) = mover.search(start_fen, &moves, &request)?;
        time_used[idx(side)] += elapsed;

        // Time mode: deduct from the clock and check for a flag fall.
        if let Limit::Time(tc) = limit {
            let elapsed_ms = elapsed.as_millis() as u64;
            if elapsed_ms > clocks[idx(side)] {
                return Ok(finish(opponent_result, Termination::TimeForfeit, sans, time_used, start_fullmove, start_white_to_move));
            }
            clocks[idx(side)] = clocks[idx(side)] - elapsed_ms + tc.increment_ms;
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
