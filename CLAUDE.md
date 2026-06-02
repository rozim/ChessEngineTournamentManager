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
- A tournament is in 1 of 2 modes:
-- time mode (starting time and increment time configured)
-- nodes mode (node limit configured)
- When telling an engine to analyze
-- If in time mode, use the UCI command that gives available time.
-- If in nodes mode, use "go nodes X" to search.

## Output

### match.pgn
- Clear the file match.pgn on startup.
- Write each game in PGN to the file.
- After one game is written, flush the output buffer.
- For "White" and "Black" use the engine name from JSON config
- Add a tag "X-White-Id-Name" for the 'id name ...' coming back after the "uci" command is sent to the engine.

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

- Tournament node, time or nodes. Default to time.
-- Time mode
--- Time limit per game in integer seconds. Default 60.
--- Increment per move in floating point seconds. Default 0.1.
--- Node limit flag is prohibited in this mode
-- Node mode
--- Node limit per engine move
--- Time arguments are prohibited in this mode
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

### JSON Validation
-- Across all the JSON engine configuration files, every engine must have a unique name
-- Every path must be a path to an executable binary
-- The name cannot be empty
