package main

import (
	"bufio"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// InstalledApp describes one app found in a backup.
type InstalledApp struct {
	BundleID         string   `json:"bundle_id"`
	Name             string   `json:"name"`
	Domain           string   `json:"domain"`
	DataSizeBytes    int64    `json:"data_size_bytes"`
	DataSize         string   `json:"data_size"`
	ContainerPath    string   `json:"container_path"`
	GroupContainers  []string `json:"group_containers"`
	HasDocuments     bool     `json:"has_documents"`
	HasLibrary       bool     `json:"has_library"`
	FileCount        int      `json:"file_count"`
	Source           string   `json:"source"`
}

// GetInstalledApps returns installed apps discovered in the case backup.
// It prefers parsing Manifest.db, then falls back to scanning AppDomain-* directories.
func (a *App) GetInstalledApps(caseName string) ([]InstalledApp, error) {
	root, err := resolveCaseRoot(caseName)
	if err != nil {
		return nil, err
	}

	backupPath := locateCaseBackup(root, caseName)
	if backupPath == "" {
		return nil, fmt.Errorf("no backup found for case: %s", caseName)
	}

	// Try Manifest.db first (standard iTunes/Finder backup).
	manifestDB := findManifestDB(backupPath)
	if manifestDB != "" {
		apps, err := appsFromManifestDB(manifestDB, backupPath)
		if err == nil && len(apps) > 0 {
			return apps, nil
		}
	}

	// Fallback: scan extracted app-domain directories.
	return appsFromDomainDirs(backupPath)
}

// locateCaseBackup finds the directory that actually contains the case backup data.
// It checks the case's own backup/ and prepared/ folders, then the HELiOS output
// layout at <projectRoot>/prepared/helios/<caseName>.
func locateCaseBackup(root, caseName string) string {
	candidates := []string{
		filepath.Join(root, "backup"),
		filepath.Join(root, "prepared"),
	}
	if projectRoot, err := findProjectRoot(); err == nil {
		candidates = append(candidates,
			filepath.Join(projectRoot, "prepared", "helios", caseName),
			filepath.Join(projectRoot, "prepared", caseName),
		)
	}

	for _, p := range candidates {
		info, err := os.Stat(p)
		if err != nil || !info.IsDir() {
			continue
		}
		// Accept the path if it contains a Manifest.db or any AppDomain directories.
		if findManifestDB(p) != "" {
			return p
		}
		if hasAppDomainDirs(p) {
			return p
		}
	}
	return ""
}

func hasAppDomainDirs(root string) bool {
	entries, err := os.ReadDir(root)
	if err != nil {
		return false
	}
	for _, e := range entries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "AppDomain") {
			return true
		}
	}
	return false
}

// OpenAppContainer opens the app's container directory in the system file manager.
func (a *App) OpenAppContainer(path string) error {
	if path == "" {
		return fmt.Errorf("no container path provided")
	}
	if _, err := os.Stat(path); err != nil {
		return fmt.Errorf("container not found: %w", err)
	}
	cmd := exec.Command("xdg-open", path)
	return cmd.Start()
}

func findManifestDB(root string) string {
	candidates := []string{
		filepath.Join(root, "Manifest.db"),
		filepath.Join(root, "backup", "Manifest.db"),
	}
	for _, p := range candidates {
		if fileExists(p) {
			return p
		}
	}
	return ""
}

