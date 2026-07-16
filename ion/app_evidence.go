package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

// CaseSummary describes a case for the dashboard.
type CaseSummary struct {
	Name            string   `json:"name"`
	RootPath        string   `json:"rootPath"`
	BackupPath      string   `json:"backupPath"`
	BackupSizeBytes int64    `json:"backupSizeBytes"`
	BackupSize      string   `json:"backupSize"`
	EvidenceAgents  []string `json:"evidenceAgents"`
	LastRunStatus   string   `json:"lastRunStatus"`
	HasBackup       bool     `json:"hasBackup"`
}

// EvidenceSummary mirrors the Rust summary.json structure.
type EvidenceSummary struct {
	Agent       string `json:"agent"`
	Slug        string `json:"slug"`
	Schema      int    `json:"schema_version"`
	GeneratedAt string `json:"generated_at"`
	RecordCount int    `json:"record_count"`
	EvidenceDir string `json:"evidence_dir"`
}

// commonCaseRoots returns well-known locations where iON cases may live.
func commonCaseRoots() []string {
	var roots []string
	if dir, err := casesDir(); err == nil {
		roots = append(roots, dir)
	}
	if home, err := os.UserHomeDir(); err == nil {
		roots = append(roots,
			filepath.Join(home, "iON-Data-Security-Systems", "backups", "cases"),
			filepath.Join(home, "iON", "backups", "cases"),
			filepath.Join(home, "iON", "cases"),
		)
	}
	return roots
}

// resolveCaseRoot finds a case root from the Rust registry or well-known locations.
func resolveCaseRoot(name string) (string, error) {
	home, err := os.UserHomeDir()
	if err == nil {
		registryPath := filepath.Join(home, ".iON", "case_registry.json")
		if data, err := os.ReadFile(registryPath); err == nil {
			var reg caseRegistry
			if err := json.Unmarshal(data, &reg); err == nil {
				if entry, ok := reg.Cases[name]; ok && entry.Root != "" {
					if info, err := os.Stat(entry.Root); err == nil && info.IsDir() {
						return entry.Root, nil
					}
				}
			}
		}
	}

	for _, root := range commonCaseRoots() {
		candidate := filepath.Join(root, name)
		if info, err := os.Stat(candidate); err == nil && info.IsDir() {
			return candidate, nil
		}
	}

	return "", fmt.Errorf("case not found: %s", name)
}

// GetCaseSummary returns dashboard metadata for a case.
func (a *App) GetCaseSummary(name string) (CaseSummary, error) {
	root, err := resolveCaseRoot(name)
	if err != nil {
		return CaseSummary{}, err
	}

	backupPath := filepath.Join(root, "backup")
	info, err := os.Lstat(backupPath)
	hasBackup := err == nil

	var size int64
	if hasBackup {
		// If it's a symlink, evaluate the target.
		if info.Mode()&os.ModeSymlink != 0 {
			target, err := os.Readlink(backupPath)
			if err == nil {
				if !filepath.IsAbs(target) {
					target = filepath.Join(root, target)
				}
				backupPath = target
			}
		}
		size, _ = dirSize(backupPath)
	}

	agents, _ := listEvidenceAgents(root)

	return CaseSummary{
		Name:            name,
		RootPath:        root,
		BackupPath:      backupPath,
		BackupSizeBytes: size,
		BackupSize:      humanSize(size),
		EvidenceAgents:  agents,
		LastRunStatus:   inferLastRunStatus(root, agents),
		HasBackup:       hasBackup,
	}, nil
}

// ListEvidenceTypes returns the agents that have produced evidence for a case.
func (a *App) ListEvidenceTypes(name string) ([]string, error) {
	root, err := resolveCaseRoot(name)
	if err != nil {
		return nil, err
	}
	return listEvidenceAgents(root)
}

// GetEvidenceSummary reads an agent's summary.json.
func (a *App) GetEvidenceSummary(caseName, agent string) (EvidenceSummary, error) {
	root, err := resolveCaseRoot(caseName)
	if err != nil {
		return EvidenceSummary{}, err
	}

	path := filepath.Join(root, "evidence", agent, "summary.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return EvidenceSummary{}, fmt.Errorf("no summary for %s/%s: %w", caseName, agent, err)
	}

	var summary EvidenceSummary
	if err := json.Unmarshal(data, &summary); err != nil {
		return EvidenceSummary{}, err
	}
	return summary, nil
}

