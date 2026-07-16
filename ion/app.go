package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx    context.Context
	cancel context.CancelFunc
	mu     sync.Mutex
	cmd    *exec.Cmd
}

func NewApp() *App {
	return &App{}
}

func (a *App) startup(ctx context.Context) {
	a.ctx, a.cancel = context.WithCancel(ctx)
}

func (a *App) shutdown(ctx context.Context) {
	if a.cancel != nil {
		a.cancel()
	}
	a.killCmd()
}

func (a *App) killCmd() {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.cmd != nil && a.cmd.Process != nil {
		_ = a.cmd.Process.Kill()
	}
}

func (a *App) setCmd(cmd *exec.Cmd) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.cmd = cmd
}

// findMiniosBinary locates the Rust core binary relative to the executable
// or the current working directory. It supports development and packaged layouts.
func findMiniosBinary() (string, error) {
	candidates := []string{}

	if exe, err := os.Executable(); err == nil {
		exePath, _ := filepath.EvalSymlinks(exe)
		exeDir := filepath.Dir(exePath)
		candidates = append(candidates,
			filepath.Join(exeDir, "..", "core", "target", "debug", "minios"),
			filepath.Join(exeDir, "..", "..", "core", "target", "debug", "minios"),
			filepath.Join(exeDir, "minios"),
		)
	}

	if cwd, err := os.Getwd(); err == nil {
		candidates = append(candidates,
			filepath.Join(cwd, "core", "target", "debug", "minios"),
			filepath.Join(cwd, "..", "core", "target", "debug", "minios"),
		)
	}

	for _, p := range candidates {
		if clean, err := filepath.Abs(p); err == nil {
			p = clean
		}
		if info, err := os.Stat(p); err == nil && !info.IsDir() {
			return p, nil
		}
	}

	// Last resort: PATH
	if pathBin, err := exec.LookPath("minios"); err == nil {
		return pathBin, nil
	}

	return "", errors.New("minios binary not found; build the Rust core or place minios on PATH")
}

// ProcessBackup runs a quick backup task synchronously.
func (a *App) ProcessBackup(agent string, caseName string) string {
	bin, err := findMiniosBinary()
	if err != nil {
		return fmt.Sprintf("Error: %s", err.Error())
	}

	cmd := exec.Command(bin, agent, caseName)
	cmd.Env = appendCaseRootsEnv(cmd.Env)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Sprintf("Error: %s\nOutput: %s", err.Error(), string(out))
	}
	return string(out)
}

// appendCaseRootsEnv adds the local cases directory to iON_CASE_ROOTS so the
// Rust core can find cases created by the UI even when cwd is not the project root.
func appendCaseRootsEnv(env []string) []string {
	if dir, err := casesDir(); err == nil {
		existing := ""
		for i, e := range env {
			if strings.HasPrefix(e, "iON_CASE_ROOTS=") {
				existing = strings.TrimPrefix(e, "iON_CASE_ROOTS=")
				env = append(env[:i], env[i+1:]...)
				break
			}
		}
		roots := []string{dir}
		if existing != "" {
			for _, p := range filepath.SplitList(existing) {
				if p != dir {
					roots = append(roots, p)
				}
			}
		}
		env = append(env, "iON_CASE_ROOTS="+strings.Join(roots, string(filepath.ListSeparator)))
	}
	return env
}

// StopExtraction cancels a running extraction.
func (a *App) StopExtraction() {
	a.killCmd()
}

