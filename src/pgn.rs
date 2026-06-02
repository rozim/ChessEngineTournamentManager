//! Append games to `match.pgn` in standard PGN format.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::game::GameRecord;

/// Buffered writer for the tournament's `match.pgn` file.
pub struct PgnWriter {
    out: BufWriter<File>,
}

impl PgnWriter {
    /// Create (truncating any existing file) the PGN output.
    pub fn create(path: &Path) -> Result<PgnWriter> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .with_context(|| format!("creating PGN file {}", path.display()))?;
        Ok(PgnWriter {
            out: BufWriter::new(file),
        })
    }

    /// Write one game and flush, so the file is durable after every game.
    #[allow(clippy::too_many_arguments)]
    pub fn write_game(
        &mut self,
        event: &str,
        round: &str,
        date: &str,
        white_name: &str,
        black_name: &str,
        white_id: &str,
        black_id: &str,
        start_fen: &str,
        white_config: &str,
        black_config: &str,
        game: &GameRecord,
    ) -> Result<()> {
        let tag = |w: &mut BufWriter<File>, name: &str, value: &str| -> Result<()> {
            writeln!(w, "[{name} \"{}\"]", escape(value)).map_err(Into::into)
        };

        tag(&mut self.out, "Event", event)?;
        tag(&mut self.out, "Site", "local")?;
        tag(&mut self.out, "Date", date)?;
        tag(&mut self.out, "Round", round)?;
        tag(&mut self.out, "White", white_name)?;
        tag(&mut self.out, "Black", black_name)?;
        tag(&mut self.out, "Result", game.result.pgn())?;
        tag(&mut self.out, "SetUp", "1")?;
        tag(&mut self.out, "FEN", start_fen)?;
        tag(&mut self.out, "Termination", game.termination.description())?;
        tag(&mut self.out, "XWhiteIdName", white_id)?;
        tag(&mut self.out, "XBlackIdName", black_id)?;
        tag(&mut self.out, "XWhiteConfiguration", white_config)?;
        tag(&mut self.out, "XBlackConfiguration", black_config)?;
        writeln!(self.out)?;

        let body = movetext(game);
        let result = game.result.pgn();
        // A game can have no recorded moves (e.g. an immediate forfeit); avoid
        // emitting a leading space before the result token in that case.
        if body.is_empty() {
            writeln!(self.out, "{result}")?;
        } else {
            writeln!(self.out, "{body} {result}")?;
        }
        writeln!(self.out)?;

        self.out.flush().context("flushing PGN file")?;
        Ok(())
    }
}

/// Escape backslashes and quotes inside a PGN tag value.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the SAN move text with correct move numbers, handling positions that
/// start with Black to move.
fn movetext(game: &GameRecord) -> String {
    let mut out = String::new();
    let mut number = game.start_fullmove;
    let mut white_to_move = game.start_white_to_move;

    for (i, san) in game.sans.iter().enumerate() {
        if white_to_move {
            out.push_str(&format!("{number}. {san} "));
        } else {
            if i == 0 {
                // First recorded move is Black's: show the ellipsis form.
                out.push_str(&format!("{number}... {san} "));
            } else {
                out.push_str(&format!("{san} "));
            }
            number += 1;
        }
        white_to_move = !white_to_move;
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameRecord, GameResult, Termination};
    use std::time::Duration;

    fn record(sans: &[&str], start_fullmove: u32, white_first: bool) -> GameRecord {
        GameRecord {
            result: GameResult::Draw,
            termination: Termination::EarlyDraw,
            sans: sans.iter().map(|s| s.to_string()).collect(),
            time_used: [Duration::ZERO; 2],
            start_fullmove,
            start_white_to_move: white_first,
        }
    }

    #[test]
    fn white_to_move_numbering() {
        assert_eq!(movetext(&record(&["e4", "e5", "Nf3"], 1, true)), "1. e4 e5 2. Nf3");
    }

    #[test]
    fn black_to_move_uses_ellipsis() {
        assert_eq!(movetext(&record(&["Nc6", "Nf3"], 1, false)), "1... Nc6 2. Nf3");
    }

    #[test]
    fn honors_starting_move_number() {
        assert_eq!(movetext(&record(&["Kg1"], 34, true)), "34. Kg1");
    }

    #[test]
    fn empty_movetext_is_empty() {
        assert_eq!(movetext(&record(&[], 1, true)), "");
    }

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
