#!/usr/bin/env python3
"""
Chess.com Game Analyzer Benchmark - Python Implementation
Supports configurable workers and Stockfish threads.
"""

import subprocess
import requests
import chess
import chess.pgn
import io
import time
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from typing import List, Tuple, Optional

STOCKFISH_PATH = "/opt/homebrew/bin/stockfish"

@dataclass
class GameAnalysis:
    url: str
    white: str
    black: str
    white_accuracy: float
    black_accuracy: float
    total_moves: int
    analysis_time_ms: float

class StockfishEngine:
    def __init__(self, path: str = STOCKFISH_PATH, threads: int = 1, depth: int = 4):
        self.depth = depth
        self.process = subprocess.Popen(
            [path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1
        )
        self._send("uci")
        self._wait_for("uciok")
        self._send(f"setoption name Threads value {threads}")
        self._send("setoption name UCI_ShowWDL value true")
        self._send("isready")
        self._wait_for("readyok")
    
    def _send(self, cmd: str):
        self.process.stdin.write(cmd + "\n")
        self.process.stdin.flush()
    
    def _wait_for(self, token: str) -> List[str]:
        lines = []
        while True:
            line = self.process.stdout.readline().strip()
            lines.append(line)
            if token in line:
                return lines
    
    def analyze(self, fen: str) -> Tuple[int, int, int]:
        self._send(f"position fen {fen}")
        self._send(f"go depth {self.depth}")
        lines = self._wait_for("bestmove")
        
        wdl = (333, 334, 333)
        for line in reversed(lines):
            if "wdl" in line:
                parts = line.split()
                try:
                    wdl_idx = parts.index("wdl")
                    wdl = (int(parts[wdl_idx + 1]), int(parts[wdl_idx + 2]), int(parts[wdl_idx + 3]))
                except (ValueError, IndexError):
                    pass
                break
        return wdl
    
    def quit(self):
        try:
            self._send("quit")
            self.process.wait(timeout=2)
        except:
            self.process.kill()

def wdl_to_win_prob(wdl: Tuple[int, int, int], is_white: bool) -> float:
    w, d, l = wdl
    if not is_white:
        w, l = l, w
    return (w + d * 0.5) / 1000.0

def calculate_move_accuracy(prob_before: float, prob_after: float) -> float:
    if prob_after >= prob_before:
        return 100.0
    loss = prob_before - prob_after
    return max(0, 100 * (1 - loss * 2))

def fetch_archives(username: str) -> List[str]:
    url = f"https://api.chess.com/pub/player/{username}/games/archives"
    headers = {"User-Agent": "ChessBenchmark/1.0"}
    resp = requests.get(url, headers=headers, timeout=30)
    resp.raise_for_status()
    return resp.json().get("archives", [])

def fetch_games(archive_url: str) -> List[dict]:
    headers = {"User-Agent": "ChessBenchmark/1.0"}
    resp = requests.get(archive_url, headers=headers, timeout=60)
    resp.raise_for_status()
    return resp.json().get("games", [])

def analyze_game(game_data: dict, username: str, sf_threads: int, depth: int) -> Optional[GameAnalysis]:
    pgn_str = game_data.get("pgn")
    if not pgn_str:
        return None
    
    start_time = time.perf_counter()
    
    try:
        pgn = chess.pgn.read_game(io.StringIO(pgn_str))
        if not pgn:
            return None
        
        board = pgn.board()
        engine = StockfishEngine(threads=sf_threads, depth=depth)
        
        white_username = game_data.get("white", {}).get("username", "").lower()
        black_username = game_data.get("black", {}).get("username", "").lower()
        target_lower = username.lower()
        
        is_target_white = white_username == target_lower
        if not is_target_white and black_username != target_lower:
            engine.quit()
            return None
        
        white_accuracies = []
        black_accuracies = []
        
        prev_wdl = engine.analyze(board.fen())
        
        for move in pgn.mainline_moves():
            is_white_move = board.turn == chess.WHITE
            board.push(move)
            
            curr_wdl = engine.analyze(board.fen())
            
            prob_before = wdl_to_win_prob(prev_wdl, is_white_move)
            prob_after = wdl_to_win_prob(curr_wdl, is_white_move)
            
            accuracy = calculate_move_accuracy(prob_before, prob_after)
            
            if is_white_move:
                white_accuracies.append(accuracy)
            else:
                black_accuracies.append(accuracy)
            
            prev_wdl = curr_wdl
        
        engine.quit()
        
        elapsed_ms = (time.perf_counter() - start_time) * 1000
        
        white_acc = sum(white_accuracies) / len(white_accuracies) if white_accuracies else 0
        black_acc = sum(black_accuracies) / len(black_accuracies) if black_accuracies else 0
        
        return GameAnalysis(
            url=game_data.get("url", ""),
            white=white_username,
            black=black_username,
            white_accuracy=white_acc,
            black_accuracy=black_acc,
            total_moves=len(white_accuracies) + len(black_accuracies),
            analysis_time_ms=elapsed_ms
        )
    except Exception as e:
        return None

def main():
    parser = argparse.ArgumentParser(description='Chess.com Game Analyzer Benchmark')
    parser.add_argument('username', nargs='?', default='hikaru', help='Chess.com username')
    parser.add_argument('games', nargs='?', type=int, default=1000, help='Number of games to analyze')
    parser.add_argument('--workers', type=int, default=4, help='Number of parallel workers')
    parser.add_argument('--threads', type=int, default=1, help='Stockfish threads per worker')
    parser.add_argument('--depth', type=int, default=4, help='Stockfish search depth')
    args = parser.parse_args()
    
    total_cpu = args.workers * args.threads
    
    print(f"Python Chess Benchmark")
    print(f"=" * 50)
    print(f"Username: {args.username}")
    print(f"Max games: {args.games}")
    print(f"Workers: {args.workers}")
    print(f"SF threads/worker: {args.threads}")
    print(f"Total CPU usage: {total_cpu}")
    print(f"Stockfish depth: {args.depth}")
    print()
    
    print("Fetching archives...")
    fetch_start = time.perf_counter()
    archives = fetch_archives(args.username)
    archives = list(reversed(archives))
    
    all_games = []
    for archive_url in archives:
        if len(all_games) >= args.games:
            break
        games = fetch_games(archive_url)
        all_games.extend(games)
        print(f"  Fetched {len(games)} games from {archive_url.split('/')[-2]}/{archive_url.split('/')[-1]}")
    
    all_games = all_games[:args.games]
    fetch_time = time.perf_counter() - fetch_start
    print(f"Fetched {len(all_games)} games in {fetch_time:.2f}s")
    print()
    
    print("Analyzing games...")
    analysis_start = time.perf_counter()
    
    results = []
    total_moves = 0
    
    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(analyze_game, game, args.username, args.threads, args.depth): i 
            for i, game in enumerate(all_games)
        }
        
        for i, future in enumerate(as_completed(futures)):
            result = future.result()
            if result:
                results.append(result)
                total_moves += result.total_moves
            
            if (i + 1) % 10 == 0 or i == len(all_games) - 1:
                elapsed = time.perf_counter() - analysis_start
                games_per_sec = (i + 1) / elapsed if elapsed > 0 else 0
                print(f"  Analyzed {i + 1}/{len(all_games)} games ({games_per_sec:.2f} games/sec)")
    
    analysis_time = time.perf_counter() - analysis_start
    
    target_lower = args.username.lower()
    user_accuracies = []
    for r in results:
        if r.white.lower() == target_lower:
            user_accuracies.append(r.white_accuracy)
        elif r.black.lower() == target_lower:
            user_accuracies.append(r.black_accuracy)
    
    avg_accuracy = sum(user_accuracies) / len(user_accuracies) if user_accuracies else 0
    
    print()
    print(f"Results")
    print(f"=" * 50)
    print(f"Games analyzed: {len(results)}")
    print(f"Total moves analyzed: {total_moves}")
    print(f"Average accuracy for {args.username}: {avg_accuracy:.2f}%")
    print()
    print(f"Performance")
    print(f"=" * 50)
    print(f"Fetch time: {fetch_time:.2f}s")
    print(f"Analysis time: {analysis_time:.2f}s")
    print(f"Total time: {fetch_time + analysis_time:.2f}s")
    print(f"Games per second: {len(results) / analysis_time:.4f}")
    print(f"Moves per second: {total_moves / analysis_time:.2f}")

if __name__ == "__main__":
    main()
