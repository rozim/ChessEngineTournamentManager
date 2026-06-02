//! Generate `openings.epd` from sharp, unbalanced opening lines.
//!
//! Each line below is a sequence of UCI moves played from the standard start.
//! Playing real (gambit / sharp) opening moves guarantees the positions are
//! legal, and these lines steer the game away from quiet, drawish territory.
//!
//! This produces `openings-gambits.epd`, a small hand-curated alternative to
//! the default `openings.epd` (the UHO_4060_v4 book). Pass it explicitly with
//! `--epd openings-gambits.epd`.
//!
//! Run with: `cargo run --example gen_openings > openings-gambits.epd`

use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, EnPassantMode, Position};

/// (name, space-separated UCI moves)
const LINES: &[(&str, &str)] = &[
    ("Kings Gambit Accepted", "e2e4 e7e5 f2f4 e5f4"),
    ("Vienna Gambit", "e2e4 e7e5 b1c3 g8f6 f2f4 d7d5 f4e5 f6e4"),
    ("Danish Gambit", "e2e4 e7e5 d2d4 e5d4 c2c3 d4c3 f1c4 c3b2 c1b2"),
    ("Smith-Morra Gambit", "e2e4 c7c5 d2d4 c5d4 c2c3 d4c3 b1c3"),
    ("Evans Gambit", "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 b2b4 c5b4 c2c3 b4a5"),
    ("Latvian Gambit", "e2e4 e7e5 g1f3 f7f5 f3e5 d8f6"),
    ("Scotch Gambit", "e2e4 e7e5 g1f3 b8c6 d2d4 e5d4 f1c4 f8c5"),
    ("Goring Gambit", "e2e4 e7e5 g1f3 b8c6 d2d4 e5d4 c2c3 d4c3 b1c3"),
    ("Halloween Gambit", "e2e4 e7e5 g1f3 b8c6 b1c3 g8f6 f3e5 c6e5 d2d4"),
    ("Fried Liver", "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 f3g5 d7d5 e4d5 f6d5 g5f7 e8f7"),
    ("Wing Gambit Sicilian", "e2e4 c7c5 b2b4 c5b4 a2a3"),
    ("Blackmar-Diemer", "d2d4 d7d5 e2e4 d5e4 b1c3 g8f6 f2f3"),
    ("Albin Countergambit", "d2d4 d7d5 c2c4 e7e5 d4e5 d5d4"),
    ("Budapest Gambit", "d2d4 g8f6 c2c4 e7e5 d4e5 f6g4"),
    ("Benko Gambit", "d2d4 g8f6 c2c4 c7c5 d4d5 b7b5 c4b5 a7a6"),
    ("Englund Gambit", "d2d4 e7e5 d4e5 b8c6 g1f3 d8e7"),
    ("Froms Gambit", "f2f4 e7e5 f4e5 d7d6 e5d6 f8d6"),
    ("Center Game", "e2e4 e7e5 d2d4 e5d4 d1d4 b8c6 d4e3"),
    ("Cochrane Gambit", "e2e4 e7e5 g1f3 g8f6 f3e5 d7d6 e5f7 e8f7"),
    ("Traxler Counterattack", "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 f3g5 f8c5"),
    ("Marshall Attack stem", "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6 e1g1 f8e7 f1e1 b7b5 a4b3 e8g8 c2c3 d7d5"),
    ("Kings Indian Saemisch", "d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d6 f2f3 e8g8"),
    ("Sicilian Najdorf", "e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4 g8f6 b1c3 a7a6"),
    ("Dutch Staunton Gambit", "d2d4 f7f5 e2e4 f5e4 b1c3 g8f6 c1g5"),
    ("Nimzowitsch Defence", "e2e4 b8c6 d2d4 d7d5 b1c3 d5e4 d4d5"),
    ("Grob Attack", "g2g4 d7d5 f1g2 c8g4 c2c4"),
    ("Schliemann Defence", "e2e4 e7e5 g1f3 b8c6 f1b5 f7f5 b1c3 f5e4"),
    ("Two Knights Modern", "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 d2d4 e5d4"),
];

fn main() {
    println!("# Unbalanced opening positions for the chess engine tournament.");
    println!("# Generated from sharp gambit/attacking lines (see examples/gen_openings.rs).");
    for (name, moves) in LINES {
        let mut pos = Chess::default();
        for token in moves.split_whitespace() {
            let uci: UciMove = token.parse().expect("valid UCI move");
            let mv = uci.to_move(&pos).expect("legal move in line");
            pos.play_unchecked(mv);
        }
        let fen = Fen::from_position(&pos, EnPassantMode::Legal).to_string();
        // EPD = first four FEN fields + an id operation.
        let epd_fields: Vec<&str> = fen.split_whitespace().take(4).collect();
        println!("{} id \"{}\";", epd_fields.join(" "), name);
        // Keep CastlingMode referenced so the import is always used.
        let _ = CastlingMode::Standard;
    }
}