// StartExtraction runs a long-running task with real-time JSON progress updates.
// If password is non-empty it is provided to the Rust core via the BACKUP_PASSWORD
// environment variable for encrypted backups.
func (a *App) StartExtraction(agent string, caseName string, password string) {
	go func() {
		bin, err := findMiniosBinary()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: "+err.Error())
			return
		}

		runtime.EventsEmit(a.ctx, "status", "Initializing Rust Core...")

		cmd := exec.Command(bin, agent, caseName, "--json")
		cmd.Env = appendCaseRootsEnv(cmd.Env)
		if strings.TrimSpace(password) != "" {
			// The Rust agent CLI currently expects encrypted-backup passwords here.
			// Keep this aligned with the working CLI contract; stdin-only attempts
			// have not worked reliably from the Wails launch path.
			cmd.Env = append(cmd.Env, "BACKUP_PASSWORD="+password)
		}

		stdout, err := cmd.StdoutPipe()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: failed to create stdout pipe: "+err.Error())
			return
		}

		stderr, err := cmd.StderrPipe()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: failed to create stderr pipe: "+err.Error())
			return
		}

		if err := cmd.Start(); err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: failed to start Rust core: "+err.Error())
			return
		}

		a.setCmd(cmd)

		// Forward stderr as raw log lines.
		go func() {
			scanner := bufio.NewScanner(stderr)
			for scanner.Scan() {
				runtime.EventsEmit(a.ctx, "log", "[stderr] "+scanner.Text())
			}
		}()

		scanner := bufio.NewScanner(stdout)
		for scanner.Scan() {
			line := scanner.Text()
			// Parse JSON and send events to the Svelte UI.
			var jsonEvent map[string]interface{}
			if err := json.Unmarshal([]byte(line), &jsonEvent); err == nil {
				if eventType, ok := jsonEvent["event"].(string); ok {
					runtime.EventsEmit(a.ctx, eventType, jsonEvent["payload"])
				}
			} else {
				// If not JSON, send as raw log.
				runtime.EventsEmit(a.ctx, "log", line)
			}
		}

		if err := scanner.Err(); err != nil && !errors.Is(err, io.ErrClosedPipe) {
			runtime.EventsEmit(a.ctx, "log", "[scanner error] "+err.Error())
		}

		if err := cmd.Wait(); err != nil {
			msg := err.Error()
			if exitErr, ok := err.(*exec.ExitError); ok {
				msg = strings.TrimSpace(string(exitErr.Stderr))
				if msg == "" {
					msg = fmt.Sprintf("Rust core exited with code %d", exitErr.ExitCode())
				}
			}
			runtime.EventsEmit(a.ctx, "status", "Error: "+msg)
			return
		}

		runtime.EventsEmit(a.ctx, "status", "Extraction Complete!")
	}()
}

func (a *App) Greet(name string) string {
	return fmt.Sprintf("Hello %s, STYGiON is ready.", name)
}

// findProjectRoot locates the iON project root by looking for core/Cargo.toml.
func findProjectRoot() (string, error) {
	const marker = "core" + string(filepath.Separator) + "Cargo.toml"

	if exe, err := os.Executable(); err == nil {
		exePath, _ := filepath.EvalSymlinks(exe)
		if root := searchUp(filepath.Dir(exePath), marker); root != "" {
			return root, nil
		}
	}

	if cwd, err := os.Getwd(); err == nil {
		if root := searchUp(cwd, marker); root != "" {
			return root, nil
		}
	}

	return "", errors.New("could not locate iON project root (missing core/Cargo.toml)")
}

func searchUp(start string, marker string) string {
	for dir := start; dir != "" && dir != string(filepath.Separator); dir = filepath.Dir(dir) {
		if _, err := os.Stat(filepath.Join(dir, marker)); err == nil {
			return dir
		}
	}
	return ""
}

// casesDir returns the directory where case folders are stored.
func casesDir() (string, error) {
	root, err := findProjectRoot()
	if err != nil {
		return "", err
	}
	return filepath.Join(root, "cases"), nil
}

type caseRegistry struct {
	Cases map[string]struct {
		Root string `json:"root"`
	} `json:"cases"`
}