func appsFromManifestDB(manifestDB, backupRoot string) ([]InstalledApp, error) {
	query := `SELECT domain, SUM(length(file)) as size, COUNT(*) as count FROM Files WHERE domain LIKE 'AppDomain-%' GROUP BY domain ORDER BY size DESC`
	cmd := exec.Command("sqlite3", "-separator", "|", manifestDB, query)
	out, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("sqlite3 query failed: %w", err)
	}

	var apps []InstalledApp
	scanner := bufio.NewScanner(strings.NewReader(string(out)))
	for scanner.Scan() {
		parts := strings.Split(scanner.Text(), "|")
		if len(parts) < 3 {
			continue
		}
		domain := parts[0]
		bundleID := strings.TrimPrefix(domain, "AppDomain-")
		var size int64
		fmt.Sscanf(parts[1], "%d", &size)
		var count int
		fmt.Sscanf(parts[2], "%d", &count)

		containerPath := filepath.Join(backupRoot, domain)
		if !dirExists(containerPath) {
			containerPath = ""
		}

		apps = append(apps, InstalledApp{
			BundleID:      bundleID,
			Name:          appNameFromBundleID(bundleID),
			Domain:        domain,
			DataSizeBytes: size,
			DataSize:      humanSize(size),
			ContainerPath: containerPath,
			FileCount:     count,
			HasDocuments:  dirExists(filepath.Join(containerPath, "Documents")),
			HasLibrary:    dirExists(filepath.Join(containerPath, "Library")),
			Source:        "Manifest.db",
		})
	}

	// Also include AppDomainGroup entries that relate to known apps.
	groupQuery := `SELECT domain, SUM(length(file)) as size, COUNT(*) as count FROM Files WHERE domain LIKE 'AppDomainGroup-%' GROUP BY domain ORDER BY size DESC`
	cmd = exec.Command("sqlite3", "-separator", "|", manifestDB, groupQuery)
	out, err = cmd.Output()
	if err == nil {
		scanner = bufio.NewScanner(strings.NewReader(string(out)))
		for scanner.Scan() {
			parts := strings.Split(scanner.Text(), "|")
			if len(parts) < 3 {
				continue
			}
			domain := parts[0]
			groupID := strings.TrimPrefix(domain, "AppDomainGroup-")
			var size int64
			fmt.Sscanf(parts[1], "%d", &size)
			var count int
			fmt.Sscanf(parts[2], "%d", &count)

			// Try to attribute group container to an existing app by bundle ID prefix.
			matched := false
			for i := range apps {
				if strings.HasSuffix(groupID, apps[i].BundleID) || strings.Contains(groupID, apps[i].BundleID) {
					apps[i].GroupContainers = append(apps[i].GroupContainers, groupID)
					apps[i].DataSizeBytes += size
					apps[i].DataSize = humanSize(apps[i].DataSizeBytes)
					matched = true
					break
				}
			}
			if !matched {
				apps = append(apps, InstalledApp{
					BundleID:      groupID,
					Name:          appNameFromBundleID(groupID),
					Domain:        domain,
					DataSizeBytes: size,
					DataSize:      humanSize(size),
					FileCount:     count,
					Source:        "Manifest.db (group)",
				})
			}
		}
	}

	return apps, nil
}

func appsFromDomainDirs(root string) ([]InstalledApp, error) {
	entries, err := os.ReadDir(root)
	if err != nil {
		return nil, err
	}

	var apps []InstalledApp
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		name := e.Name()
		if !strings.HasPrefix(name, "AppDomain-") && !strings.HasPrefix(name, "AppDomainGroup-") {
			continue
		}

		path := filepath.Join(root, name)
		size, count := dirSizeAndCount(path)

		var bundleID, source string
		if strings.HasPrefix(name, "AppDomain-") {
			bundleID = strings.TrimPrefix(name, "AppDomain-")
			source = "domain scan"
		} else {
			bundleID = strings.TrimPrefix(name, "AppDomainGroup-")
			source = "domain scan (group)"
		}

		apps = append(apps, InstalledApp{
			BundleID:      bundleID,
			Name:          appNameFromBundleID(bundleID),
			Domain:        name,
			DataSizeBytes: size,
			DataSize:      humanSize(size),
			ContainerPath: path,
			FileCount:     count,
			HasDocuments:  dirExists(filepath.Join(path, "Documents")),
			HasLibrary:    dirExists(filepath.Join(path, "Library")),
			Source:        source,
		})
	}

	return apps, nil
}

func appNameFromBundleID(bundleID string) string {
	// Best-effort friendly name from bundle ID.
	parts := strings.Split(bundleID, ".")
	if len(parts) == 0 {
		return bundleID
	}
	last := parts[len(parts)-1]
	last = strings.ReplaceAll(last, "-", " ")
	last = strings.ReplaceAll(last, "_", " ")
	return strings.Title(last)
}

