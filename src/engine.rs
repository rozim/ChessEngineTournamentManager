//! A thin UCI driver around a child engine process.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use crate::cli::Limit;
use crate::config::EngineConfig;

/// A running UCI engine we can drive turn by turn.
pub struct Engine {
    /// Display name from the JSON config.
    pub name: String,
    /// The `id name` reported by the engine after the `uci` handshake.
    pub id_name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Engine {
    /// Spawn the engine, perform the `uci` handshake, apply options, and wait
    /// until it is ready.
    pub fn start(cfg: &EngineConfig) -> Result<Engine> {
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
    /// the move in UCI notation and the wall-clock time spent thinking.
    pub fn search(
        &mut self,
        start_fen: &str,
        moves: &[String],
        limit: &SearchRequest,
    ) -> Result<(String, Duration)> {
        self.set_position(start_fen, moves)?;
        let go = match *limit {
            SearchRequest::Time {
                wtime,
                btime,
                winc,
                binc,
            } => format!("go wtime {wtime} btime {btime} winc {winc} binc {binc}"),
            SearchRequest::Nodes(nodes) => format!("go nodes {nodes}"),
        };

        let started = Instant::now();
        self.send(&go)?;
        let mut best = None;
        self.read_until(|line| {
            if let Some(rest) = line.strip_prefix("bestmove ") {
                best = rest.split_whitespace().next().map(|s| s.to_string());
                true
            } else {
                false
            }
        })?;
        let elapsed = started.elapsed();

        let best = best.ok_or_else(|| anyhow!("engine '{}' sent empty bestmove", self.name))?;
        Ok((best, elapsed))
    }

    /// Politely ask the engine to quit, then reap the process.
    pub fn quit(mut self) {
        let _ = self.send("quit");
        let _ = self.child.wait();
    }
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
}

impl SearchRequest {
    /// Build a node-limited request from a [`Limit::Nodes`].
    pub fn nodes_from_limit(limit: &Limit) -> Option<SearchRequest> {
        match limit {
            Limit::Nodes(n) => Some(SearchRequest::Nodes(*n)),
            Limit::Time(_) => None,
        }
    }
}
