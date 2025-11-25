package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/notnil/chess"
)

const StockfishPath = "/opt/homebrew/bin/stockfish"

type ArchivesResponse struct{ Archives []string `json:"archives"` }
type GamesResponse struct{ Games []GameData `json:"games"` }
type GameData struct {
	PGN   string      `json:"pgn"`
	White *PlayerData `json:"white"`
	Black *PlayerData `json:"black"`
}
type PlayerData struct{ Username string `json:"username"` }

type StockfishEngine struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	reader *bufio.Reader
	depth  int
}

func NewStockfishEngine(threads, depth int) (*StockfishEngine, error) {
	cmd := exec.Command(StockfishPath)
	stdin, _ := cmd.StdinPipe()
	stdout, _ := cmd.StdoutPipe()
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	// Use smaller buffer for lower latency (256 bytes like optimized Rust)
	e := &StockfishEngine{
		cmd:    cmd,
		stdin:  stdin,
		reader: bufio.NewReaderSize(stdout, 256),
		depth:  depth,
	}
	e.send("uci")
	e.waitForReady("uciok")
	e.send(fmt.Sprintf("setoption name Threads value %d", threads))
	e.send("setoption name UCI_ShowWDL value true")
	e.send("isready")
	e.waitForReady("readyok")
	return e, nil
}

func (e *StockfishEngine) send(cmd string) {
	io.WriteString(e.stdin, cmd+"\n")
}

// waitForReady waits for a token without parsing WDL
func (e *StockfishEngine) waitForReady(token string) {
	for {
		line, err := e.reader.ReadString('\n')
		if err != nil || strings.Contains(line, token) {
			return
		}
	}
}

// analyze sends position and returns WDL, parsing inline without storing lines
func (e *StockfishEngine) analyze(fen string) (int, int, int) {
	e.send("position fen " + fen)
	e.send(fmt.Sprintf("go depth %d", e.depth))

	w, d, l := 333, 334, 333

	for {
		line, err := e.reader.ReadString('\n')
		if err != nil {
			break
		}

		// Parse WDL inline - look for " wdl " pattern
		if idx := strings.Index(line, " wdl "); idx != -1 {
			// Extract the part after "wdl "
			rest := line[idx+5:]
			parts := strings.SplitN(rest, " ", 4)
			if len(parts) >= 3 {
				if v, err := strconv.Atoi(parts[0]); err == nil {
					w = v
				}
				if v, err := strconv.Atoi(parts[1]); err == nil {
					d = v
				}
				if v, err := strconv.Atoi(parts[2]); err == nil {
					l = v
				}
			}
		}

		// Check for bestmove to exit
		if strings.HasPrefix(line, "bestmove") {
			break
		}
	}

	return w, d, l
}

func (e *StockfishEngine) quit() {
	e.send("quit")
	e.cmd.Wait()
}

func wdlToProb(w, d, l int, isWhite bool) float64 {
	if !isWhite {
		w, l = l, w
	}
	return (float64(w) + float64(d)*0.5) / 1000.0
}

func calcAccuracy(before, after float64) float64 {
	if after >= before {
		return 100.0
	}
	acc := 100.0 * (1.0 - (before-after)*2.0)
	if acc < 0 {
		return 0
	}
	return acc
}

var httpClient = &http.Client{Timeout: 60 * time.Second}

func fetchArchives(username string) ([]string, error) {
	req, _ := http.NewRequest("GET", "https://api.chess.com/pub/player/"+username+"/games/archives", nil)
	req.Header.Set("User-Agent", "ChessBenchmark/1.0")
	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	var data ArchivesResponse
	json.NewDecoder(resp.Body).Decode(&data)
	return data.Archives, nil
}

func fetchGames(url string) ([]GameData, error) {
	req, _ := http.NewRequest("GET", url, nil)
	req.Header.Set("User-Agent", "ChessBenchmark/1.0")
	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	var data GamesResponse
	json.NewDecoder(resp.Body).Decode(&data)
	return data.Games, nil
}

func analyzeGame(g GameData, username string, sfThreads, depth int) (float64, float64, int, string, string, bool) {
	if g.PGN == "" {
		return 0, 0, 0, "", "", false
	}

	white, black := "", ""
	if g.White != nil {
		white = strings.ToLower(g.White.Username)
	}
	if g.Black != nil {
		black = strings.ToLower(g.Black.Username)
	}
	target := strings.ToLower(username)
	if white != target && black != target {
		return 0, 0, 0, "", "", false
	}

	// Parse PGN using notnil/chess (this is the slow part we can't optimize without changing library)
	pgnGame, err := chess.PGN(strings.NewReader(g.PGN))
	if err != nil {
		return 0, 0, 0, "", "", false
	}
	game := chess.NewGame(pgnGame)
	moves := game.Moves()

	if len(moves) == 0 {
		return 0, 0, 0, "", "", false
	}

	engine, err := NewStockfishEngine(sfThreads, depth)
	if err != nil {
		return 0, 0, 0, "", "", false
	}
	defer engine.quit()

	// Pre-allocate slices
	whiteAcc := make([]float64, 0, len(moves)/2+1)
	blackAcc := make([]float64, 0, len(moves)/2+1)

	pos := chess.NewGame()
	pw, pd, pl := engine.analyze(pos.Position().String())

	for _, mv := range moves {
		isWhite := pos.Position().Turn() == chess.White
		pos.Move(mv)

		cw, cd, cl := engine.analyze(pos.Position().String())
		acc := calcAccuracy(wdlToProb(pw, pd, pl, isWhite), wdlToProb(cw, cd, cl, isWhite))

		if isWhite {
			whiteAcc = append(whiteAcc, acc)
		} else {
			blackAcc = append(blackAcc, acc)
		}
		pw, pd, pl = cw, cd, cl
	}

	wa, ba := 0.0, 0.0
	if len(whiteAcc) > 0 {
		for _, a := range whiteAcc {
			wa += a
		}
		wa /= float64(len(whiteAcc))
	}
	if len(blackAcc) > 0 {
		for _, a := range blackAcc {
			ba += a
		}
		ba /= float64(len(blackAcc))
	}

	return wa, ba, len(whiteAcc) + len(blackAcc), white, black, true
}

