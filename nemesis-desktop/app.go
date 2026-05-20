package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"os/exec"
	"path/filepath"
	"time"
)

// App is the thin Wails host. It owns desktop lifecycle only.
type App struct {
	ctx       context.Context
	serverURL string
	cmd       *exec.Cmd
}

type BootstrapConfig struct {
	ServerURL string `json:"serverUrl"`
}

func NewApp() *App {
	return &App{
		serverURL: "http://127.0.0.1:17870",
	}
}

func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
	if err := a.ensureDesktopServer(); err != nil {
		println("desktop server startup failed:", err.Error())
	}
}

func (a *App) shutdown(ctx context.Context) {
	if a.cmd != nil && a.cmd.Process != nil {
		_ = a.cmd.Process.Kill()
	}
}

func (a *App) GetBootstrapConfig() BootstrapConfig {
	return BootstrapConfig{ServerURL: a.serverURL}
}

func (a *App) ensureDesktopServer() error {
	if serverHealthy(a.serverURL) {
		return nil
	}

	manifestPath, err := filepath.Abs("../core/Cargo.toml")
	if err != nil {
		return err
	}

	a.cmd = exec.Command(
		"cargo",
		"run",
		"--manifest-path",
		manifestPath,
		"--bin",
		"minios",
		"--",
		"desktop-serve",
		"127.0.0.1:17870",
	)
	a.cmd.Dir = filepath.Dir(filepath.Dir(manifestPath))

	if err := a.cmd.Start(); err != nil {
		return fmt.Errorf("start Rust desktop server: %w", err)
	}

	deadline := time.Now().Add(20 * time.Second)
	for time.Now().Before(deadline) {
		if serverHealthy(a.serverURL) {
			return nil
		}
		time.Sleep(300 * time.Millisecond)
	}

	return errors.New("Rust desktop server did not become healthy before timeout")
}

func serverHealthy(baseURL string) bool {
	client := http.Client{Timeout: 500 * time.Millisecond}
	resp, err := client.Get(baseURL + "/api/health")
	if err != nil {
		return false
	}
	defer resp.Body.Close()
	return resp.StatusCode == http.StatusOK
}