// GetEvidenceRecords returns records from an agent's records.json, optionally
// filtered by record_type. Each record is tagged with a stable _id derived from
// its index so the UI can select and act on individual messages.
// limit <= 0 means return all records (use with care on huge backups).
func (a *App) GetEvidenceRecords(caseName, agent, recordType string, limit int) ([]map[string]interface{}, error) {
	root, err := resolveCaseRoot(caseName)
	if err != nil {
		return nil, err
	}

	path := filepath.Join(root, "evidence", agent, "records.json")
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("no records for %s/%s: %w", caseName, agent, err)
	}
	defer file.Close()

	return readEvidenceRecords(file, recordType, limit)
}

func readEvidenceRecords(reader io.Reader, recordType string, limit int) ([]map[string]interface{}, error) {
	decoder := json.NewDecoder(reader)
	token, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	if delimiter, ok := token.(json.Delim); !ok || delimiter != '[' {
		return nil, fmt.Errorf("evidence records must be a JSON array")
	}

	wantType := strings.TrimSpace(recordType)
	var filtered []map[string]interface{}
	for i := 0; decoder.More(); i++ {
		var r map[string]interface{}
		if err := decoder.Decode(&r); err != nil {
			return nil, err
		}
		if wantType != "" {
			if rt, ok := r["record_type"].(string); !ok || rt != wantType {
				continue
			}
		}
		r["_id"] = fmt.Sprintf("%d", i)
		filtered = append(filtered, r)
		if limit > 0 && len(filtered) >= limit {
			return filtered, nil
		}
	}
	closing, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	if delimiter, ok := closing.(json.Delim); !ok || delimiter != ']' {
		return nil, fmt.Errorf("evidence records must end with a JSON array delimiter")
	}

	return filtered, nil
}

// ExportEvidence returns the filesystem path to an existing export file.
// format must be "json", "jsonl", "csv", or "pdf".
func (a *App) ExportEvidence(caseName, agent, format string) (string, error) {
	root, err := resolveCaseRoot(caseName)
	if err != nil {
		return "", err
	}

	switch strings.ToLower(format) {
	case "json":
		return filepath.Join(root, "evidence", agent, "records.json"), nil
	case "jsonl":
		return filepath.Join(root, "evidence", agent, "records.jsonl"), nil
	case "csv":
		return filepath.Join(root, "evidence", agent, "records.csv"), nil
	case "pdf":
		return filepath.Join(root, "evidence", agent, "report.pdf"), nil
	default:
		return "", fmt.Errorf("unsupported export format: %s", format)
	}
}

// listEvidenceAgents scans the evidence directory for completed agent runs.
func listEvidenceAgents(root string) ([]string, error) {
	evidenceDir := filepath.Join(root, "evidence")
	entries, err := os.ReadDir(evidenceDir)
	if err != nil {
		if os.IsNotExist(err) {
			return []string{}, nil
		}
		return nil, err
	}

	var agents []string
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		if _, err := os.Stat(filepath.Join(evidenceDir, e.Name(), "summary.json")); err == nil {
			agents = append(agents, e.Name())
		}
	}
	sort.Strings(agents)
	return agents, nil
}

// inferLastRunStatus guesses whether a case has been processed recently.
func inferLastRunStatus(root string, agents []string) string {
	if len(agents) == 0 {
		return "No agents run yet"
	}
	return fmt.Sprintf("Evidence available from %d agent(s)", len(agents))
}

// dirSize recursively sums the size of a directory.
func dirSize(path string) (int64, error) {
	var size int64
	err := filepath.WalkDir(path, func(_ string, d os.DirEntry, err error) error {
		if err != nil {
			return nil // skip inaccessible files
		}
		if d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return nil
		}
		size += info.Size()
		return nil
	})
	return size, err
}

// humanSize formats bytes as human-readable string.
func humanSize(bytes int64) string {
	const (
		KB = 1024
		MB = 1024 * KB
		GB = 1024 * MB
		TB = 1024 * GB
	)
	switch {
	case bytes >= TB:
		return fmt.Sprintf("%.2f TB", float64(bytes)/float64(TB))
	case bytes >= GB:
		return fmt.Sprintf("%.2f GB", float64(bytes)/float64(GB))
	case bytes >= MB:
		return fmt.Sprintf("%.2f MB", float64(bytes)/float64(MB))
	case bytes >= KB:
		return fmt.Sprintf("%.2f KB", float64(bytes)/float64(KB))
	default:
		return fmt.Sprintf("%d B", bytes)
	}
}

