#!/usr/bin/env node
import { Chess } from 'chess.js';
import { parseArgs } from 'util';

function parsePgnMoves(pgn) {
  const match = pgn.match(/\n\n([\s\S]+)$/);
  if (!match) return [];
  let str = match[1].replace(/\{[^}]*\}/g, '').replace(/\([^)]*\)/g, '')
    .replace(/\d+\.\.\./g, '').replace(/\d+\./g, '').replace(/(1-0|0-1|1\/2-1\/2|\*)$/g, '');
  return str.trim().split(/\s+/).filter(m => m && !m.match(/^[\d.]+$/));
}

async function fetchArchives(username) {
  const resp = await fetch(`https://api.chess.com/pub/player/${username}/games/archives`,
    { headers: { 'User-Agent': 'ChessBenchmark/1.0' } });
  return (await resp.json()).archives || [];
}

async function fetchGames(url) {
  const resp = await fetch(url, { headers: { 'User-Agent': 'ChessBenchmark/1.0' } });
  return (await resp.json()).games || [];
}

function parseGame(pgn) {
  if (!pgn) return { moves: 0, positions: 0 };
  try {
    const chess = new Chess();
    const moves = parsePgnMoves(pgn);
    let moveCount = 0, posCount = 1;
    for (const m of moves) {
      try { chess.move(m); moveCount++; posCount++; chess.fen(); } catch { break; }
    }
    return { moves: moveCount, positions: posCount };
  } catch { return { moves: 0, positions: 0 }; }
}

async function main() {
  const { positionals } = parseArgs({ allowPositionals: true, options: {} });
  const username = positionals[0] || 'hikaru';
  const maxGames = parseInt(positionals[1]) || 1000;

  console.log('Node.js PGN Parsing Benchmark');
  console.log('='.repeat(50));
  console.log(`Library: chess.js`);
  console.log(`Username: ${username}`);
  console.log(`Max games: ${maxGames}\n`);

  console.log('Fetching games...');
  const fetchStart = performance.now();
  const archives = (await fetchArchives(username)).reverse();
  
  const allPgns = [];
  for (const url of archives) {
    if (allPgns.length >= maxGames) break;
    const games = await fetchGames(url);
    games.forEach(g => g.pgn && allPgns.push(g.pgn));
    console.log(`  Fetched ${games.length} games from ${url.split('/').slice(-2).join('/')}`);
  }

  const pgns = allPgns.slice(0, maxGames);
  console.log(`Fetched ${pgns.length} games in ${((performance.now() - fetchStart)/1000).toFixed(2)}s\n`);

  console.log('Parsing PGNs...');
  const parseStart = performance.now();
  let totalMoves = 0, totalPos = 0, parsed = 0;

  for (let i = 0; i < pgns.length; i += 100) {
    const batch = pgns.slice(i, i + 100);
    for (const pgn of batch) {
      const { moves, positions } = parseGame(pgn);
      if (moves > 0) { totalMoves += moves; totalPos += positions; parsed++; }
    }
    const elapsed = (performance.now() - parseStart) / 1000;
    console.log(`  Parsed ${Math.min(i + 100, pgns.length)}/${pgns.length} games (${((i+100)/elapsed).toFixed(2)} games/sec)`);
  }

  const parseTime = (performance.now() - parseStart) / 1000;
  console.log('\nResults');
  console.log('='.repeat(50));
  console.log(`Games parsed: ${parsed}`);
  console.log(`Total moves: ${totalMoves}`);
  console.log('\nPerformance');
  console.log('='.repeat(50));
  console.log(`Parse time: ${parseTime.toFixed(4)}s`);
  console.log(`Games per second: ${(parsed / parseTime).toFixed(2)}`);
  console.log(`Moves per second: ${(totalMoves / parseTime).toFixed(2)}`);
}

main().catch(console.error);
