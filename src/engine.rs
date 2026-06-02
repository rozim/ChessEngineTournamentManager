//! A thin UCI driver around a child engine process.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use crate::config::{EngineConfig, SearchLimit, WeakenConfig};

/// A running UCI engine we can drive turn by turn.
pub struct Engine {
    /// Display name from the JSON config.
    pub name: String,
    /// The `id name` reported by the engine after the `uci` handshake.
    pub id_name: String,
    /// This engine's own search limit (time, nodes, or depth).
    pub limit: SearchLimit,
    /// Effective move-weakening settings (None if disabled globally or unset).
    pub weaken: Option<WeakenConfig>,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Engine {
    /// Spawn the engine, perform the `uci` handshake, apply options, and wait
    /// until it is ready. `weaken_enabled` is the global toggle; when false,
    /// this engine's `weaken` config is ignored.
    pub fn start(cfg: &EngineConfig, weaken_enabled: bool) -> Result<Engine> {
        // Normalized search limit (already validated when the config parsed).
        let limit = cfg.search_limit();
        let weaken = if weaken_enabled { cfg.weaken } else { None };

        let mut child = Command::new(&cfg.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawning engine '{}' at {}", cfg.name, cfg.path))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("engine '{}' has no stdin", cfg.name))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("engine '{}' has no stdout", cfg.name))?,
        );

        let mut engine = Engine {
            name: cfg.name.clone(),
            id_name: cfg.name.clone(),
            limit,
            weaken,
            child,
            stdin,
            stdout,
        };

        // UCI handshake.
        engine.send("uci")?;
        let mut id_name = None;
        engine.read_until(|line| {
            if let Some(rest) = line.strip_prefix("id name ") {
                id_name = Some(rest.trim().to_string());
            }
            line == "uciok"
        })?;
        if let Some(name) = id_name {
            engine.id_name = name;
        }

        // Apply configured UCI options.
        for (key, value) in cfg.option_strings() {
            engine.send(&format!("setoption name {key} value {value}"))?;
        }

        // Weakening needs ranked alternatives, so request MultiPV lines. This
        // is sent last so it takes precedence over any MultiPV in `options`.
        if let Some(w) = weaken {
            engine.send(&format!("setoption name MultiPV value {}", w.candidates))?;
        }

        engine.ready()?;
        Ok(engine)
    }

    /// Write a single command line to the engine and flush it.
    fn send(&mut self, command: &str) -> Result<()> {
        writeln!(self.stdin, "{command}")
            .with_context(|| format!("sending '{command}' to engine '{}'", self.name))?;
        self.stdin
            .flush()
            .with_context(|| format!("flushing command to engine '{}'", self.name))?;
        Ok(())
    }

    /// Read lines until `stop` returns true for one of them. The matching line
    /// is included in the scan but not returned.
    fn read_until(&mut self, mut stop: impl FnMut(&str) -> bool) -> Result<String> {
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = self
                .stdout
                .read_line(&mut buf)
                .with_context(|| format!("reading from engine '{}'", self.name))?;
            if n == 0 {
                bail!("engine '{}' closed its output unexpectedly", self.name);
            }
            let line = buf.trim_end();
            if stop(line) {
                return Ok(line.to_string());
            }
        }
    }

    /// Send `isready` and block until `readyok`.
    fn ready(&mut self) -> Result<()> {
        self.send("isready")?;
        self.read_until(|line| line == "readyok")?;
        Ok(())
    }

    /// Tell the engine to start a fresh game, which clears its hash tables.
    pub fn new_game(&mut self) -> Result<()> {
        self.send("ucinewgame")?;
        self.ready()
    }

    /// Set the current position from a starting FEN plus the moves played so
    /// far (in UCI long-algebraic notation).
    fn set_position(&mut self, start_fen: &str, moves: &[String]) -> Result<()> {
        let mut command = format!("position fen {start_fen}");
        if !moves.is_empty() {
            command.push_str(" moves ");
            command.push_str(&moves.join(" "));
        }
        self.send(&command)
    }

