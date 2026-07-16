package main

import (
	"encoding/csv"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestMessageIndexQueriesCompleteThreads(t *testing.T) {
	root := createSyntheticMessageCase(t)
	sources, err := locateMessageSources("synthetic-messages", root)
	if err != nil {
		t.Fatal(err)
	}
	indexPath := messageIndexPath(root)
	t.Cleanup(func() { _ = os.RemoveAll(filepath.Dir(indexPath)) })
	if strings.HasPrefix(indexPath, filepath.Join(root, "evidence")) {
		t.Fatalf("index must be outside evidence: %s", indexPath)
	}

	entry := &messageIndexEntry{}
	entry.buildMu.Lock()
	db, status, err := entry.ensureLocked(sources)
	entry.buildMu.Unlock()
	if err != nil {
		t.Fatalf("build index: %v", err)
	}
	t.Cleanup(func() {
		if entry.db != nil {
			_ = entry.db.Close()
		}
	})
	if status.State != "ready" || status.MessageCount != 4 || status.ThreadCount != 2 || status.Reused {
		t.Fatalf("unexpected status: %+v", status)
	}

	contacts, err := getMessageContacts(db, "Alice", 20, 0)
	if err != nil {
		t.Fatal(err)
	}
	if contacts.Total != 1 || len(contacts.Contacts) != 1 {
		t.Fatalf("expected Alice contact, got %+v", contacts)
	}
	alice := contacts.Contacts[0]
	if alice.DisplayName != "Alice Example" || alice.MessageCount != 2 || alice.ThreadCount != 1 {
		t.Fatalf("unexpected Alice summary: %+v", alice)
	}

	page, err := queryMessages(db, MessageQuery{Handles: []string{"+1 (555) 123-4567"}, Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if page.Total != 2 || len(page.Messages) != 1 || !page.HasMore || page.NextOffset != 1 {
		t.Fatalf("unexpected first page: %+v", page)
	}
	page2, err := queryMessages(db, MessageQuery{Handles: []string{"5551234567"}, Limit: 10, Offset: page.NextOffset})
	if err != nil {
		t.Fatal(err)
	}
	if len(page2.Messages) != 1 || page2.Messages[0].Handle != "" || page2.Messages[0].Direction != "Sent" {
		t.Fatalf("outgoing blank-handle message was not included: %+v", page2.Messages)
	}
	if page.Messages[0].ID == "" || page2.Messages[0].ID == "" {
		t.Fatal("query messages must have stable IDs")
	}
	redacted := map[string]bool{page.Messages[0].ID: true}
	applyMessageRedactions(page.Messages, redacted)
	if page.Messages[0].Text == nil || *page.Messages[0].Text != "[REDACTED]" {
		t.Fatalf("redaction overlay was not applied: %+v", page.Messages[0])
	}

	groupPage, err := queryMessages(db, MessageQuery{Handles: []string{"2223334444", "3334445555"}, Limit: 20})
	if err != nil {
		t.Fatal(err)
	}
	if groupPage.Total != 2 || len(groupPage.Messages) != 2 {
		t.Fatalf("multi-contact group thread was duplicated or truncated: %+v", groupPage)
	}

	searchPage, err := queryMessages(db, MessageQuery{Handles: []string{"5551234567"}, Search: "reply with 100%", Limit: 20})
	if err != nil {
		t.Fatal(err)
	}
	if searchPage.Total != 1 || searchPage.Messages[0].Direction != "Sent" {
		t.Fatalf("escaped message search failed: %+v", searchPage)
	}
	threads, err := getMessageThreads(db, nil, "reply with 100%", 20, 0)
	if err != nil {
		t.Fatal(err)
	}
	if threads.Total != 1 || len(threads.Threads) != 1 || threads.Threads[0].ThreadID != "thread-alice" {
		t.Fatalf("message-text thread search failed: %+v", threads)
	}

	jsonExport, err := exportMessageQuery(db, root, MessageQuery{Handles: []string{"5551234567"}}, "json", time.Unix(1, 2), redacted)
	if err != nil {
		t.Fatal(err)
	}
	if jsonExport.Count != 2 {
		t.Fatalf("unexpected JSON export: %+v", jsonExport)
	}
	data, err := os.ReadFile(jsonExport.Path)
	if err != nil {
		t.Fatal(err)
	}
	var exported []MessageRecord
	if err := json.Unmarshal(data, &exported); err != nil || len(exported) != 2 {
		t.Fatalf("invalid JSON export (%v): %s", err, data)
	}
	if exported[0].Text == nil || *exported[0].Text != "[REDACTED]" {
		t.Fatalf("JSON export did not apply redaction overlay: %+v", exported[0])
	}

	csvExport, err := exportMessageQuery(db, root, MessageQuery{Handles: []string{"5551234567"}}, "csv", time.Unix(1, 3), nil)
	if err != nil {
		t.Fatal(err)
	}
	csvFile, err := os.Open(csvExport.Path)
	if err != nil {
		t.Fatal(err)
	}
	records, readErr := csv.NewReader(csvFile).ReadAll()
	_ = csvFile.Close()
	if readErr != nil || len(records) != 3 {
		t.Fatalf("invalid CSV export (%v): %d rows", readErr, len(records))
	}
}

func TestMessageIndexInvalidatesAndReusesCache(t *testing.T) {
	root := createSyntheticMessageCase(t)
	sources, err := locateMessageSources("synthetic-invalidation", root)
	if err != nil {
		t.Fatal(err)
	}
	indexPath := messageIndexPath(root)
	t.Cleanup(func() { _ = os.RemoveAll(filepath.Dir(indexPath)) })

	entry := &messageIndexEntry{}
	entry.buildMu.Lock()
	_, first, err := entry.ensureLocked(sources)
	entry.buildMu.Unlock()
	if err != nil {
		t.Fatal(err)
	}
	if first.MessageCount != 4 {
		t.Fatalf("unexpected initial count: %+v", first)
	}

	message := syntheticMessage("thread-alice", 5, "guid-5", "2026-01-01T00:04:00Z", "Received", "+15551234567", "new message")
	appendJSONLine(t, sources.Messages.Path, message)
	updatedSources, err := locateMessageSources("synthetic-invalidation", root)
	if err != nil {
		t.Fatal(err)
	}
	if updatedSources.Fingerprint == sources.Fingerprint {
		t.Fatal("source fingerprint did not change")
	}
	entry.buildMu.Lock()
	_, rebuilt, err := entry.ensureLocked(updatedSources)
	entry.buildMu.Unlock()
	if err != nil {
		t.Fatal(err)
	}
	if rebuilt.MessageCount != 5 || rebuilt.Reused {
		t.Fatalf("index was not rebuilt: %+v", rebuilt)
	}
	_ = entry.db.Close()
	entry.db = nil

	reopened := &messageIndexEntry{}
	reopened.buildMu.Lock()
	_, reused, err := reopened.ensureLocked(updatedSources)
	reopened.buildMu.Unlock()
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.db.Close()
	if reused.MessageCount != 5 || !reused.Reused {
		t.Fatalf("valid index was not reused: %+v", reused)
	}
}

func TestNormalizeMessageIdentity(t *testing.T) {
	tests := map[string]string{
		"+1 (555) 123-4567":  "5551234567",
		"555.123.4567":       "5551234567",
		"411":                "411",
		"Person@Example.COM": "person@example.com",
	}
	for input, want := range tests {
		if got := normalizeMessageIdentity(input); got != want {
			t.Errorf("normalizeMessageIdentity(%q) = %q, want %q", input, got, want)
		}
	}
}

func createSyntheticMessageCase(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	cerberusDir := filepath.Join(root, "evidence", "cerberus")
	packageDir := filepath.Join(cerberusDir, "review_package")
	if err := os.MkdirAll(packageDir, 0755); err != nil {
		t.Fatal(err)
	}
	contacts := []map[string]any{
		{"contact_id": 1, "contact_name": "Alice Example", "value_type": "phone", "value": "5551234567"},
		{"contact_id": 2, "contact_name": "Bob Group", "value_type": "phone", "value": "2223334444"},
		{"contact_id": 3, "contact_name": "Carol Group", "value_type": "phone", "value": "3334445555"},
	}
	writeJSONFile(t, filepath.Join(cerberusDir, "contact_identities.json"), contacts)
	threads := []messageThreadJSONL{
		{ThreadID: "thread-alice", MessageCount: 2, Participants: []string{"+15551234567"}, HTMLPath: "/evidence/alice.html"},
		{ThreadID: "thread-group", MessageCount: 2, Participants: []string{"2223334444", "3334445555"}, HTMLPath: "/evidence/group.html"},
	}
	writeJSONLines(t, filepath.Join(packageDir, "threads.jsonl"), threads)
	messages := []MessageRecord{
		syntheticMessage("thread-alice", 1, "guid-1", "2026-01-01T00:00:00Z", "Received", "+15551234567", "hello"),
		syntheticMessage("thread-alice", 2, "guid-2", "2026-01-01T00:01:00Z", "Sent", "", "reply with 100% confidence"),
		syntheticMessage("thread-group", 3, "guid-3", "2026-01-01T00:02:00Z", "Received", "2223334444", "group hello"),
		syntheticMessage("thread-group", 4, "guid-4", "2026-01-01T00:03:00Z", "Sent", "", "group reply"),
	}
	writeJSONLines(t, filepath.Join(packageDir, "messages.jsonl"), messages)
	return root
}

func syntheticMessage(threadID string, messageID int64, guid, timestamp, direction, handle, text string) MessageRecord {
	return MessageRecord{
		ThreadID: threadID, MessageID: messageID, GUID: guid, TimestampUTC: stringPointer(timestamp),
		Direction: direction, Handle: handle, Service: "iMessage", Text: stringPointer(text),
		AttachmentPaths: []string{}, AttachmentExportPaths: []string{}, AttachmentMIMETypes: []string{},
	}
}

func stringPointer(value string) *string { return &value }

func writeJSONFile(t *testing.T, path string, value any) {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, 0644); err != nil {
		t.Fatal(err)
	}
}

func writeJSONLines[T any](t *testing.T, path string, values []T) {
	t.Helper()
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	encoder := json.NewEncoder(file)
	for _, value := range values {
		if err := encoder.Encode(value); err != nil {
			_ = file.Close()
			t.Fatal(err)
		}
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
}

func appendJSONLine(t *testing.T, path string, value any) {
	t.Helper()
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0644)
	if err != nil {
		t.Fatal(err)
	}
	err = json.NewEncoder(file).Encode(value)
	closeErr := file.Close()
	if err != nil {
		t.Fatal(err)
	}
	if closeErr != nil {
		t.Fatal(closeErr)
	}
}
