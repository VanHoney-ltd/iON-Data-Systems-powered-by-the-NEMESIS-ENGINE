package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os/exec"

	"github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx context.Context
}

func NewApp() *App {
	return &App{}
}

func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
}

// ProcessBackup runs a quick backup task
func (a *App) ProcessBackup(agent string, caseName string) string {
	cmd := exec.Command("../core/target/debug/minios", agent, caseName)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Sprintf("Error: %s\nOutput: %s", err.Error(), string(out))
	}
	return string(out)
}

// StartExtraction runs a long-running task with real-time JSON progress updates
func (a *App) StartExtraction(agent string, caseName string) {
	go func() {
		runtime.EventsEmit(a.ctx, "status", "Initializing Rust Core...")

		cmd := exec.Command("../core/target/debug/minios", agent, caseName, "--json")

		// This captures the output line-by-line instead of waiting for the end
		stdout, _ := cmd.StdoutPipe()
		cmd.Start()

		scanner := bufio.NewScanner(stdout)
		for scanner.Scan() {
			line := scanner.Text()
			// Parse JSON and send events to the Svelte UI
			var jsonEvent map[string]interface{}
			if err := json.Unmarshal([]byte(line), &jsonEvent); err == nil {
				if eventType, ok := jsonEvent["event"].(string); ok {
					runtime.EventsEmit(a.ctx, eventType, jsonEvent["payload"])
				}
			} else {
				// If not JSON, send as raw log
				runtime.EventsEmit(a.ctx, "log", line)
			}
		}

		cmd.Wait()
		runtime.EventsEmit(a.ctx, "status", "Extraction Complete!")
	}()
}

func (a *App) Greet(name string) string {
	return fmt.Sprintf("Hello %s, STYGiON is ready.", name)
}
