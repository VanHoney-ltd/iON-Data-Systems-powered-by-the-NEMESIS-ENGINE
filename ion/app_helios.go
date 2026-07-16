package main

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/wailsapp/wails/v2/pkg/runtime"
)

// findHeliosBinary locates the HELiOS decryption binary.
func findHeliosBinary() (string, error) {
	candidates := []string{}

	if exe, err := os.Executable(); err == nil {
		exePath, _ := filepath.EvalSymlinks(exe)
		exeDir := filepath.Dir(exePath)
		candidates = append(candidates,
			filepath.Join(exeDir, "ion-helios"),
			filepath.Join(exeDir, "helios"),
			filepath.Join(exeDir, "..", "build", "bin", "ion-helios"),
			filepath.Join(exeDir, "..", "HELiOS", "target", "release", "helios"),
			filepath.Join(exeDir, "..", "HELiOS", "target", "debug", "helios"),
			filepath.Join(exeDir, "..", "core", "target", "debug", "helios"),
			filepath.Join(exeDir, "..", "..", "core", "target", "debug", "helios"),
			filepath.Join(exeDir, "..", "..", "HELiOS", "target", "release", "helios"),
			filepath.Join(exeDir, "..", "..", "HELiOS", "target", "debug", "helios"),
		)
	}

	if cwd, err := os.Getwd(); err == nil {
		candidates = append(candidates,
			filepath.Join(cwd, "build", "bin", "ion-helios"),
			filepath.Join(cwd, "HELiOS", "target", "release", "helios"),
			filepath.Join(cwd, "HELiOS", "target", "debug", "helios"),
			filepath.Join(cwd, "..", "build", "bin", "ion-helios"),
			filepath.Join(cwd, "..", "HELiOS", "target", "release", "helios"),
			filepath.Join(cwd, "..", "HELiOS", "target", "debug", "helios"),
			filepath.Join(cwd, "core", "target", "debug", "helios"),
			filepath.Join(cwd, "..", "core", "target", "debug", "helios"),
		)
	}

	for _, p := range candidates {
		if clean, err := filepath.Abs(p); err == nil {
			p = clean
		}
		if info, err := os.Stat(p); err == nil && !info.IsDir() && isUsableHeliosBinary(p) {
			return p, nil
		}
	}

	if pathBin, err := exec.LookPath("helios"); err == nil {
		if isUsableHeliosBinary(pathBin) {
			return pathBin, nil
		}
	}

	return "", fmt.Errorf("helios binary not found; build the Rust core or place helios on PATH")
}

func isUsableHeliosBinary(path string) bool {
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		resolved = path
	}
	out, err := exec.Command(resolved, "--help").CombinedOutput()
	if err != nil {
		return false
	}
	help := string(out)
	return strings.Contains(help, "HELiOS - encrypted iOS backup decrypt/reconstruct CLI")
}

func resolveIOSBackupRoot(path string) (string, error) {
	clean := filepath.Clean(path)
	info, err := os.Stat(clean)
	if err != nil || !info.IsDir() {
		return "", fmt.Errorf("backup path is not a directory: %s", clean)
	}

	if isIOSBackupRoot(clean) {
		return clean, nil
	}

	backupChild := filepath.Join(clean, "backup")
	if isIOSBackupRoot(backupChild) {
		return backupChild, nil
	}

	entries, err := os.ReadDir(clean)
	if err != nil {
		return "", fmt.Errorf("read backup path: %w", err)
	}

	var candidates []string
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		candidate := filepath.Join(clean, entry.Name())
		if isIOSBackupRoot(candidate) {
			candidates = append(candidates, candidate)
		}
	}
	if len(candidates) == 0 {
		return "", fmt.Errorf("backup does not contain Manifest.db or Manifest.plist; selected: %s", clean)
	}

	sort.Slice(candidates, func(i, j int) bool {
		return modifiedTime(candidates[i]).Before(modifiedTime(candidates[j]))
	})
	return candidates[len(candidates)-1], nil
}

