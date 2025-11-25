use clap::Parser;
use rayon::prelude::*;
use serde::Deserialize;
use shakmaty::{Chess, Position, san::San, fen::Fen, EnPassantMode};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Parser)]
struct Args {
    #[arg(default_value = "hikaru")]
    username: String,
    #[arg(default_value = "1000")]
    games: usize,
    #[arg(long, default_value = "4")]
    workers: usize,
}

#[derive(Deserialize)]
struct ArchivesResponse { archives: Vec<String> }
#[derive(Deserialize)]
struct GamesResponse { games: Vec<GameData> }
#[derive(Deserialize, Clone)]
struct GameData { pgn: Option<String> }

fn parse_pgn_moves(pgn: &str) -> Vec<String> {
    let mut moves = Vec::new();
    let mut in_moves = false;
    for line in pgn.lines() {
        let line = line.trim();
        if line.starts_with('[') { continue; }
        if !line.is_empty() { in_moves = true; }
        if in_moves {
            let mut cleaned = line.to_string();
            while let Some(s) = cleaned.find('{') {
                if let Some(e) = cleaned.find('}') {
                    cleaned = format!("{}{}", &cleaned[..s], &cleaned[e+1..]);
                } else { break; }
            }
            moves.extend(cleaned.split_whitespace()
                .filter(|t| !t.contains('.') && !["1-0","0-1","1/2-1/2","*"].contains(t))
                .map(String::from));
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

fn parse_game(pgn: &str) -> (usize, usize) {
    let moves = parse_pgn_moves(pgn);
    if moves.is_empty() { return (0, 0); }

    let mut pos = Chess::default();
    let mut mc = 0;
    let mut pc = 1;

    for m in moves {
        if let Ok(san) = San::from_str(&m) {
            if let Ok(mv) = san.to_move(&pos) {
                if let Ok(new_pos) = pos.clone().play(mv) {
                    pos = new_pos;
                    mc += 1;
                    pc += 1;
                    let _ = Fen::from_position(&pos, EnPassantMode::Legal).to_string();
                } else { break; }
            } else { break; }
        } else { break; }
    }
    (mc, pc)
}

fn main() {
    let args = Args::parse();

    println!("Rust PGN Parsing Benchmark");
    println!("{}", "=".repeat(50));
    println!("Library: shakmaty");
    println!("Username: {}", args.username);
    println!("Max games: {}", args.games);
    println!("Workers: {}", args.workers);
    println!();

    rayon::ThreadPoolBuilder::new().num_threads(args.workers).build_global().unwrap();

    println!("Fetching games...");
    let fetch_start = Instant::now();
    let mut archives = fetch_archives(&args.username).expect("Failed");
    archives.reverse();

    let mut all_pgns: Vec<String> = Vec::new();
    for url in &archives {
        if all_pgns.len() >= args.games { break; }
        if let Ok(games) = fetch_games(url) {
            let parts: Vec<&str> = url.split('/').collect();
            println!("  Fetched {} games from {}/{}", games.len(), parts[parts.len()-2], parts[parts.len()-1]);
            for g in games { if let Some(p) = g.pgn { all_pgns.push(p); } }
        }
    }
    all_pgns.truncate(args.games);
    let fetch_time = fetch_start.elapsed();
    println!("Fetched {} games in {:.2}s\n", all_pgns.len(), fetch_time.as_secs_f64());

    println!("Parsing PGNs...");
    let parse_start = Instant::now();
    let completed = Arc::new(AtomicUsize::new(0));
    let total = all_pgns.len();

    let results: Vec<_> = all_pgns.par_iter().map(|p| {
        let r = parse_game(p);
        let c = completed.fetch_add(1, Ordering::SeqCst) + 1;
        if c % 100 == 0 || c == total {
            println!("  Parsed {}/{} games ({:.2} games/sec)", c, total, c as f64 / parse_start.elapsed().as_secs_f64());
        }
        r
    }).collect();

    let parse_time = parse_start.elapsed();
    let (mut tm, mut tp, mut parsed) = (0, 0, 0);
    for (m, p) in results { if m > 0 { tm += m; tp += p; parsed += 1; } }

    println!("\nResults");
    println!("{}", "=".repeat(50));
    println!("Games parsed: {}", parsed);
    println!("Total moves: {}", tm);
    println!("\nPerformance");
    println!("{}", "=".repeat(50));
    println!("Parse time: {:.4}s", parse_time.as_secs_f64());
    println!("Games per second: {:.2}", parsed as f64 / parse_time.as_secs_f64());
    println!("Moves per second: {:.2}", tm as f64 / parse_time.as_secs_f64());
}
