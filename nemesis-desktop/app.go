package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
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
	serverURL := os.Getenv("iON_DESKTOP_SERVER_URL")
	if serverURL == "" {
		serverURL = "http://127.0.0.1:17870"
	}
	return &App{
		serverURL: serverURL,
	}
}

func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
	if err := a.ensureDesktopServer(); err != nil {
		println("desktop server startup failed:", err.Error())
	}
}

func (a *App) shutdown(ctx context.Context) {
	if a.cmd == nil || a.cmd.Process == nil {
		return
	}

	// Try graceful termination first, then force kill.
	_ = a.cmd.Process.Signal(os.Interrupt)
	done := make(chan struct{})
	go func() {
		_ = a.cmd.Wait()
		close(done)
	}()

	select {
	case <-done:
		return
	case <-time.After(3 * time.Second):
		_ = a.cmd.Process.Kill()
		<-done
	}
}

func (a *App) GetBootstrapConfig() BootstrapConfig {
	return BootstrapConfig{ServerURL: a.serverURL}
}

func (a *App) ensureDesktopServer() error {
	if serverHealthy(a.serverURL) {
		println("desktop server already healthy at", a.serverURL)
		return nil
	}

	projectRoot, err := findProjectRoot()
	if err != nil {
		return err
	}

	manifestPath := filepath.Join(projectRoot, "core", "Cargo.toml")
	if _, err := os.Stat(manifestPath); err != nil {
		return fmt.Errorf("core manifest not found at %s: %w", manifestPath, err)
	}

	bindAddr := strings.TrimPrefix(a.serverURL, "http://")
	bindAddr = strings.TrimPrefix(bindAddr, "https://")

	println("starting desktop server from", projectRoot, "on", bindAddr)
	a.cmd = exec.Command(
		"cargo",
		"run",
		"--manifest-path",
		manifestPath,
		"--bin",
		"minios",
		"--",
		"desktop-serve",
		bindAddr,
	)
	a.cmd.Dir = projectRoot
	a.cmd.Stderr = os.Stderr

	if err := a.cmd.Start(); err != nil {
		return fmt.Errorf("start Rust desktop server: %w", err)
	}

	deadline := time.Now().Add(20 * time.Second)
	for time.Now().Before(deadline) {
		if serverHealthy(a.serverURL) {
			println("desktop server healthy at", a.serverURL)
			return nil
		}
		time.Sleep(300 * time.Millisecond)
	}

	return errors.New("Rust desktop server did not become healthy before timeout")
}

// findProjectRoot searches upward from the executable and the current working
// directory for the iON project root, identified by the presence of
// core/Cargo.toml. This keeps the desktop host working when the binary is
// launched from unexpected directories.
func findProjectRoot() (string, error) {
	const marker = "core" + string(filepath.Separator) + "Cargo.toml"

	if exe, err := os.Executable(); err == nil {
		exePath, err := filepath.EvalSymlinks(exe)
		if err != nil {
			exePath = exe
		}
		if root := searchUp(filepath.Dir(exePath), marker); root != "" {
			return root, nil
		}
	}

	if cwd, err := os.Getwd(); err == nil {
		if root := searchUp(cwd, marker); root != "" {
			return root, nil
		}
	}

	return "", errors.New("could not locate project root (missing core/Cargo.toml)")
}

func searchUp(start string, marker string) string {
	for dir := start; dir != "" && dir != string(filepath.Separator); dir = filepath.Dir(dir) {
		if _, err := os.Stat(filepath.Join(dir, marker)); err == nil {
			return dir
		}
	}
	return ""
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
