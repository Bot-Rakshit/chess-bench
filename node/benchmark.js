#!/usr/bin/env node
import { Chess } from 'chess.js';
import { spawn } from 'child_process';
import { createInterface } from 'readline';
import { parseArgs } from 'util';

const STOCKFISH_PATH = '/opt/homebrew/bin/stockfish';

class StockfishEngine {
  constructor(threads = 1, depth = 4) {
    this.depth = depth;
    this.threads = threads;
    this.process = null;
    this.rl = null;
    this.pendingResolve = null;
    this.lines = [];
  }

  async init() {
    return new Promise((resolve, reject) => {
      this.process = spawn(STOCKFISH_PATH, [], { stdio: ['pipe', 'pipe', 'ignore'] });
      this.rl = createInterface({ input: this.process.stdout });
      this.rl.on('line', (line) => this.handleLine(line));
      this.send('uci');
      this.waitFor('uciok').then(() => {
        this.send(`setoption name Threads value ${this.threads}`);
        this.send('setoption name UCI_ShowWDL value true');
        this.send('isready');
        return this.waitFor('readyok');
      }).then(resolve).catch(reject);
    });
  }

  send(cmd) { this.process.stdin.write(cmd + '\n'); }

  handleLine(line) {
    this.lines.push(line);
    if (this.pendingResolve && line.includes(this.waitToken)) {
      this.pendingResolve([...this.lines]);
      this.pendingResolve = null;
      this.lines = [];
    }
  }

  waitFor(token) {
    return new Promise((resolve) => {
      this.waitToken = token;
      this.pendingResolve = resolve;
    });
  }

  async analyze(fen) {
    this.send(`position fen ${fen}`);
    this.send(`go depth ${this.depth}`);
    const lines = await this.waitFor('bestmove');
    let wdl = [333, 334, 333];
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].includes('wdl')) {
        const parts = lines[i].split(' ');
        const idx = parts.indexOf('wdl');
        if (idx !== -1 && idx + 3 < parts.length) {
          wdl = [parseInt(parts[idx + 1]), parseInt(parts[idx + 2]), parseInt(parts[idx + 3])];
        }
        break;
      }
    }
    return wdl;
  }

  quit() { try { this.send('quit'); this.process.kill(); } catch (e) {} }
}

function wdlToWinProb(wdl, isWhite) {
  let [w, d, l] = wdl;
  if (!isWhite) [w, l] = [l, w];
  return (w + d * 0.5) / 1000.0;
}

function calculateAccuracy(probBefore, probAfter) {
  if (probAfter >= probBefore) return 100.0;
  return Math.max(0, 100 * (1 - (probBefore - probAfter) * 2));
}

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

async function analyzeGame(gameData, username, sfThreads, depth) {
  if (!gameData.pgn) return null;
  try {
    const chess = new Chess();
    const engine = new StockfishEngine(sfThreads, depth);
    await engine.init();

    const whiteUser = (gameData.white?.username || '').toLowerCase();
    const blackUser = (gameData.black?.username || '').toLowerCase();
    const target = username.toLowerCase();
    if (whiteUser !== target && blackUser !== target) { engine.quit(); return null; }

    const moves = parsePgnMoves(gameData.pgn);
    const whiteAcc = [], blackAcc = [];
    let prevWdl = await engine.analyze(chess.fen());

    for (const m of moves) {
      try {
        const isWhite = chess.turn() === 'w';
        chess.move(m);
        const currWdl = await engine.analyze(chess.fen());
        const acc = calculateAccuracy(wdlToWinProb(prevWdl, isWhite), wdlToWinProb(currWdl, isWhite));
        (isWhite ? whiteAcc : blackAcc).push(acc);
        prevWdl = currWdl;
      } catch { break; }
    }
    engine.quit();

    return {
      white: whiteUser, black: blackUser,
      whiteAccuracy: whiteAcc.length ? whiteAcc.reduce((a,b)=>a+b,0)/whiteAcc.length : 0,
      blackAccuracy: blackAcc.length ? blackAcc.reduce((a,b)=>a+b,0)/blackAcc.length : 0,
      totalMoves: whiteAcc.length + blackAcc.length
    };
  } catch { return null; }
}

async function processGames(games, username, workers, sfThreads, depth) {
  const results = [];
  let completed = 0, totalMoves = 0;
  const startTime = performance.now();

  for (let i = 0; i < games.length; i += workers) {
    const batch = games.slice(i, i + workers);
    const batchResults = await Promise.all(batch.map(g => analyzeGame(g, username, sfThreads, depth)));
    for (const r of batchResults) {
      if (r) { results.push(r); totalMoves += r.totalMoves; }
      completed++;
    }
    const elapsed = (performance.now() - startTime) / 1000;
    console.log(`  Analyzed ${completed}/${games.length} games (${(completed/elapsed).toFixed(2)} games/sec)`);
  }
  return { results, totalMoves };
}

async function main() {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      workers: { type: 'string', default: '4' },
      threads: { type: 'string', default: '1' },
      depth: { type: 'string', default: '4' }
    }
  });

  const username = positionals[0] || 'hikaru';
  const maxGames = parseInt(positionals[1]) || 1000;
  const workers = parseInt(values.workers);
  const sfThreads = parseInt(values.threads);
  const depth = parseInt(values.depth);

  console.log('Node.js Chess Benchmark');
  console.log('='.repeat(50));
  console.log(`Username: ${username}`);
  console.log(`Max games: ${maxGames}`);
  console.log(`Workers: ${workers}`);
  console.log(`SF threads/worker: ${sfThreads}`);
  console.log(`Total CPU: ${workers * sfThreads}`);
  console.log(`Depth: ${depth}`);
  console.log();

  console.log('Fetching archives...');
  const fetchStart = performance.now();
  let archives = (await fetchArchives(username)).reverse();
  
  const allGames = [];
  for (const url of archives) {
    if (allGames.length >= maxGames) break;
    const games = await fetchGames(url);
    allGames.push(...games);
    console.log(`  Fetched ${games.length} games from ${url.split('/').slice(-2).join('/')}`);
  }

  const gamesToAnalyze = allGames.slice(0, maxGames);
  const fetchTime = (performance.now() - fetchStart) / 1000;
  console.log(`Fetched ${gamesToAnalyze.length} games in ${fetchTime.toFixed(2)}s\n`);

  console.log('Analyzing games...');
  const analysisStart = performance.now();
  const { results, totalMoves } = await processGames(gamesToAnalyze, username, workers, sfThreads, depth);
  const analysisTime = (performance.now() - analysisStart) / 1000;

  const target = username.toLowerCase();
  const userAcc = results.map(r => r.white === target ? r.whiteAccuracy : r.blackAccuracy);
  const avgAcc = userAcc.length ? userAcc.reduce((a,b)=>a+b,0)/userAcc.length : 0;

  console.log('\nResults');
  console.log('='.repeat(50));
  console.log(`Games analyzed: ${results.length}`);
  console.log(`Total moves: ${totalMoves}`);
  console.log(`Average accuracy for ${username}: ${avgAcc.toFixed(2)}%`);
  console.log('\nPerformance');
  console.log('='.repeat(50));
  console.log(`Fetch time: ${fetchTime.toFixed(2)}s`);
  console.log(`Analysis time: ${analysisTime.toFixed(2)}s`);
  console.log(`Total time: ${(fetchTime + analysisTime).toFixed(2)}s`);
  console.log(`Games per second: ${(results.length / analysisTime).toFixed(4)}`);
  console.log(`Moves per second: ${(totalMoves / analysisTime).toFixed(2)}`);
}

main().catch(console.error);