// ListCases returns the names of existing case directories plus any cases
// remembered in ~/.iON/case_registry.json.
func (a *App) ListCases() ([]string, error) {
	seen := make(map[string]struct{})
	var cases []string

	add := func(name string) {
		if _, ok := seen[name]; ok {
			return
		}
		seen[name] = struct{}{}
		cases = append(cases, name)
	}

	// Local and well-known case roots.
	for _, root := range commonCaseRoots() {
		_ = os.MkdirAll(root, 0755)
		if entries, err := os.ReadDir(root); err == nil {
			for _, e := range entries {
				if e.IsDir() {
					add(e.Name())
				}
			}
		}
	}

	// Registered cases from the Rust core registry.
	home, err := os.UserHomeDir()
	if err == nil {
		registryPath := filepath.Join(home, ".iON", "case_registry.json")
		if data, err := os.ReadFile(registryPath); err == nil {
			var reg caseRegistry
			if err := json.Unmarshal(data, &reg); err == nil {
				for name := range reg.Cases {
					add(name)
				}
			}
		}
	}

	sort.Strings(cases)
	return cases, nil
}

// SelectBackupDirectory opens a native directory picker and returns the chosen path.
func (a *App) SelectBackupDirectory() (string, error) {
	return runtime.OpenDirectoryDialog(a.ctx, runtime.OpenDialogOptions{
		Title: "Select iOS Backup Directory",
	})
}

// CreateCaseFromBackup sets up a new case that points at an existing iOS backup.
// It symlinks the backup directory instead of copying it, so 90 GB+ backups are safe.
func (a *App) CreateCaseFromBackup(caseName string, backupPath string) (string, error) {
	if strings.TrimSpace(caseName) == "" {
		return "", errors.New("case name is required")
	}

	backupPath = filepath.Clean(backupPath)
	info, err := os.Stat(backupPath)
	if err != nil || !info.IsDir() {
		return "", fmt.Errorf("backup path is not a directory: %s", backupPath)
	}

	// Basic sanity check that this looks like an iOS backup.
	if _, err := os.Stat(filepath.Join(backupPath, "Manifest.db")); err != nil {
		if _, err := os.Stat(filepath.Join(backupPath, "Manifest.plist")); err != nil {
			return "", fmt.Errorf("backup does not contain Manifest.db or Manifest.plist; selected: %s", backupPath)
		}
	}

	dir, err := casesDir()
	if err != nil {
		return "", err
	}

	caseRoot := filepath.Join(dir, caseName)
	if err := os.MkdirAll(caseRoot, 0755); err != nil {
		return "", fmt.Errorf("create case root: %w", err)
	}

	// Remove any stale symlink/directory named backup.
	backupLink := filepath.Join(caseRoot, "backup")
	_ = os.Remove(backupLink)

	if err := os.Symlink(backupPath, backupLink); err != nil {
		return "", fmt.Errorf("symlink backup: %w", err)
	}

	// Ensure the standard subdirectories exist.
	for _, sub := range []string{"logs", "evidence", "intake", "clean"} {
		if err := os.MkdirAll(filepath.Join(caseRoot, sub), 0755); err != nil {
			return "", fmt.Errorf("create %s dir: %w", sub, err)
		}
	}

	// Register the case in the Rust core registry so it can be found regardless
	// of the working directory of the Wails process.
	if err := registerCaseRoot(caseName, caseRoot); err != nil {
		return "", fmt.Errorf("register case: %w", err)
	}

	return caseName, nil
}

func registryPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".iON", "case_registry.json"), nil
}

func registerCaseRoot(name string, root string) error {
	path, err := registryPath()
	if err != nil {
		return err
	}

	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}

	reg := caseRegistry{Cases: make(map[string]struct {
		Root string `json:"root"`
	})}

	if data, err := os.ReadFile(path); err == nil {
		_ = json.Unmarshal(data, &reg)
	}

	if reg.Cases == nil {
		reg.Cases = make(map[string]struct {
			Root string `json:"root"`
		})
	}

	reg.Cases[name] = struct {
		Root string `json:"root"`
	}{Root: root}

	data, err := json.MarshalIndent(reg, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(path, data, 0644)
}
