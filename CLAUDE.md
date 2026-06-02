# Goal

Implement a headless, terminal-only, non-GUI, Rust application that runs a chess tournament with multiple UCI chess engines.

# Guidelines

Try to use the Rust crates: shakmaty, stockfish, pgn-reader

# Description

## Basic logic

- For every pair of configured chess engines, they play a match that is a configurable number of mini-matches.
- A mini-match is 2 games from the same starting position, with each engine taking turns as black and white.
- Every (engine1, engine2, mini-match-number) tuple will start from the same randomly chosen opening position
- Detect end of game: mate, draw, lack of sufficient material, too many moves without progress.
- At the start of a game, tell each engine to clear their hash table.
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
- Add tags "X-White-Id-Name" and "X-Black-Id-Name" for the 'id name ...' each
  engine reports after the "uci" command is sent to it.
- Add tags "X-White-Configuration" and "X-Black-Configuration" describing each
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

Order the engines based on highest points first.


## Command line arguments

- Search limits are per engine (see JSON configuration below), so there is no
  tournament-wide mode/time/nodes flag.
- EPD file of chess game starting positions
-- Find an EPD file of unbalanced chess positions that will probably not result in draws
-- Store this in the repository
-- Default to this file
- Number of mini matches to play for each pair of engines. Default 1.
- Positional arguments of JSON configuration files for each chess engine
-- There must be 2 or more of these


## JSON engine configuration

- path the engine binary
- text name of engine - use this in the PGN output
- optional UCI options to set with the UCI setoption command.
- search limit, selecting this engine's mode and its parameters. Optional;
  defaults to time mode with 60 seconds base and 0.1 second increment.
-- time:  { "mode": "time", "seconds": <int, default 60>, "increment": <float seconds, default 0.1> }
-- nodes: { "mode": "nodes", "nodes": <int> }
-- depth: { "mode": "depth", "depth": <int> }

### JSON Validation
-- Across all the JSON engine configuration files, every engine must have a unique name
-- Every path must be a path to an executable binary
-- The name cannot be empty
-- The search limit must be valid: time seconds > 0 and increment finite and
   non-negative; node count > 0; depth > 0
