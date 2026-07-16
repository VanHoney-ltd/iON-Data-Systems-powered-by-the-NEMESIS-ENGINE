package main

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/wailsapp/wails/v2/pkg/runtime"
)

var (
	percentPattern = regexp.MustCompile(`(?i)(overall|total|file|files|progress)?[^0-9]{0,24}([0-9]{1,3})(?:\.[0-9]+)?\s*%`)
	ansiPattern    = regexp.MustCompile(`\x1b\[[0-9;]*[A-Za-z]`)
)

type acquisitionProgress struct {
	OverallPercent int    `json:"overallPercent"`
	FilePercent    int    `json:"filePercent"`
	Label          string `json:"label"`
	Raw            string `json:"raw"`
}

// findChronosBinary locates the chronos acquisition binary.
func findChronosBinary() (string, error) {
	candidates := []string{}

	if exe, err := os.Executable(); err == nil {
		exePath, _ := filepath.EvalSymlinks(exe)
		exeDir := filepath.Dir(exePath)
		candidates = append(candidates,
			filepath.Join(exeDir, "..", "core", "target", "release", "chronos"),
			filepath.Join(exeDir, "..", "core", "target", "debug", "chronos"),
			filepath.Join(exeDir, "..", "..", "core", "target", "release", "chronos"),
			filepath.Join(exeDir, "..", "..", "core", "target", "debug", "chronos"),
			filepath.Join(exeDir, "chronos"),
		)
	}

	if cwd, err := os.Getwd(); err == nil {
		candidates = append(candidates,
			filepath.Join(cwd, "core", "target", "release", "chronos"),
			filepath.Join(cwd, "core", "target", "debug", "chronos"),
			filepath.Join(cwd, "..", "core", "target", "release", "chronos"),
			filepath.Join(cwd, "..", "core", "target", "debug", "chronos"),
		)
	}

	for _, p := range candidates {
		if clean, err := filepath.Abs(p); err == nil {
			p = clean
		}
		if info, err := os.Stat(p); err == nil && !info.IsDir() && isUsableChronosBinary(p) {
			return p, nil
		}
	}

	if pathBin, err := exec.LookPath("chronos"); err == nil {
		if isUsableChronosBinary(pathBin) {
			return pathBin, nil
		}
	}

	return "", fmt.Errorf("chronos binary not found; build the Rust core or place chronos on PATH")
}

func isUsableChronosBinary(path string) bool {
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		resolved = path
	}
	out, err := exec.Command(resolved, "--help").CombinedOutput()
	if err != nil {
		return false
	}
	help := string(out)
	return strings.Contains(help, "iON acquisition and preparation agent") &&
		!strings.Contains(help, "Unknown agent")
}

// PullBackup acquires a new iOS backup from a connected device using the chronos binary.
// Backup passwords are entered only at the libimobiledevice/idevicebackup2 prompt.
func (a *App) PullBackup(caseName string) {
	go func() {
		bin, err := findChronosBinary()
		if err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: "+err.Error())
			runtime.EventsEmit(a.ctx, "log", "[pull backup] "+err.Error())
			return
		}

		runtime.EventsEmit(a.ctx, "status", "Starting backup acquisition...")
		runtime.EventsEmit(a.ctx, "log", "[pull backup] Connecting to device via chronos...")

		cmd := exec.Command(bin, caseName)
		cmd.Env = appendCaseRootsEnv(cmd.Env)
		cmd.Stdin = os.Stdin

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
			runtime.EventsEmit(a.ctx, "status", "Error: failed to start chronos: "+err.Error())
			return
		}

		a.setCmd(cmd)

		go func() {
			if err := readChronosLines(stderr, a.emitChronosLine); err != nil && err != io.ErrClosedPipe {
				runtime.EventsEmit(a.ctx, "log", "[scanner error] "+err.Error())
			}
		}()

		if err := readChronosLines(stdout, a.emitChronosLine); err != nil && err != io.ErrClosedPipe {
			runtime.EventsEmit(a.ctx, "log", "[scanner error] "+err.Error())
		}

		if err := cmd.Wait(); err != nil {
			runtime.EventsEmit(a.ctx, "status", "Error: backup acquisition failed: "+err.Error())
			return
		}

		// Register the newly acquired case root so the UI can find it.
		if root, err := resolveCaseRoot(caseName); err == nil {
			_ = registerCaseRoot(caseName, root)
		}

		runtime.EventsEmit(a.ctx, "status", "Backup acquisition complete!")
		runtime.EventsEmit(a.ctx, "case-created", caseName)
	}()
}