// readAtMost returns up to limit bytes from a file, or the whole file if limit <= 0.
func readAtMost(path string, limit int64) ([]byte, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	if limit <= 0 {
		return io.ReadAll(f)
	}

	return io.ReadAll(io.LimitReader(f, limit))
}

// MessageActionResult reports the outcome of a batch action.
type MessageActionResult struct {
	Action  string `json:"action"`
	Count   int    `json:"count"`
	Path    string `json:"path,omitempty"`
	Message string `json:"message,omitempty"`
}

// BatchMessageAction applies a bulk operation to selected message IDs.
// Supported actions: "evidence", "review", "redact", "print".
func (a *App) BatchMessageAction(caseName, agent, action string, ids []string) (MessageActionResult, error) {
	root, err := resolveCaseRoot(caseName)
	if err != nil {
		return MessageActionResult{}, err
	}
	if len(ids) == 0 {
		return MessageActionResult{}, fmt.Errorf("no messages selected")
	}

	switch strings.ToLower(action) {
	case "evidence":
		return tagMessages(root, agent, "tagged_evidence.json", ids)
	case "review":
		return tagMessages(root, agent, "review_queue.json", ids)
	case "redact":
		return redactMessages(root, agent, ids)
	case "print":
		return printMessages(root, agent, ids)
	default:
		return MessageActionResult{}, fmt.Errorf("unsupported action: %s", action)
	}
}

// tagMessages appends IDs to a JSON list stored in the agent evidence directory.
func tagMessages(root, agent, filename string, ids []string) (MessageActionResult, error) {
	path := filepath.Join(root, "evidence", agent, filename)
	existing := readStringSlice(path)
	seen := make(map[string]bool)
	for _, id := range existing {
		seen[id] = true
	}
	added := 0
	for _, id := range ids {
		if !seen[id] {
			existing = append(existing, id)
			seen[id] = true
			added++
		}
	}
	data, err := json.MarshalIndent(existing, "", "  ")
	if err != nil {
		return MessageActionResult{}, err
	}
	if err := os.WriteFile(path, data, 0644); err != nil {
		return MessageActionResult{}, err
	}
	return MessageActionResult{
		Action:  filepath.Base(filename),
		Count:   added,
		Path:    path,
		Message: fmt.Sprintf("Tagged %d new message(s); %d total", added, len(existing)),
	}, nil
}

// redactMessages replaces the text field of selected records with [REDACTED].
func redactMessages(root, agent string, ids []string) (MessageActionResult, error) {
	path := filepath.Join(root, "evidence", agent, "records.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return MessageActionResult{}, err
	}
	var records []map[string]interface{}
	if err := json.Unmarshal(data, &records); err != nil {
		return MessageActionResult{}, err
	}

	set := make(map[string]bool)
	for _, id := range ids {
		set[id] = true
	}

	redacted := 0
	for i, r := range records {
		if recordMatchesMessageIDs(r, i, set) {
			r["text"] = "[REDACTED]"
			redacted++
		}
	}

	out, err := json.MarshalIndent(records, "", "  ")
	if err != nil {
		return MessageActionResult{}, err
	}
	if err := os.WriteFile(path, out, 0644); err != nil {
		return MessageActionResult{}, err
	}
	manifestPath := filepath.Join(root, "evidence", agent, "redacted_messages.json")
	if _, _, err := mergeMessageIDs(manifestPath, ids); err != nil {
		return MessageActionResult{}, err
	}

	return MessageActionResult{
		Action:  "redact",
		Count:   redacted,
		Path:    manifestPath,
		Message: fmt.Sprintf("Redacted %d message(s)", redacted),
	}, nil
}