    /// Ask the engine for its best move under the given search limit. Returns
    /// the authoritative best move, the ranked MultiPV candidates (each a first
    /// move + its score, ordered best-first), and the wall-clock time spent.
    /// With MultiPV 1 (the default) there is a single candidate: the best move.
    pub fn search(
        &mut self,
        start_fen: &str,
        moves: &[String],
        limit: &SearchRequest,
    ) -> Result<SearchResult> {
        self.set_position(start_fen, moves)?;
        let go = match *limit {
            SearchRequest::Time {
                wtime,
                btime,
                winc,
                binc,
            } => format!("go wtime {wtime} btime {btime} winc {winc} binc {binc}"),
            SearchRequest::Nodes(nodes) => format!("go nodes {nodes}"),
            SearchRequest::Depth(depth) => format!("go depth {depth}"),
        };

        let started = Instant::now();
        self.send(&go)?;
        let mut best = None;
        // Latest (score, first PV move) seen per MultiPV index.
        let mut lines: BTreeMap<u32, (Option<i32>, Option<String>)> = BTreeMap::new();
        self.read_until(|line| {
            if let Some(info) = line.strip_prefix("info ") {
                if let Some((idx, score, mv)) = parse_info_line(info) {
                    let entry = lines.entry(idx).or_default();
                    if score.is_some() {
                        entry.0 = score;
                    }
                    if mv.is_some() {
                        entry.1 = mv;
                    }
                }
                false
            } else if let Some(rest) = line.strip_prefix("bestmove ") {
                best = rest.split_whitespace().next().map(|s| s.to_string());
                true
            } else {
                false
            }
        })?;
        let elapsed = started.elapsed();

        let best = best.ok_or_else(|| anyhow!("engine '{}' sent empty bestmove", self.name))?;

        // Candidates ordered by MultiPV index (BTreeMap iterates ascending), so
        // index 1 (the best line) comes first.
        let mut candidates: Vec<Candidate> = lines
            .into_values()
            .filter_map(|(score, mv)| mv.map(|mv| Candidate { mv, score }))
            .collect();
        if candidates.is_empty() {
            // Engine reported no usable PV line; fall back to the best move.
            candidates.push(Candidate {
                mv: best.clone(),
                score: None,
            });
        }

        Ok(SearchResult {
            best,
            candidates,
            elapsed,
        })
    }
}

/// One MultiPV candidate: the line's first move and the engine's score for it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub mv: String,
    pub score: Option<i32>,
}

/// The result of a search: the best move, the ranked candidates (best-first),
/// and the elapsed wall-clock time.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best: String,
    pub candidates: Vec<Candidate>,
    pub elapsed: Duration,
}

impl Drop for Engine {
    /// Ensure the child process is always terminated and reaped, on every code
    /// path (normal end, early error, or panic). `std::process::Child` does not
    /// kill on drop, so without this a failed run would leave engines running.
    fn drop(&mut self) {
        // Ask politely first, then force the issue so a wedged engine can never
        // make cleanup hang.
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Magnitude a mate score folds to, so it always sits well outside any
/// reasonable equality band.
pub const MATE_CP: i32 = 30_000;

/// Parse a UCI `info` line body (text after "info ") into its MultiPV index
/// (defaulting to 1), its score in centipawns from the side-to-move's
/// perspective (a `mate` score folds to ±[`MATE_CP`]), and the first move of
/// its principal variation. Returns `None` for lines carrying neither a score
/// nor a PV move (e.g. `info string`, `currmove`).
fn parse_info_line(info: &str) -> Option<(u32, Option<i32>, Option<String>)> {
    let words: Vec<&str> = info.split_whitespace().collect();

    let multipv = words
        .iter()
        .position(|&w| w == "multipv")
        .and_then(|p| words.get(p + 1))
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1);

    let score = words.iter().position(|&w| w == "score").and_then(|p| {
        match words.get(p + 1).copied() {
            Some("cp") => words.get(p + 2)?.parse::<i32>().ok(),
            Some("mate") => words
                .get(p + 2)?
                .parse::<i32>()
                .ok()
                .map(|n| if n < 0 { -MATE_CP } else { MATE_CP }),
            _ => None,
        }
    });

    let first_move = words
        .iter()
        .position(|&w| w == "pv")
        .and_then(|p| words.get(p + 1))
        .map(|s| s.to_string());

    if score.is_none() && first_move.is_none() {
        return None;
    }
    Some((multipv, score, first_move))
}

/// The per-move search request handed to [`Engine::search`].
#[derive(Copy, Clone, Debug)]
pub enum SearchRequest {
    Time {
        wtime: u64,
        btime: u64,
        winc: u64,
        binc: u64,
    },
    Nodes(u64),
    Depth(u32),
}