func readChronosLines(reader io.Reader, emit func(string)) error {
	buffered := bufio.NewReaderSize(reader, 256*1024)
	for {
		line, err := buffered.ReadString('\n')
		if line != "" {
			emit(strings.TrimRight(line, "\r\n"))
		}
		if err != nil {
			if err == io.EOF {
				return nil
			}
			return err
		}
	}
}

func (a *App) emitChronosLine(line string) {
	if progress, ok := parseAcquisitionProgress(line); ok {
		runtime.EventsEmit(a.ctx, "acquisition-progress", progress)
	}
	if shouldLogChronosLine(line) {
		runtime.EventsEmit(a.ctx, "log", "[chronos] "+line)
	}
}

func shouldLogChronosLine(line string) bool {
	clean := strings.TrimSpace(stripANSI(line))
	if clean == "" {
		return false
	}
	lower := strings.ToLower(clean)
	importantMarkers := []string{
		"error",
		"failed",
		"aborting",
		"warning",
		"please enter",
		"trust",
		"pairing",
		"validating pairing",
		"collecting device info",
		"creating full encrypted backup",
		"backup acquisition completed",
		"backup acquisition complete",
		"full encrypted backup completed",
		"chronos acquisition complete",
		"chronos success",
		"skipping helios",
	}
	for _, marker := range importantMarkers {
		if strings.Contains(lower, marker) {
			return true
		}
	}
	if strings.HasPrefix(clean, "📱 Step") || strings.HasPrefix(clean, "📱 Creating") {
		return true
	}
	return false
}

func stripANSI(line string) string {
	return ansiPattern.ReplaceAllString(line, "")
}

func parseAcquisitionProgress(line string) (acquisitionProgress, bool) {
	matches := percentPattern.FindAllStringSubmatch(line, -1)
	if len(matches) == 0 {
		return acquisitionProgress{}, false
	}

	progress := acquisitionProgress{OverallPercent: -1, FilePercent: -1, Raw: line}
	var genericPercents []int
	for _, match := range matches {
		value, err := strconv.Atoi(match[2])
		if err != nil || value < 0 || value > 100 {
			continue
		}

		context := strings.ToLower(match[1])
		switch context {
		case "file", "files":
			progress.FilePercent = value
		case "overall", "total":
			progress.OverallPercent = value
		default:
			genericPercents = append(genericPercents, value)
		}
	}

	if len(genericPercents) == 1 {
		if progress.OverallPercent < 0 {
			progress.OverallPercent = genericPercents[0]
		}
		if progress.FilePercent < 0 {
			progress.FilePercent = genericPercents[0]
		}
	} else if len(genericPercents) > 1 {
		if progress.FilePercent < 0 {
			progress.FilePercent = genericPercents[0]
		}
		if progress.OverallPercent < 0 {
			progress.OverallPercent = genericPercents[len(genericPercents)-1]
			for _, value := range genericPercents {
				if value > progress.OverallPercent {
					progress.OverallPercent = value
				}
			}
		}
	}

	if progress.FilePercent < 0 && progress.OverallPercent >= 0 {
		progress.FilePercent = progress.OverallPercent
	}
	if progress.OverallPercent < 0 && progress.FilePercent >= 0 {
		progress.OverallPercent = progress.FilePercent
	}
	if progress.OverallPercent < 0 && progress.FilePercent < 0 {
		return acquisitionProgress{}, false
	}

	progress.Label = labelForAcquisitionLine(line)
	return progress, true
}

func labelForAcquisitionLine(line string) string {
	clean := strings.TrimSpace(strings.TrimPrefix(line, "[idevicebackup2]"))
	if clean == "" {
		return "Backup running"
	}
	if len(clean) > 140 {
		return clean[:137] + "..."
	}
	return clean
}
