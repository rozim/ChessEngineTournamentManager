# Goal

Implement a headless, terminal-only, non-GUI, Rust application that runs a chess tournament with multiple UCI chess engines.

# Guidelines

Try to use the Rust crates: shakmaty, stockfish, pgn-reader

# Description

## Basic logic

- For every pair of configured chess engines, they play a match that is a configurable number of mini-matches.
- A mini-match is 2 games from the same starting position, with each engine taking turns as black and white.
- A single opening set is chosen once (one position per mini-match number) and
  shared by every pair of engines, so all pairs play the same openings. The
  selection is seeded (--seed) and thus reproducible; both games of a given
  mini-match start from that mini-match's opening.
- Detect end of game: mate, draw, lack of sufficient material, too many moves without progress.
- Configurable early-draw adjudication (enabled by default): once the game has
  reached a configurable full-move number (default 34), if both engines report
  scores within a centipawn band of equality (default +-20cp) for a number of
  consecutive full moves (default 8), the game is adjudicated a draw. Any score
  outside the band resets the streak. The end-of-game reason is reported as
  "early_draw" (not just "draw").
- Configurable early-resign (loss) adjudication (enabled by default): if an
  engine's own reported score stays at or below a threshold (default -400cp)
  for a number of consecutive full moves (default 3), the game is scored as a
  loss for that (trailing) engine. Any better score resets the streak. The
  end-of-game reason is reported as "early_resign".
- At the start of a game, tell each engine to clear their hash table.
- Optional per-engine move weakening (configured in JSON): in balanced
  positions (best-move eval within a centipawn band of 0), with a configurable
  probability, play a good-but-not-best move chosen at random from the engine's
  MultiPV candidates that are within a score margin of the best. Seeded from
  --seed for reproducibility. Can be globally disabled with --no-weaken.
- There is no tournament-wide search mode. Each engine independently runs in
  one of three modes, chosen in its own JSON configuration:
-- time limited (base time and increment configured)
-- node limited (node count configured)
-- depth limited (search depth configured)
- When telling an engine to analyze, send the UCI "go" command that matches
  that engine's own mode:
-- time limited: "go wtime <w> btime <b> winc <wi> binc <bi>"
-- node limited: "go nodes X"
-- depth limited: "go depth X"
- Only keep time and enforce time forfeits for engines that are time limited.
  Node- and depth-limited engines are never timed and cannot lose on time.
  (Wall-clock time used is still measured for all engines, for reporting.)

## Output

### match.pgn
- Clear the file match.pgn on startup.
- Write each game in PGN to the file.
- After one game is written, flush the output buffer.
- For "White" and "Black" use the engine name from JSON config
- Add tags "XWhiteIdName" and "XBlackIdName" for the 'id name ...' each
  engine reports after the "uci" command is sent to it.
- Add tags "XWhiteConfiguration" and "XBlackConfiguration" describing each
  engine's search configuration (its mode and parameters, plus UCI options).

### Stdout

For every game completed, show this on 1 line:
- name of each engine
- result ("white wins", "black wins", or "draw")
- time used by each engine
- cumulative game #
- match #
- end of line is FEN of starting position

After all matches are done, for every engine show:
- summary of results (points, wins, losses, draws).
-- For points, win=1, draw=0.5, loss=0
- relative ELO change based on results
- a 95% confidence interval on the relative ELO, computed pentanomially (the
  per-mini-match pair is the sampling unit); shown as n/a at 0%/100% scores or
  with fewer than two pairs

Order the engines based on highest points first.


## Command line arguments

- Search limits are per engine (see JSON configuration below), so there is no
  tournament-wide mode/time/nodes flag.
- EPD file of chess game starting positions
-- Find an EPD file of unbalanced chess positions that will probably not result in draws
-- Store this in the repository
-- Default to this file
- Number of mini matches to play for each pair of engines. Default 1.
- Seed for choosing the shared opening set. Optional; a fresh random seed is
  used and printed when omitted, so any run can be reproduced.
- Concurrency: number of games to play in parallel. Default 1. Each parallel
  worker runs its own set of engine processes.
- Early-draw adjudication settings: disable flag (default enabled); minimum
  full-move number (default 34); centipawn band (default 20); number of
  consecutive full moves (default 8).
- Early-resign adjudication settings: disable flag (default enabled); resign
  centipawn threshold (default 400, i.e. score <= -400); number of consecutive
  full moves (default 3).
- Global flag to disable per-engine move weakening (--no-weaken).
- Positional arguments of JSON configuration files for each chess engine
-- There must be 2 or more of these


## JSON engine configuration

- path the engine binary
- text name of engine - use this in the PGN output
- optional UCI options to set with the UCI setoption command.
- search limit, selecting this engine's mode and its parameters. Required and
  explicit (there is no default). The config must specify exactly one mode and
  only that mode's fields:
-- time:  { "mode": "time", "seconds": <int>, "increment": <float seconds> }
-- nodes: { "mode": "nodes", "nodes": <int> }
-- depth: { "mode": "depth", "depth": <int> }
- optional move weakening, an object "weaken" with fields: probability
  (0..1, default 0.15), margin_cp (default 30), candidates / MultiPV
  (default 4, must be >= 2), balance_cp (default 50), temperature (default 0).

### JSON Validation
-- Across all the JSON engine configuration files, every engine must have a unique name
-- Every path must be a path to an executable binary
-- The name cannot be empty
-- The search limit is required and must specify exactly one mode, with only
   that mode's fields present (no extra or unknown fields)
-- The search limit values must be valid: time seconds > 0 and increment finite
   and >= 0; node count > 0; depth > 0
