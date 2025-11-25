package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/notnil/chess"
)

type ArchivesResp struct{ Archives []string `json:"archives"` }
type GamesResp struct{ Games []GameD `json:"games"` }
type GameD struct{ PGN string `json:"pgn"` }

func fetchArch(username string) []string {
	client := &http.Client{Timeout: 30 * time.Second}
	req, _ := http.NewRequest("GET", fmt.Sprintf("https://api.chess.com/pub/player/%s/games/archives", username), nil)
	req.Header.Set("User-Agent", "ChessBenchmark/1.0")
	resp, _ := client.Do(req)
	defer resp.Body.Close()
	var data ArchivesResp
	json.NewDecoder(resp.Body).Decode(&data)
	return data.Archives
}

func fetchG(url string) []GameD {
	client := &http.Client{Timeout: 60 * time.Second}
	req, _ := http.NewRequest("GET", url, nil)
	req.Header.Set("User-Agent", "ChessBenchmark/1.0")
	resp, _ := client.Do(req)
	defer resp.Body.Close()
	var data GamesResp
	json.NewDecoder(resp.Body).Decode(&data)
	return data.Games
}

func parseGame(pgn string) (int, int) {
	if pgn == "" {
		return 0, 0
	}
	pgnGame, err := chess.PGN(strings.NewReader(pgn))
	if err != nil {
		return 0, 0
	}
	game := chess.NewGame(pgnGame)
	moves := game.Moves()
	mc, pc := 0, 1
	pos := chess.NewGame()
	for _, mv := range moves {
		pos.Move(mv)
		mc++
		pc++
		_ = pos.Position().String()
	}
	return mc, pc
}

func main() {
	username := flag.String("username", "hikaru", "")
	maxGames := flag.Int("games", 1000, "")
	workers := flag.Int("workers", 4, "")
	flag.Parse()
	if flag.NArg() >= 1 {
		*username = flag.Arg(0)
	}
	if flag.NArg() >= 2 {
		fmt.Sscanf(flag.Arg(1), "%d", maxGames)
	}

	fmt.Println("Go PGN Parsing Benchmark")
	fmt.Println(strings.Repeat("=", 50))
	fmt.Printf("Library: notnil/chess\n")
	fmt.Printf("Username: %s\n", *username)
	fmt.Printf("Max games: %d\n", *maxGames)
	fmt.Printf("Workers: %d\n\n", *workers)

	fmt.Println("Fetching games...")
	fetchStart := time.Now()
	archives := fetchArch(*username)
	for i, j := 0, len(archives)-1; i < j; i, j = i+1, j-1 {
		archives[i], archives[j] = archives[j], archives[i]
	}

	var allPgns []string
	for _, url := range archives {
		if len(allPgns) >= *maxGames {
			break
		}
		games := fetchG(url)
		parts := strings.Split(url, "/")
		fmt.Printf("  Fetched %d games from %s/%s\n", len(games), parts[len(parts)-2], parts[len(parts)-1])
		for _, g := range games {
			if g.PGN != "" {
				allPgns = append(allPgns, g.PGN)
			}
		}
	}
	if len(allPgns) > *maxGames {
		allPgns = allPgns[:*maxGames]
	}
	fmt.Printf("Fetched %d games in %.2fs\n\n", len(allPgns), time.Since(fetchStart).Seconds())

	fmt.Println("Parsing PGNs...")
	parseStart := time.Now()
	var completed int64
	total := len(allPgns)

	type res struct{ m, p int }
	results := make(chan res, total)
	var wg sync.WaitGroup
	sem := make(chan struct{}, *workers)

	for _, pgn := range allPgns {
		wg.Add(1)
		go func(p string) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			m, pos := parseGame(p)
			results <- res{m, pos}
			c := atomic.AddInt64(&completed, 1)
			if c%100 == 0 || c == int64(total) {
				fmt.Printf("  Parsed %d/%d games (%.2f games/sec)\n", c, total, float64(c)/time.Since(parseStart).Seconds())
			}
		}(pgn)
	}
	go func() { wg.Wait(); close(results) }()

	var tm, tp, parsed int
	for r := range results {
		if r.m > 0 {
			tm += r.m
			tp += r.p
			parsed++
		}
	}
	parseTime := time.Since(parseStart)

	fmt.Println("\nResults")
	fmt.Println(strings.Repeat("=", 50))
	fmt.Printf("Games parsed: %d\n", parsed)
	fmt.Printf("Total moves: %d\n", tm)
	fmt.Println("\nPerformance")
	fmt.Println(strings.Repeat("=", 50))
	fmt.Printf("Parse time: %.4fs\n", parseTime.Seconds())
	fmt.Printf("Games per second: %.2f\n", float64(parsed)/parseTime.Seconds())
	fmt.Printf("Moves per second: %.2f\n", float64(tm)/parseTime.Seconds())
}
