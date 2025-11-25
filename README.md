# chess-bench

A comprehensive multi-language benchmark suite for chess game analysis, comparing **Python**, **Node.js**, **Rust**, and **Go** implementations.

## What It Does

Fetches games from [Chess.com](https://chess.com) public API and analyzes them using [Stockfish](https://stockfishchess.org/) engine to calculate move accuracy based on WDL (Win-Draw-Loss) probabilities.

**Features:**
- Fetches games from Chess.com API for any username
- Parses PGN (Portable Game Notation) files
- Analyzes each position with Stockfish at configurable depth
- Calculates accuracy scores using WDL probabilities
- Supports parallel processing with configurable workers/threads
- Benchmarks both Stockfish analysis and pure PGN parsing

---

## Benchmark Results

**Test Configuration:**
- Player: hikaru (1000 games)
- Stockfish: depth 4, WDL enabled
- Parallelization: 4 workers × 1 thread each
- Hardware: Apple Silicon (M-series)

### Stockfish Analysis (1000 games)

| Rank | Language | Library | Games/sec | Moves/sec | Time |
|------|----------|---------|-----------|-----------|------|
| 🥇 | **Rust** | shakmaty | **18.73** | **1,643** | **51.5s** |
| 🥈 | Python | python-chess | 18.67 | 1,624 | 53.6s |
| 🥉 | Node.js | chess.js | 17.73 | 1,503 | 56.4s |
| 4 | Go | notnil/chess | 13.84 | 1,206 | 70.7s |

### Pure PGN Parsing (1000 games, no Stockfish)

| Rank | Language | Library | Games/sec | Moves/sec | Time | vs Rust |
|------|----------|---------|-----------|-----------|------|---------|
| 🥇 | **Rust** | shakmaty | **35,121** | **3,001,431** | **0.03s** | 1x |
| 🥈 | Python | python-chess | 251 | 21,817 | 3.98s | 140x slower |
| 🥉 | Node.js | chess.js | 214 | 18,271 | 4.64s | 164x slower |
| 4 | Go | notnil/chess | 40 | 3,483 | 24.5s | 878x slower |

### Parallelization Strategy Comparison

| Strategy | Games/sec | Result |
|----------|-----------|--------|
| **4 workers × 1 SF thread** | **20.03** | ✅ Best for depth 4 |
| 2 workers × 2 SF threads | 11.73 | 41% slower |
| 1 worker × 4 SF threads | 5.98 | 70% slower |

> At shallow depths, game-level parallelism beats Stockfish multi-threading.

---

## Key Findings

### 1. Rust Wins Both Benchmarks

After optimizing I/O buffering (256 bytes vs 8KB default) and eliminating unnecessary allocations, Rust achieves:
- **Fastest Stockfish analysis** (18.73 games/sec)
- **878x faster PGN parsing** than Go

### 2. Python is Surprisingly Competitive

Despite being interpreted, Python nearly matches Rust for Stockfish analysis due to:
- Mature `subprocess` module with optimized IPC
- Excellent `python-chess` library
- GIL irrelevant (each worker has its own Stockfish process)

### 3. Go's Library is the Bottleneck

The `notnil/chess` library is critically slow:
- **4x slower than Python** for PGN parsing
- **878x slower than Rust**
- This is a library problem, not a Go problem

### 4. The Stockfish Bottleneck Effect

```
Time breakdown per game:
├── Stockfish analysis: ~95%
├── PGN parsing:        ~3%
├── IPC overhead:       ~1.5%
└── FEN generation:     ~0.5%
```

When Stockfish dominates, language speed matters less—but library quality still matters!

---

## Installation

### Prerequisites

- **Stockfish** - Install and note the path (default: `/opt/homebrew/bin/stockfish`)
- **Python 3.8+** with pip
- **Node.js 18+** with npm
- **Rust 1.70+** with cargo
- **Go 1.21+**

### Setup

```bash
git clone https://github.com/Bot-Rakshit/chess-bench.git
cd chess-bench

# Python
cd python && pip install -r requirements.txt && cd ..

# Node.js
cd node && npm install && cd ..

# Rust
cd rust && cargo build --release && cd ..

# Go
cd go && go build -o benchmark benchmark.go && go build -o pgn_benchmark pgn_benchmark.go && cd ..
```

---

## Usage

### Stockfish Analysis

Analyze games with full Stockfish evaluation:

```bash
# Python
python python/benchmark.py <username> <games> --workers 4 --threads 1 --depth 4

# Node.js
node node/benchmark.js <username> <games> --workers 4 --threads 1 --depth 4

# Rust
./rust/target/release/benchmark <username> <games> --workers 4 --threads 1 --depth 4

# Go
./go/benchmark <username> <games> -workers 4 -threads 1 -depth 4
```

**Example:**
```bash
python python/benchmark.py hikaru 100 --workers 4 --threads 1 --depth 4
```

### Pure PGN Parsing

Test library parsing speed without Stockfish:

```bash
# Rust (fastest)
./rust/target/release/pgn_benchmark hikaru 1000

# Python
python python/pgn_benchmark.py hikaru 1000

# Node.js
node node/pgn_benchmark.js hikaru 1000

# Go
./go/pgn_benchmark hikaru 1000
```

---

## Project Structure

```
chess-bench/
├── README.md
├── .gitignore
├── python/
│   ├── benchmark.py          # Stockfish analysis
│   ├── pgn_benchmark.py      # Pure PGN parsing
│   └── requirements.txt
├── node/
│   ├── benchmark.js          # Stockfish analysis
│   ├── pgn_benchmark.js      # Pure PGN parsing
│   └── package.json
├── rust/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs           # Stockfish analysis
│       └── bin/
│           └── pgn_benchmark.rs
└── go/
    ├── go.mod
    ├── benchmark.go          # Stockfish analysis
    └── pgn_benchmark.go      # Pure PGN parsing
```

---

## How It Works

### Chess.com API

The benchmark fetches games using Chess.com's public API:
- **Archives endpoint**: `https://api.chess.com/pub/player/{username}/games/archives`
- **Games endpoint**: `https://api.chess.com/pub/player/{username}/games/{YYYY}/{MM}`

### WDL Accuracy Calculation

1. Get WDL (Win/Draw/Loss) probabilities from Stockfish for each position
2. Convert to win probability: `P = (W + D×0.5) / 1000`
3. Calculate accuracy per move:
   - If position improved: `accuracy = 100%`
   - If position worsened: `accuracy = max(0, 100 × (1 - loss × 2))`
4. Average all move accuracies for the target player

---

## Libraries Used

| Language | Library | Version | Notes |
|----------|---------|---------|-------|
| Python | [python-chess](https://python-chess.readthedocs.io/) | 1.10+ | Mature, excellent Stockfish integration |
| Node.js | [chess.js](https://github.com/jhlywa/chess.js) | 1.0.0-beta | Easy to use, TypeScript support |
| Rust | [shakmaty](https://crates.io/crates/shakmaty) | 0.28 | Zero-copy, SIMD optimized, blazingly fast |
| Go | [notnil/chess](https://github.com/notnil/chess) | 1.9.0 | Simple API, but very slow PGN parsing |

---

## Configuration

### Stockfish Path

Default path is `/opt/homebrew/bin/stockfish`. To change:
- **Python**: Edit `STOCKFISH_PATH` in `benchmark.py`
- **Node.js**: Edit `STOCKFISH_PATH` in `benchmark.js`
- **Rust**: Edit `STOCKFISH_PATH` in `src/main.rs`
- **Go**: Edit `StockfishPath` in `benchmark.go`

### Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `username` | Chess.com username | hikaru |
| `games` | Number of games to analyze | 1000 |
| `--workers` | Parallel workers | 4 |
| `--threads` | Stockfish threads per worker | 1 |
| `--depth` | Stockfish search depth | 4 |

---

## Contributing

PRs welcome! Especially interested in:
- Alternative Go chess libraries (to replace slow `notnil/chess`)
- Performance optimizations
- Additional language implementations (C++, Zig, Java, etc.)
- Higher depth analysis comparisons

## License

MIT