func isIOSBackupRoot(path string) bool {
	if info, err := os.Stat(path); err != nil || !info.IsDir() {
		return false
	}
	if _, err := os.Stat(filepath.Join(path, "Manifest.plist")); err != nil {
		return false
	}
	if _, err := os.Stat(filepath.Join(path, "Info.plist")); err != nil {
		return false
	}
	if _, err := os.Stat(filepath.Join(path, "Status.plist")); err != nil {
		return false
	}
	return true
}

func modifiedTime(path string) time.Time {
	info, err := os.Stat(path)
	if err != nil {
		return time.Time{}
	}
	return info.ModTime()
}

// DecryptBackup runs HELiOS to decrypt an encrypted iOS backup into a prepared
// output tree that the forensic agents can then read.
func (a *App) DecryptBackup(caseName string, backupPath string, password string, profile string) {
	go func() {
		bin, err := findHeliosBinary()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: "+err.Error())
			runtime.EventsEmit(a.ctx, "log", "[helios] "+err.Error())
			return
		}

		backupPath = filepath.Clean(backupPath)
		resolvedBackupPath, err := resolveIOSBackupRoot(backupPath)
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: "+err.Error())
			runtime.EventsEmit(a.ctx, "log", "[helios] "+err.Error())
			return
		}

		if strings.TrimSpace(password) == "" {
			runtime.EventsEmit(a.ctx, "status", "Error: password is required")
			return
		}

		profile = strings.TrimSpace(profile)
		if profile == "" {
			profile = "full"
		}

		runtime.EventsEmit(a.ctx, "status", "Starting HELiOS decryption...")
		runtime.EventsEmit(a.ctx, "log", fmt.Sprintf("[helios] Decrypting %s with profile %s", caseName, profile))
		runtime.EventsEmit(a.ctx, "log", fmt.Sprintf("[helios] Using binary %s", bin))
		if resolvedBackupPath != backupPath {
			runtime.EventsEmit(a.ctx, "log", fmt.Sprintf("[helios] Using backup root %s", resolvedBackupPath))
		}

		// HELiOS currently accepts backup passwords through this CLI surface.
		// Keep this aligned with the working Rust CLI contract; previous
		// stdin/env-only handoffs did not work reliably from Wails.
		args := []string{caseName, "-i", resolvedBackupPath, "-p", password, "--profile", profile}
		cmd := exec.Command(bin, args...)

		stdout, err := cmd.StdoutPipe()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: failed to create stdout pipe")
			return
		}
		stderr, err := cmd.StderrPipe()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: failed to create stderr pipe")
			return
		}

		if err := cmd.Start(); err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: failed to start helios: "+err.Error())
			return
		}

		a.setCmd(cmd)

		go func() {
			if err := readChronosLines(stderr, func(line string) {
				runtime.EventsEmit(a.ctx, "log", "[helios] "+line)
			}); err != nil && err != io.ErrClosedPipe {
				runtime.EventsEmit(a.ctx, "log", "[scanner error] "+err.Error())
			}
		}()

		if err := readChronosLines(stdout, func(line string) {
			runtime.EventsEmit(a.ctx, "log", "[helios] "+line)
		}); err != nil && err != io.ErrClosedPipe {
			runtime.EventsEmit(a.ctx, "log", "[scanner error] "+err.Error())
		}

		if err := cmd.Wait(); err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: HELiOS decryption failed: "+err.Error())
			return
		}

		// Register the prepared case root so agents can discover it.
		home, _ := os.UserHomeDir()
		preparedRoot := filepath.Join(home, "iON", "prepared", "helios", caseName)
		if info, err := os.Stat(preparedRoot); err == nil && info.IsDir() {
			_ = registerCaseRoot(caseName, preparedRoot)
		}

		runtime.EventsEmit(a.ctx, "status", "HELiOS decryption complete!")
		runtime.EventsEmit(a.ctx, "case-created", caseName)
	}()
}
