//! Load starting positions from an EPD file.

use std::path::Path;

use anyhow::{bail, Context, Result};
use shakmaty::fen::Fen;

/// Read an EPD file and return a list of full FEN strings, one per position.
///
/// EPD lines carry four position fields (board, side, castling, en passant)
/// optionally followed by operations. We keep the four position fields and
/// append a zeroed halfmove clock and a fullmove number of 1, then validate
/// each position by parsing it.
pub fn load_epd(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading EPD file {}", path.display()))?;

    let mut positions = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.len() < 4 {
            bail!("{}:{}: EPD line has fewer than 4 fields", path.display(), lineno + 1);
        }
        let fen = format!("{} {} {} {} 0 1", fields[0], fields[1], fields[2], fields[3]);
        fen.parse::<Fen>()
            .with_context(|| format!("{}:{}: invalid position", path.display(), lineno + 1))?;
        positions.push(fen);
    }

    if positions.is_empty() {
        bail!("no positions found in {}", path.display());
    }
    Ok(positions)
}
