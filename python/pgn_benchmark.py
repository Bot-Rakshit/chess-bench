#!/usr/bin/env python3
"""Pure PGN Parsing Benchmark - Python Implementation"""

import requests
import chess
import chess.pgn
import io
import time
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed

def fetch_archives(username: str):
    url = f"https://api.chess.com/pub/player/{username}/games/archives"
    headers = {"User-Agent": "ChessBenchmark/1.0"}
    resp = requests.get(url, headers=headers, timeout=30)
    resp.raise_for_status()
    return resp.json().get("archives", [])

def fetch_games(archive_url: str):
    headers = {"User-Agent": "ChessBenchmark/1.0"}
    resp = requests.get(archive_url, headers=headers, timeout=60)
    resp.raise_for_status()
    return resp.json().get("games", [])

def parse_game(pgn_str: str):
    if not pgn_str:
        return 0, 0
    try:
        pgn = chess.pgn.read_game(io.StringIO(pgn_str))
        if not pgn:
            return 0, 0
        board = pgn.board()
        move_count = 0
        position_count = 1
        for move in pgn.mainline_moves():
            board.push(move)
            move_count += 1
            position_count += 1
            _ = board.fen()
        return move_count, position_count
    except:
        return 0, 0

def main():
    parser = argparse.ArgumentParser(description='Pure PGN Parsing Benchmark')
    parser.add_argument('username', nargs='?', default='hikaru')
    parser.add_argument('games', nargs='?', type=int, default=1000)
    parser.add_argument('--workers', type=int, default=4)
    args = parser.parse_args()

    print(f"Python PGN Parsing Benchmark")
    print(f"=" * 50)
    print(f"Library: python-chess")
    print(f"Username: {args.username}")
    print(f"Max games: {args.games}")
    print(f"Threads: {args.workers}")
    print()

    print("Fetching games...")
    fetch_start = time.perf_counter()
    archives = list(reversed(fetch_archives(args.username)))
    
    all_pgns = []
    for archive_url in archives:
        if len(all_pgns) >= args.games:
            break
        games = fetch_games(archive_url)
        for g in games:
            if g.get("pgn"):
                all_pgns.append(g["pgn"])
        print(f"  Fetched {len(games)} games from {archive_url.split('/')[-2]}/{archive_url.split('/')[-1]}")

    all_pgns = all_pgns[:args.games]
    fetch_time = time.perf_counter() - fetch_start
    print(f"Fetched {len(all_pgns)} games in {fetch_time:.2f}s")
    print()

    print("Parsing PGNs...")
    parse_start = time.perf_counter()
    
    total_moves = 0
    total_positions = 0
    games_parsed = 0

    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {executor.submit(parse_game, pgn): i for i, pgn in enumerate(all_pgns)}
        for i, future in enumerate(as_completed(futures)):
            moves, positions = future.result()
            if moves > 0:
                total_moves += moves
                total_positions += positions
                games_parsed += 1
            if (i + 1) % 100 == 0 or i == len(all_pgns) - 1:
                elapsed = time.perf_counter() - parse_start
                print(f"  Parsed {i + 1}/{len(all_pgns)} games ({(i + 1) / elapsed:.2f} games/sec)")

    parse_time = time.perf_counter() - parse_start

    print()
    print(f"Results")
    print(f"=" * 50)
    print(f"Games parsed: {games_parsed}")
    print(f"Total moves: {total_moves}")
    print(f"Total positions: {total_positions}")
    print()
    print(f"Performance")
    print(f"=" * 50)
    print(f"Parse time: {parse_time:.4f}s")
    print(f"Games per second: {games_parsed / parse_time:.2f}")
    print(f"Moves per second: {total_moves / parse_time:.2f}")

if __name__ == "__main__":
    main()