// printMessages writes a print-friendly HTML report of selected messages.
func printMessages(root, agent string, ids []string) (MessageActionResult, error) {
	path := filepath.Join(root, "evidence", agent, "records.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return MessageActionResult{}, err
	}
	var records []map[string]interface{}
	if err := json.Unmarshal(data, &records); err != nil {
		return MessageActionResult{}, err
	}

	set := make(map[string]bool)
	for _, id := range ids {
		set[id] = true
	}

	var selected []map[string]interface{}
	for i, r := range records {
		if recordMatchesMessageIDs(r, i, set) {
			selected = append(selected, r)
		}
	}

	outPath := filepath.Join(root, "evidence", agent, "print_report.html")
	f, err := os.Create(outPath)
	if err != nil {
		return MessageActionResult{}, err
	}
	defer f.Close()

	fmt.Fprintf(f, "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>%s - Print Report</title>", agent)
	fmt.Fprint(f, `<style>
body { font-family: sans-serif; margin: 40px; color: #222; }
h1 { border-bottom: 2px solid #333; padding-bottom: 8px; }
.message { border: 1px solid #ccc; border-radius: 6px; margin: 12px 0; padding: 12px; }
.meta { color: #666; font-size: 0.85rem; margin-bottom: 8px; }
.text { white-space: pre-wrap; }
.sent { background: #e6f7ff; }
.received { background: #f5f5f5; }
.footer { margin-top: 40px; font-size: 0.8rem; color: #666; }
@media print { body { margin: 20px; } }
</style></head><body>`)
	fmt.Fprintf(f, "<h1>%s — Message Report</h1>", agent)
	fmt.Fprintf(f, "<p>Generated %s | %d message(s) selected</p>", time.Now().Format(time.RFC1123), len(selected))
	for _, r := range selected {
		direction := "received"
		if d, ok := r["direction"].(string); ok && (d == "Sent" || d == "sent") {
			direction = "sent"
		} else if s, ok := r["is_sent"].(bool); ok && s {
			direction = "sent"
		}
		fmt.Fprintf(f, `<div class="message %s"><div class="meta">`, direction)
		fmt.Fprintf(f, "From: %v | To: %v | %v | %v", r["phone_number"], r["to_phone"], r["service"], r["timestamp"])
		fmt.Fprint(f, "</div><div class=\"text\">")
		if text, ok := r["text"].(string); ok {
			fmt.Fprint(f, text)
		}
		fmt.Fprint(f, "</div></div>")
	}
	fmt.Fprintf(f, "<div class=\"footer\">Generated by iON</div></body></html>")

	return MessageActionResult{
		Action:  "print",
		Count:   len(selected),
		Path:    outPath,
		Message: fmt.Sprintf("Print report ready with %d message(s)", len(selected)),
	}, nil
}

func recordMatchesMessageIDs(record map[string]interface{}, index int, selected map[string]bool) bool {
	if selected[strconv.Itoa(index)] {
		return true
	}
	if guid, ok := record["guid"].(string); ok && guid != "" {
		if selected["guid:"+guid] || selected[guid] {
			return true
		}
	}
	if id, ok := record["id"]; ok {
		var value string
		switch typed := id.(type) {
		case float64:
			value = strconv.FormatInt(int64(typed), 10)
		case string:
			value = typed
		}
		if value != "" && (selected["message:"+value] || selected[value]) {
			return true
		}
	}
	return false
}

func mergeMessageIDs(path string, ids []string) (added, total int, err error) {
	existing := readStringSlice(path)
	seen := make(map[string]bool, len(existing)+len(ids))
	for _, id := range existing {
		seen[id] = true
	}
	for _, id := range ids {
		if id == "" || seen[id] {
			continue
		}
		existing = append(existing, id)
		seen[id] = true
		added++
	}
	data, err := json.MarshalIndent(existing, "", "  ")
	if err != nil {
		return 0, 0, err
	}
	if err := os.WriteFile(path, data, 0644); err != nil {
		return 0, 0, err
	}
	return added, len(existing), nil
}

// OpenAttachment opens an extracted attachment with the system's default application.
func (a *App) OpenAttachment(path string) error {
	if path == "" {
		return fmt.Errorf("no attachment path provided")
	}
	info, err := os.Stat(path)
	if err != nil {
		return fmt.Errorf("attachment not found: %w", err)
	}
	if info.IsDir() {
		return fmt.Errorf("attachment path is a directory")
	}
	cmd := exec.Command("xdg-open", path)
	return cmd.Start()
}

// readStringSlice loads a JSON string slice or returns an empty slice.
func readStringSlice(path string) []string {
	data, err := os.ReadFile(path)
	if err != nil {
		return []string{}
	}
	var out []string
	if err := json.Unmarshal(data, &out); err != nil {
		return []string{}
	}
	return out
}