func main() {
	username := flag.String("username", "hikaru", "Chess.com username")
	maxGames := flag.Int("games", 1000, "Max games")
	workers := flag.Int("workers", 4, "Number of workers")
	sfThreads := flag.Int("threads", 1, "SF threads per worker")
	depth := flag.Int("depth", 4, "Stockfish depth")
	flag.Parse()

	if flag.NArg() >= 1 {
		*username = flag.Arg(0)
	}
	if flag.NArg() >= 2 {
		fmt.Sscanf(flag.Arg(1), "%d", maxGames)
	}

	fmt.Println("Go Chess Benchmark")
	fmt.Println(strings.Repeat("=", 50))
	fmt.Printf("Username: %s\n", *username)
	fmt.Printf("Max games: %d\n", *maxGames)
	fmt.Printf("Workers: %d\n", *workers)
	fmt.Printf("SF threads/worker: %d\n", *sfThreads)
	fmt.Printf("Total CPU: %d\n", *workers**sfThreads)
	fmt.Printf("Depth: %d\n", *depth)
	fmt.Println()

	fmt.Println("Fetching archives...")
	fetchStart := time.Now()
	archives, _ := fetchArchives(*username)
	for i, j := 0, len(archives)-1; i < j; i, j = i+1, j-1 {
		archives[i], archives[j] = archives[j], archives[i]
	}

	var allGames []GameData
	for _, url := range archives {
		if len(allGames) >= *maxGames {
			break
		}
		games, _ := fetchGames(url)
		parts := strings.Split(url, "/")
		fmt.Printf("  Fetched %d games from %s/%s\n", len(games), parts[len(parts)-2], parts[len(parts)-1])
		allGames = append(allGames, games...)
	}
	if len(allGames) > *maxGames {
		allGames = allGames[:*maxGames]
	}
	fetchTime := time.Since(fetchStart)
	fmt.Printf("Fetched %d games in %.2fs\n\n", len(allGames), fetchTime.Seconds())

	fmt.Println("Analyzing games...")
	analysisStart := time.Now()
	var completed int64
	total := len(allGames)

	type result struct {
		wa, ba       float64
		moves        int
		white, black string
		ok           bool
	}
	results := make(chan result, total)
	var wg sync.WaitGroup
	sem := make(chan struct{}, *workers)

	for _, g := range allGames {
		wg.Add(1)
		go func(game GameData) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()

			wa, ba, m, w, b, ok := analyzeGame(game, *username, *sfThreads, *depth)
			results <- result{wa, ba, m, w, b, ok}

			c := atomic.AddInt64(&completed, 1)
			if c%10 == 0 || c == int64(total) {
				fmt.Printf("  Analyzed %d/%d games (%.2f games/sec)\n", c, total, float64(c)/time.Since(analysisStart).Seconds())
			}
		}(g)
	}
	go func() { wg.Wait(); close(results) }()

	var userAcc []float64
	var totalMoves, analyzed int
	target := strings.ToLower(*username)
	for r := range results {
		if r.ok {
			analyzed++
			totalMoves += r.moves
			if r.white == target {
				userAcc = append(userAcc, r.wa)
			} else {
				userAcc = append(userAcc, r.ba)
			}
		}
	}
	analysisTime := time.Since(analysisStart)

	avg := 0.0
	if len(userAcc) > 0 {
		for _, a := range userAcc {
			avg += a
		}
		avg /= float64(len(userAcc))
	}

	fmt.Println("\nResults")
	fmt.Println(strings.Repeat("=", 50))
	fmt.Printf("Games analyzed: %d\n", analyzed)
	fmt.Printf("Total moves: %d\n", totalMoves)
	fmt.Printf("Average accuracy for %s: %.2f%%\n", *username, avg)
	fmt.Println("\nPerformance")
	fmt.Println(strings.Repeat("=", 50))
	fmt.Printf("Fetch time: %.2fs\n", fetchTime.Seconds())
	fmt.Printf("Analysis time: %.2fs\n", analysisTime.Seconds())
	fmt.Printf("Total time: %.2fs\n", fetchTime.Seconds()+analysisTime.Seconds())
	fmt.Printf("Games per second: %.4f\n", float64(analyzed)/analysisTime.Seconds())
	fmt.Printf("Moves per second: %.2f\n", float64(totalMoves)/analysisTime.Seconds())
}
