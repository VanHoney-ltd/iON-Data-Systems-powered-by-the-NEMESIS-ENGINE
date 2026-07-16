package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHumanSize(t *testing.T) {
	if humanSize(512) != "512 B" {
		t.Fatalf("unexpected bytes formatting")
	}
	if humanSize(1024) != "1.00 KB" {
		t.Fatalf("unexpected KB formatting")
	}
}

func TestGetCaseSummary(t *testing.T) {
	app := NewApp()
	// The registry should contain iowa-vs-pearson-leary from prior work.
	_, err := app.GetCaseSummary("iowa-vs-pearson-leary")
	if err != nil {
		t.Fatalf("GetCaseSummary failed: %v", err)
	}
}

func TestEvidenceRecordsFromIowaCase(t *testing.T) {
	app := NewApp()
	records, err := app.GetEvidenceRecords("iowa-vs-pearson-leary", "vigil", "", 10)
	if err != nil {
		t.Fatalf("GetEvidenceRecords failed: %v", err)
	}
	if len(records) == 0 {
		t.Fatalf("expected some vigil records, got none")
	}
}

func TestGetEvidenceRecords_PcrByType(t *testing.T) {
	app := NewApp()
	messages, err := app.GetEvidenceRecords("pcr", "cerberus", "message", 100)
	if err != nil {
		t.Fatalf("GetEvidenceRecords messages failed: %v", err)
	}
	if len(messages) == 0 {
		t.Fatal("expected pcr cerberus messages")
	}
	contacts, err := app.GetEvidenceRecords("pcr", "cerberus", "contact", 100)
	if err != nil {
		t.Fatalf("GetEvidenceRecords contacts failed: %v", err)
	}
	if len(contacts) == 0 {
		t.Fatal("expected pcr cerberus contacts")
	}
	t.Logf("pcr cerberus: %d messages, %d contacts", len(messages), len(contacts))
}

func TestEvidenceBindingsOnSyntheticCase(t *testing.T) {
	// Use a fake case with a fake evidence file.
	root := t.TempDir()

	// Because resolveCaseRoot reads the registry, we can't easily point it at a temp dir.
	// Instead test the helper functions directly.
	agents, err := listEvidenceAgents(root)
	if err != nil {
		t.Fatalf("listEvidenceAgents failed: %v", err)
	}
	if len(agents) != 0 {
		t.Fatalf("expected no agents, got %v", agents)
	}

	agentDir := filepath.Join(root, "evidence", "vigil")
	if err := os.MkdirAll(agentDir, 0755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(agentDir, "summary.json"), []byte(`{"agent":"Vigil","record_count":5}`), 0644); err != nil {
		t.Fatalf("write summary: %v", err)
	}

	agents, err = listEvidenceAgents(root)
	if err != nil {
		t.Fatalf("listEvidenceAgents failed: %v", err)
	}
	if len(agents) != 1 || agents[0] != "vigil" {
		t.Fatalf("expected [vigil], got %v", agents)
	}
}

func TestReadEvidenceRecordsStreamsFilteredLimit(t *testing.T) {
	input := `[
		{"record_type":"contact","name":"A"},
		{"record_type":"message","text":"one"},
		{"record_type":"contact","name":"B"},
		{"record_type":"message","text":"two"},
		{"record_type":"message","text":"three"}
	]`

	records, err := readEvidenceRecords(strings.NewReader(input), "message", 2)
	if err != nil {
		t.Fatalf("readEvidenceRecords failed: %v", err)
	}
	if len(records) != 2 {
		t.Fatalf("got %d records, want 2", len(records))
	}
	if records[0]["_id"] != "1" || records[1]["_id"] != "3" {
		t.Fatalf("source indexes were not preserved: %#v", records)
	}
}

func TestReadEvidenceRecordsStopsBeforeUnreadInvalidTail(t *testing.T) {
	records, err := readEvidenceRecords(
		strings.NewReader(`[{"record_type":"message","text":"one"},{"broken":`),
		"message",
		1,
	)
	if err != nil {
		t.Fatalf("reader decoded beyond its limit: %v", err)
	}
	if len(records) != 1 {
		t.Fatalf("got %d records, want 1", len(records))
	}
}

func TestRecordMatchesStableMessageIDs(t *testing.T) {
	record := map[string]interface{}{
		"guid": "message-guid",
		"id":   float64(42),
	}
	if !recordMatchesMessageIDs(record, 7, map[string]bool{"guid:message-guid": true}) {
		t.Fatal("GUID-based message ID did not match")
	}
	if !recordMatchesMessageIDs(record, 7, map[string]bool{"message:42": true}) {
		t.Fatal("message row ID did not match")
	}
	if !recordMatchesMessageIDs(record, 7, map[string]bool{"7": true}) {
		t.Fatal("legacy source index did not match")
	}
}
