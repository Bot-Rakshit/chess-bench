use clap::Parser;
use rayon::prelude::*;
use serde::Deserialize;
use shakmaty::{Chess, Position, fen::Fen, san::San, EnPassantMode, Color};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio, ChildStdin, ChildStdout};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

const STOCKFISH_PATH: &str = "/opt/homebrew/bin/stockfish";

#[derive(Parser)]
struct Args {
    #[arg(default_value = "hikaru")]
    username: String,
    #[arg(default_value = "1000")]
    games: usize,
    #[arg(long, default_value = "4")]
    workers: usize,
    #[arg(long, default_value = "1")]
    threads: usize,
    #[arg(long, default_value = "4")]
    depth: u32,
}

#[derive(Deserialize)]
struct ArchivesResponse { archives: Vec<String> }

#[derive(Deserialize)]
struct GamesResponse { games: Vec<GameData> }

#[derive(Deserialize, Clone)]
struct GameData {
    pgn: Option<String>,
    white: Option<PlayerData>,
    black: Option<PlayerData>,
}

#[derive(Deserialize, Clone)]
struct PlayerData { username: Option<String> }

struct StockfishEngine {
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    depth: u32,
    line_buf: String,
}

impl StockfishEngine {
    fn new(threads: usize, depth: u32) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut child = Command::new(STOCKFISH_PATH)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        // Use smaller buffer for lower latency (like Python's bufsize=1)
        let reader = BufReader::with_capacity(256, stdout);
        
        let mut engine = Self { 
            stdin, 
            reader, 
            depth,
            line_buf: String::with_capacity(512),
        };
        
        engine.send("uci")?;
        engine.wait_for("uciok")?;
        engine.send(&format!("setoption name Threads value {}", threads))?;
        engine.send("setoption name UCI_ShowWDL value true")?;
        engine.send("isready")?;
        engine.wait_for("readyok")?;
        Ok(engine)
    }

    #[inline]
    fn send(&mut self, cmd: &str) -> Result<(), std::io::Error> {
        writeln!(self.stdin, "{}", cmd)?;
        self.stdin.flush()
    }

    fn wait_for(&mut self, token: &str) -> Result<(i32, i32, i32), Box<dyn std::error::Error + Send + Sync>> {
        let mut wdl = (333, 334, 333);
        
        loop {
            self.line_buf.clear();
            self.reader.read_line(&mut self.line_buf)?;
            
            // Check for WDL in this line (avoid allocation by working with &str)
            if let Some(wdl_pos) = self.line_buf.find(" wdl ") {
                let after_wdl = &self.line_buf[wdl_pos + 5..];
                let parts: Vec<&str> = after_wdl.split_whitespace().take(3).collect();
                if parts.len() >= 3 {
                    wdl = (
                        parts[0].parse().unwrap_or(333),
                        parts[1].parse().unwrap_or(334),
                        parts[2].parse().unwrap_or(333),
                    );
                }
            }
            
            if self.line_buf.contains(token) {
                return Ok(wdl);
            }
        }
    }

    #[inline]
    fn analyze(&mut self, fen: &str) -> Result<(i32, i32, i32), Box<dyn std::error::Error + Send + Sync>> {
        self.send(&format!("position fen {}", fen))?;
        self.send(&format!("go depth {}", self.depth))?;
        self.wait_for("bestmove")
    }

    fn quit(&mut self) {
        let _ = self.send("quit");
    }
}

#[inline]
fn wdl_to_prob(w: i32, d: i32, l: i32, is_white: bool) -> f64 {
    let (w, l) = if is_white { (w, l) } else { (l, w) };
    (w as f64 + d as f64 * 0.5) / 1000.0
}

#[inline]
fn calc_accuracy(before: f64, after: f64) -> f64 {
    if after >= before { 100.0 } else { (100.0 * (1.0 - (before - after) * 2.0)).max(0.0) }
}

fn parse_pgn_moves(pgn: &str) -> Vec<&str> {
    let mut moves = Vec::with_capacity(100);
    let mut in_moves = false;
    
    for line in pgn.lines() {
        let line = line.trim();
        if line.starts_with('[') { continue; }
        if !line.is_empty() { in_moves = true; }
        if in_moves {
            let mut i = 0;
            let bytes = line.as_bytes();
            while i < bytes.len() {
                // Skip comments {...}
                if bytes[i] == b'{' {
                    while i < bytes.len() && bytes[i] != b'}' { i += 1; }
                    i += 1;
                    continue;
                }
                // Skip whitespace
                if bytes[i].is_ascii_whitespace() { i += 1; continue; }
                // Find token end
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'{' { i += 1; }
                let token = &line[start..i];
                // Skip move numbers and results
                if !token.contains('.') && token != "1-0" && token != "0-1" && token != "1/2-1/2" && token != "*" {
                    moves.push(token);
                }
            }
        }
    }
    moves
}

fn fetch_archives(username: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder().user_agent("ChessBenchmark/1.0").build()?;
    let resp: ArchivesResponse = client.get(format!("https://api.chess.com/pub/player/{}/games/archives", username)).send()?.json()?;
    Ok(resp.archives)
}

fn fetch_games(url: &str) -> Result<Vec<GameData>, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder().user_agent("ChessBenchmark/1.0").build()?;
    let resp: GamesResponse = client.get(url).send()?.json()?;
    Ok(resp.games)
}