func dirSizeAndCount(path string) (int64, int) {
	var size int64
	var count int
	filepath.WalkDir(path, func(_ string, d os.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return nil
		}
		size += info.Size()
		count++
		return nil
	})
	return size, count
}

func dirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

// AppDataHit describes one text match inside an app container.
type AppDataHit struct {
	BundleID    string `json:"bundle_id"`
	AppName     string `json:"app_name"`
	FilePath    string `json:"file_path"`
	Line        int    `json:"line"`
	Snippet     string `json:"snippet"`
	MatchType   string `json:"match_type"`
}

// SearchAppData scans app container files for keywords or phone numbers.
// It focuses on SQLite, JSON, plist, log, and archive files.
func (a *App) SearchAppData(caseName string, query string) ([]AppDataHit, error) {
	if strings.TrimSpace(query) == "" {
		return nil, fmt.Errorf("search query is empty")
	}

	apps, err := a.GetInstalledApps(caseName)
	if err != nil {
		return nil, err
	}

	terms := strings.Fields(strings.ToLower(query))
	if len(terms) == 0 {
		return nil, fmt.Errorf("search query is empty")
	}

	var hits []AppDataHit
	var extensions = map[string]bool{
		".json": true, ".plist": true, ".log": true, ".txt": true,
		".sql": true, ".sqlite": true, ".sqlite3": true, ".db": true,
		".archive": true, ".dat": true, ".data": true,
	}

	for _, app := range apps {
		container := app.ContainerPath
		if container == "" || !dirExists(container) {
			continue
		}

		err := filepath.WalkDir(container, func(path string, d os.DirEntry, err error) error {
			if err != nil || d.IsDir() {
				return nil
			}

			ext := strings.ToLower(filepath.Ext(path))
			if !extensions[ext] {
				return nil
			}

			if info, err := d.Info(); err == nil && info.Size() > 50*1024*1024 {
				return nil // skip files larger than 50 MB
			}

			f, err := os.Open(path)
			if err != nil {
				return nil
			}
			defer f.Close()

			scanner := bufio.NewScanner(f)
			scanner.Buffer(make([]byte, 1024*1024), 1024*1024)
			lineNo := 0
			for scanner.Scan() {
				lineNo++
				line := scanner.Text()
				lower := strings.ToLower(line)
				for _, term := range terms {
					if strings.Contains(lower, term) {
						snippet := line
						if len(snippet) > 240 {
							snippet = snippet[:240] + "..."
						}
						hits = append(hits, AppDataHit{
							BundleID:  app.BundleID,
							AppName:   app.Name,
							FilePath:  path,
							Line:      lineNo,
							Snippet:   snippet,
							MatchType: "keyword",
						})
						break
					}
				}
			}
			return nil
		})
		if err != nil {
			continue
		}
	}

	return hits, nil
}

// AppFile describes one file inside an app container.
type AppFile struct {
	Path string `json:"path"`
	Name string `json:"name"`
	Size int64  `json:"size"`
	Dir  bool   `json:"dir"`
}

// ReadAppFile returns the contents of a small app container file as a string.
func (a *App) ReadAppFile(path string) (string, error) {
	if path == "" {
		return "", fmt.Errorf("path is required")
	}
	info, err := os.Stat(path)
	if err != nil {
		return "", err
	}
	if info.IsDir() {
		return "", fmt.Errorf("path is a directory")
	}
	if info.Size() > 1024*1024 {
		return "", fmt.Errorf("file is too large to preview")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

// ListAppFiles returns the immediate children of an app container path.
func (a *App) ListAppFiles(path string) ([]AppFile, error) {
	if path == "" {
		return nil, fmt.Errorf("path is required")
	}
	info, err := os.Stat(path)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("path is not a directory")
	}

	entries, err := os.ReadDir(path)
	if err != nil {
		return nil, err
	}

	var files []AppFile
	for _, e := range entries {
		info, err := e.Info()
		if err != nil {
			continue
		}
		files = append(files, AppFile{
			Path: filepath.Join(path, e.Name()),
			Name: e.Name(),
			Size: info.Size(),
			Dir:  e.IsDir(),
		})
	}
	return files, nil
}