fn analyze_game(game: &GameData, username: &str, sf_threads: usize, depth: u32) -> Option<(f64, f64, usize, String, String)> {
    let pgn = game.pgn.as_ref()?;
    let white = game.white.as_ref()?.username.as_ref()?.to_lowercase();
    let black = game.black.as_ref()?.username.as_ref()?.to_lowercase();
    let target = username.to_lowercase();
    if white != target && black != target { return None; }

    let moves = parse_pgn_moves(pgn);
    if moves.is_empty() { return None; }

    let mut engine = StockfishEngine::new(sf_threads, depth).ok()?;
    let mut pos = Chess::default();
    
    // Pre-allocate accuracy vectors
    let mut white_acc = Vec::with_capacity(moves.len() / 2 + 1);
    let mut black_acc = Vec::with_capacity(moves.len() / 2 + 1);
    
    // Reuse FEN buffer
    let mut fen_buf = Fen::from_position(&pos, EnPassantMode::Legal).to_string();
    let (mut pw, mut pd, mut pl) = engine.analyze(&fen_buf).ok()?;

    for m in moves {
        let is_white = pos.turn() == Color::White;
        let san = San::from_str(m).ok()?;
        let mv = san.to_move(&pos).ok()?;
        pos = pos.play(mv).ok()?;
        
        // Generate FEN
        fen_buf = Fen::from_position(&pos, EnPassantMode::Legal).to_string();
        let (cw, cd, cl) = engine.analyze(&fen_buf).ok()?;
        
        let acc = calc_accuracy(wdl_to_prob(pw, pd, pl, is_white), wdl_to_prob(cw, cd, cl, is_white));
        if is_white { white_acc.push(acc); } else { black_acc.push(acc); }
        pw = cw; pd = cd; pl = cl;
    }
    engine.quit();

    let wa = if white_acc.is_empty() { 0.0 } else { white_acc.iter().sum::<f64>() / white_acc.len() as f64 };
    let ba = if black_acc.is_empty() { 0.0 } else { black_acc.iter().sum::<f64>() / black_acc.len() as f64 };
    Some((wa, ba, white_acc.len() + black_acc.len(), white, black))
}

fn main() {
    let args = Args::parse();
    
    println!("Rust Chess Benchmark");
    println!("{}", "=".repeat(50));
    println!("Username: {}", args.username);
    println!("Max games: {}", args.games);
    println!("Workers: {}", args.workers);
    println!("SF threads/worker: {}", args.threads);
    println!("Total CPU: {}", args.workers * args.threads);
    println!("Depth: {}", args.depth);
    println!();

    rayon::ThreadPoolBuilder::new().num_threads(args.workers).build_global().unwrap();

    println!("Fetching archives...");
    let fetch_start = Instant::now();
    let mut archives = fetch_archives(&args.username).expect("Failed to fetch");
    archives.reverse();

    let mut all_games = Vec::new();
    for url in &archives {
        if all_games.len() >= args.games { break; }
        if let Ok(games) = fetch_games(url) {
            let parts: Vec<&str> = url.split('/').collect();
            println!("  Fetched {} games from {}/{}", games.len(), parts[parts.len()-2], parts[parts.len()-1]);
            all_games.extend(games);
        }
    }
    all_games.truncate(args.games);
    let fetch_time = fetch_start.elapsed();
    println!("Fetched {} games in {:.2}s\n", all_games.len(), fetch_time.as_secs_f64());

    println!("Analyzing games...");
    let analysis_start = Instant::now();
    let completed = Arc::new(AtomicUsize::new(0));
    let total = all_games.len();

    let results: Vec<_> = all_games.par_iter().map(|g| {
        let r = analyze_game(g, &args.username, args.threads, args.depth);
        let c = completed.fetch_add(1, Ordering::Relaxed) + 1;
        if c % 10 == 0 || c == total {
            println!("  Analyzed {}/{} games ({:.2} games/sec)", c, total, c as f64 / analysis_start.elapsed().as_secs_f64());
        }
        r
    }).collect();

    let analysis_time = analysis_start.elapsed();
    let target = args.username.to_lowercase();
    let mut user_acc = Vec::new();
    let mut total_moves = 0;
    let mut analyzed = 0;

    for r in results.into_iter().flatten() {
        analyzed += 1;
        total_moves += r.2;
        if r.3 == target { user_acc.push(r.0); } else { user_acc.push(r.1); }
    }

    let avg = if user_acc.is_empty() { 0.0 } else { user_acc.iter().sum::<f64>() / user_acc.len() as f64 };

    println!("\nResults");
    println!("{}", "=".repeat(50));
    println!("Games analyzed: {}", analyzed);
    println!("Total moves: {}", total_moves);
    println!("Average accuracy for {}: {:.2}%", args.username, avg);
    println!("\nPerformance");
    println!("{}", "=".repeat(50));
    println!("Fetch time: {:.2}s", fetch_time.as_secs_f64());
    println!("Analysis time: {:.2}s", analysis_time.as_secs_f64());
    println!("Total time: {:.2}s", fetch_time.as_secs_f64() + analysis_time.as_secs_f64());
    println!("Games per second: {:.4}", analyzed as f64 / analysis_time.as_secs_f64());
    println!("Moves per second: {:.2}", total_moves as f64 / analysis_time.as_secs_f64());
}
