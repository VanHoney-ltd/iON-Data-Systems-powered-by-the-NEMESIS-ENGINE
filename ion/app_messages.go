package main

import (
	"bufio"
	"crypto/sha256"
	"database/sql"
	"encoding/csv"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

const (
	messageIndexSchemaVersion = 1
	defaultMessagePageSize    = 200
	maxMessagePageSize        = 500
	defaultSummaryPageSize    = 500
	maxSummaryPageSize        = 5000
)

// MessageIndexStatus describes the disposable index derived from Cerberus's
// review package. State is one of not_indexed, building, ready, or error.
type MessageIndexStatus struct {
	CaseName          string `json:"caseName"`
	State             string `json:"state"`
	SourcePath        string `json:"sourcePath"`
	IndexPath         string `json:"indexPath"`
	SchemaVersion     int    `json:"schemaVersion"`
	SourceSizeBytes   int64  `json:"sourceSizeBytes"`
	SourceModifiedAt  string `json:"sourceModifiedAt"`
	ProcessedMessages int64  `json:"processedMessages"`
	MessageCount      int64  `json:"messageCount"`
	ThreadCount       int64  `json:"threadCount"`
	IndexedAt         string `json:"indexedAt,omitempty"`
	Reused            bool   `json:"reused"`
	Error             string `json:"error,omitempty"`
}

// MessageContactSummary groups a participant identity across every thread in
// which it appears. MessageCount includes incoming and outgoing messages.
type MessageContactSummary struct {
	Handle            string  `json:"handle"`
	NormalizedHandle  string  `json:"normalizedHandle"`
	DisplayName       string  `json:"displayName"`
	MessageCount      int64   `json:"messageCount"`
	ThreadCount       int64   `json:"threadCount"`
	FirstTimestampUTC *string `json:"firstTimestampUtc,omitempty"`
	LastTimestampUTC  *string `json:"lastTimestampUtc,omitempty"`
}

type MessageContactPage struct {
	Contacts []MessageContactSummary `json:"contacts"`
	Total    int64                   `json:"total"`
	Limit    int                     `json:"limit"`
	Offset   int                     `json:"offset"`
	HasMore  bool                    `json:"hasMore"`
}

// MessageThreadSummary mirrors threads.jsonl and adds contact display names.
type MessageThreadSummary struct {
	ThreadID          string   `json:"threadId"`
	ChatID            *int64   `json:"chatId,omitempty"`
	ChatIdentifier    *string  `json:"chatIdentifier,omitempty"`
	DisplayName       *string  `json:"displayName,omitempty"`
	MessageCount      int64    `json:"messageCount"`
	AttachmentCount   int64    `json:"attachmentCount"`
	FirstTimestampUTC *string  `json:"firstTimestampUtc,omitempty"`
	LastTimestampUTC  *string  `json:"lastTimestampUtc,omitempty"`
	Participants      []string `json:"participants"`
	ParticipantNames  []string `json:"participantNames"`
	HTMLPath          string   `json:"htmlPath"`
}

type MessageThreadPage struct {
	Threads []MessageThreadSummary `json:"threads"`
	Total   int64                  `json:"total"`
	Limit   int                    `json:"limit"`
	Offset  int                    `json:"offset"`
	HasMore bool                   `json:"hasMore"`
}

// MessageQuery selects complete threads for Handles. This is intentional:
// outgoing iOS messages can have a blank handle and must not be omitted.
type MessageQuery struct {
	Handles  []string `json:"handles"`
	ThreadID string   `json:"threadId"`
	Search   string   `json:"search"`
	Limit    int      `json:"limit"`
	Offset   int      `json:"offset"`
}

// MessageRecord is the typed Cerberus messages.jsonl record. ID is derived
// from the GUID (or message ID fallback) and remains stable across page loads.
type MessageRecord struct {
	ID                    string   `json:"_id,omitempty"`
	ThreadID              string   `json:"thread_id"`
	ChatID                *int64   `json:"chat_id"`
	ChatIdentifier        *string  `json:"chat_identifier"`
	ChatDisplayName       *string  `json:"chat_display_name"`
	MessageID             int64    `json:"message_id"`
	GUID                  string   `json:"guid"`
	TimestampRaw          *int64   `json:"timestamp_raw"`
	TimestampUTC          *string  `json:"timestamp_utc"`
	Direction             string   `json:"direction"`
	Handle                string   `json:"handle"`
	Service               string   `json:"service"`
	Text                  *string  `json:"text"`
	Subject               *string  `json:"subject"`
	AttachmentCount       int      `json:"attachment_count"`
	AttachmentPaths       []string `json:"attachment_paths"`
	AttachmentExportPaths []string `json:"attachment_export_paths"`
	AttachmentMIMETypes   []string `json:"attachment_mime_types"`
	IsRead                bool     `json:"is_read"`
	IsDelivered           bool     `json:"is_delivered"`
	IsSent                bool     `json:"is_sent"`
	IsAudioMessage        bool     `json:"is_audio_message"`
	ItemType              *int64   `json:"item_type"`
	GroupTitle            *string  `json:"group_title"`
	ReplyToGUID           *string  `json:"reply_to_guid"`
	BalloonBundleID       *string  `json:"balloon_bundle_id"`
}

type MessagePage struct {
	Messages   []MessageRecord `json:"messages"`
	Total      int64           `json:"total"`
	Limit      int             `json:"limit"`
	Offset     int             `json:"offset"`
	NextOffset int             `json:"nextOffset"`
	HasMore    bool            `json:"hasMore"`
}

type MessageExportResult struct {
	Path   string `json:"path"`
	Count  int64  `json:"count"`
	Format string `json:"format"`
}

type messageThreadJSONL struct {
	ThreadID          string   `json:"thread_id"`
	ChatID            *int64   `json:"chat_id"`
	ChatIdentifier    *string  `json:"chat_identifier"`
	DisplayName       *string  `json:"display_name"`
	MessageCount      int64    `json:"message_count"`
	AttachmentCount   int64    `json:"attachment_count"`
	FirstTimestampUTC *string  `json:"first_timestamp_utc"`
	LastTimestampUTC  *string  `json:"last_timestamp_utc"`
	Participants      []string `json:"participants"`
	HTMLPath          string   `json:"html_path"`
}

type messageSourceFile struct {
	Path        string `json:"path"`
	Size        int64  `json:"size"`
	ModTimeNano int64  `json:"mtime_ns"`
	Kind        string `json:"kind,omitempty"`
}

type messageSources struct {
	CaseName    string            `json:"case_name"`
	Root        string            `json:"root"`
	Messages    messageSourceFile `json:"messages"`
	Threads     messageSourceFile `json:"threads"`
	Contacts    messageSourceFile `json:"contacts,omitempty"`
	Schema      int               `json:"schema"`
	Fingerprint string            `json:"-"`
}

type messageIndexEntry struct {
	buildMu     sync.Mutex
	statusMu    sync.RWMutex
	status      MessageIndexStatus
	db          *sql.DB
	fingerprint string
}

var messageIndexEntries sync.Map

func (a *App) GetMessageIndexStatus(caseName string) (MessageIndexStatus, error) {
	sources, entry, err := messageIndexForCase(caseName)
	if err != nil {
		return MessageIndexStatus{}, err
	}

	entry.statusMu.RLock()
	current := entry.status
	entry.statusMu.RUnlock()
	if current.State == "building" && current.SourcePath == sources.Messages.Path {
		return current, nil
	}

	if !entry.buildMu.TryLock() {
		entry.statusMu.RLock()
		defer entry.statusMu.RUnlock()
		return entry.status, nil
	}
	defer entry.buildMu.Unlock()

	if entry.db != nil && entry.fingerprint == sources.Fingerprint {
		return entry.statusSnapshot(), nil
	}
	if entry.db != nil {
		_ = entry.db.Close()
		entry.db = nil
		entry.fingerprint = ""
	}

	db, metadata, ok := openValidMessageIndex(messageIndexPath(sources.Root), sources.Fingerprint)
	if ok {
		entry.db = db
		entry.fingerprint = sources.Fingerprint
		status := statusFromMetadata(sources, metadata, true)
		entry.setStatus(status)
		return status, nil
	}
	if db != nil {
		_ = db.Close()
	}

	status := baseMessageIndexStatus(sources)
	status.State = "not_indexed"
	entry.setStatus(status)
	return status, nil
}

// PrepareMessageIndex creates or reuses the message index. Data methods call
// this implicitly, so callers do not need a separate preparation step.
func (a *App) PrepareMessageIndex(caseName string) (MessageIndexStatus, error) {
	sources, entry, err := messageIndexForCase(caseName)
	if err != nil {
		return MessageIndexStatus{}, err
	}
	entry.buildMu.Lock()
	defer entry.buildMu.Unlock()
	_, status, err := entry.ensureLocked(sources)
	return status, err
}

func (a *App) GetMessageContacts(caseName, search string, limit, offset int) (MessageContactPage, error) {
	sources, entry, err := messageIndexForCase(caseName)
	if err != nil {
		return MessageContactPage{}, err
	}
	entry.buildMu.Lock()
	defer entry.buildMu.Unlock()
	db, _, err := entry.ensureLocked(sources)
	if err != nil {
		return MessageContactPage{}, err
	}
	return getMessageContacts(db, search, limit, offset)
}

func (a *App) GetMessageThreads(caseName string, handles []string, search string, limit, offset int) (MessageThreadPage, error) {
	sources, entry, err := messageIndexForCase(caseName)
	if err != nil {
		return MessageThreadPage{}, err
	}
	entry.buildMu.Lock()
	defer entry.buildMu.Unlock()
	db, _, err := entry.ensureLocked(sources)
	if err != nil {
		return MessageThreadPage{}, err
	}
	return getMessageThreads(db, handles, search, limit, offset)
}

func (a *App) QueryMessages(caseName string, query MessageQuery) (MessagePage, error) {
	sources, entry, err := messageIndexForCase(caseName)
	if err != nil {
		return MessagePage{}, err
	}
	entry.buildMu.Lock()
	defer entry.buildMu.Unlock()
	db, _, err := entry.ensureLocked(sources)
	if err != nil {
		return MessagePage{}, err
	}
	page, err := queryMessages(db, query)
	if err != nil {
		return MessagePage{}, err
	}
	applyMessageRedactions(page.Messages, redactedMessageIDs(sources.Root))
	return page, nil
}

// ExportMessageQuery writes every matching message without passing the full
// result set through the Wails bridge. query.Limit and query.Offset are ignored.
func (a *App) ExportMessageQuery(caseName string, query MessageQuery, format string) (MessageExportResult, error) {
	sources, entry, err := messageIndexForCase(caseName)
	if err != nil {
		return MessageExportResult{}, err
	}
	entry.buildMu.Lock()
	defer entry.buildMu.Unlock()
	db, _, err := entry.ensureLocked(sources)
	if err != nil {
		return MessageExportResult{}, err
	}
	return exportMessageQuery(db, sources.Root, query, format, time.Now(), redactedMessageIDs(sources.Root))
}

func redactedMessageIDs(caseRoot string) map[string]bool {
	ids := readStringSlice(filepath.Join(caseRoot, "evidence", "cerberus", "redacted_messages.json"))
	set := make(map[string]bool, len(ids))
	for _, id := range ids {
		set[id] = true
	}
	return set
}

func applyMessageRedactions(messages []MessageRecord, redacted map[string]bool) {
	for i := range messages {
		if redacted[messages[i].ID] {
			text := "[REDACTED]"
			messages[i].Text = &text
		}
	}
}

func messageIndexForCase(caseName string) (messageSources, *messageIndexEntry, error) {
	root, err := resolveCaseRoot(caseName)
	if err != nil {
		return messageSources{}, nil, err
	}
	sources, err := locateMessageSources(caseName, root)
	if err != nil {
		return messageSources{}, nil, err
	}
	value, _ := messageIndexEntries.LoadOrStore(sources.Root, &messageIndexEntry{})
	return sources, value.(*messageIndexEntry), nil
}

func locateMessageSources(caseName, root string) (messageSources, error) {
	absRoot, err := filepath.Abs(root)
	if err != nil {
		return messageSources{}, err
	}
	if evaluated, err := filepath.EvalSymlinks(absRoot); err == nil {
		absRoot = evaluated
	}

	cerberusDir := filepath.Join(absRoot, "evidence", "cerberus")
	packageDir := filepath.Join(cerberusDir, "review_package")
	messages, err := statMessageSource(filepath.Join(packageDir, "messages.jsonl"), "messages")
	if err != nil {
		return messageSources{}, fmt.Errorf("Cerberus messages are not available for %s: %w", caseName, err)
	}
	threads, err := statMessageSource(filepath.Join(packageDir, "threads.jsonl"), "threads")
	if err != nil {
		return messageSources{}, fmt.Errorf("Cerberus thread summaries are not available for %s: %w", caseName, err)
	}

	var contacts messageSourceFile
	for _, candidate := range []struct{ name, kind string }{
		{"contact_identities.json", "identities"},
		{"contacts_full.json", "contacts_full"},
	} {
		if source, err := statMessageSource(filepath.Join(cerberusDir, candidate.name), candidate.kind); err == nil {
			contacts = source
			break
		}
	}

	sources := messageSources{
		CaseName: caseName,
		Root:     absRoot,
		Messages: messages,
		Threads:  threads,
		Contacts: contacts,
		Schema:   messageIndexSchemaVersion,
	}
	fingerprintInput, _ := json.Marshal(sources)
	sum := sha256.Sum256(fingerprintInput)
	sources.Fingerprint = fmt.Sprintf("%x", sum[:])
	return sources, nil
}

func statMessageSource(path, kind string) (messageSourceFile, error) {
	info, err := os.Stat(path)
	if err != nil {
		return messageSourceFile{}, err
	}
	if !info.Mode().IsRegular() {
		return messageSourceFile{}, fmt.Errorf("not a regular file: %s", path)
	}
	return messageSourceFile{Path: path, Size: info.Size(), ModTimeNano: info.ModTime().UnixNano(), Kind: kind}, nil
}

func messageIndexPath(root string) string {
	sum := sha256.Sum256([]byte(root))
	cacheRoot, err := os.UserCacheDir()
	if err != nil || strings.TrimSpace(cacheRoot) == "" {
		cacheRoot = filepath.Join(os.TempDir(), "ion-cache")
	}
	return filepath.Join(cacheRoot, "ion", "message-index", fmt.Sprintf("%x", sum[:16]), "index-v1.sqlite")
}

func baseMessageIndexStatus(sources messageSources) MessageIndexStatus {
	return MessageIndexStatus{
		CaseName:         sources.CaseName,
		SourcePath:       sources.Messages.Path,
		IndexPath:        messageIndexPath(sources.Root),
		SchemaVersion:    messageIndexSchemaVersion,
		SourceSizeBytes:  sources.Messages.Size,
		SourceModifiedAt: time.Unix(0, sources.Messages.ModTimeNano).Format(time.RFC3339Nano),
	}
}

func (entry *messageIndexEntry) setStatus(status MessageIndexStatus) {
	entry.statusMu.Lock()
	entry.status = status
	entry.statusMu.Unlock()
}

func (entry *messageIndexEntry) statusSnapshot() MessageIndexStatus {
	entry.statusMu.RLock()
	defer entry.statusMu.RUnlock()
	return entry.status
}

type messageIndexMetadata struct {
	IndexedAt    string
	MessageCount int64
	ThreadCount  int64
}

func (entry *messageIndexEntry) ensureLocked(sources messageSources) (*sql.DB, MessageIndexStatus, error) {
	if entry.db != nil && entry.fingerprint == sources.Fingerprint {
		return entry.db, entry.statusSnapshot(), nil
	}
	if entry.db != nil {
		_ = entry.db.Close()
		entry.db = nil
		entry.fingerprint = ""
	}

	indexPath := messageIndexPath(sources.Root)
	if db, metadata, ok := openValidMessageIndex(indexPath, sources.Fingerprint); ok {
		entry.db = db
		entry.fingerprint = sources.Fingerprint
		status := statusFromMetadata(sources, metadata, true)
		entry.setStatus(status)
		return db, status, nil
	} else if db != nil {
		_ = db.Close()
	}

	status := baseMessageIndexStatus(sources)
	status.State = "building"
	entry.setStatus(status)

	metadata, err := buildMessageIndex(sources, indexPath, func(processed int64) {
		progress := status
		progress.ProcessedMessages = processed
		entry.setStatus(progress)
	})
	if err != nil {
		status.State = "error"
		status.Error = err.Error()
		entry.setStatus(status)
		return nil, status, err
	}

	db, validated, ok := openValidMessageIndex(indexPath, sources.Fingerprint)
	if !ok {
		if db != nil {
			_ = db.Close()
		}
		err := errors.New("message index failed validation after build")
		status.State = "error"
		status.Error = err.Error()
		entry.setStatus(status)
		return nil, status, err
	}
	if validated.MessageCount != metadata.MessageCount || validated.ThreadCount != metadata.ThreadCount {
		_ = db.Close()
		err := errors.New("message index counts changed during validation")
		status.State = "error"
		status.Error = err.Error()
		entry.setStatus(status)
		return nil, status, err
	}

	entry.db = db
	entry.fingerprint = sources.Fingerprint
	status = statusFromMetadata(sources, validated, false)
	entry.setStatus(status)
	return db, status, nil
}

func statusFromMetadata(sources messageSources, metadata messageIndexMetadata, reused bool) MessageIndexStatus {
	status := baseMessageIndexStatus(sources)
	status.State = "ready"
	status.ProcessedMessages = metadata.MessageCount
	status.MessageCount = metadata.MessageCount
	status.ThreadCount = metadata.ThreadCount
	status.IndexedAt = metadata.IndexedAt
	status.Reused = reused
	return status
}

func openValidMessageIndex(path, fingerprint string) (*sql.DB, messageIndexMetadata, bool) {
	if _, err := os.Stat(path); err != nil {
		return nil, messageIndexMetadata{}, false
	}
	db, err := sql.Open("sqlite3", sqliteDSN(path, true))
	if err != nil {
		return nil, messageIndexMetadata{}, false
	}
	db.SetMaxOpenConns(4)
	if err := db.Ping(); err != nil {
		_ = db.Close()
		return nil, messageIndexMetadata{}, false
	}

	values := make(map[string]string)
	rows, err := db.Query("SELECT key, value FROM meta")
	if err != nil {
		_ = db.Close()
		return nil, messageIndexMetadata{}, false
	}
	for rows.Next() {
		var key, value string
		if err := rows.Scan(&key, &value); err != nil {
			_ = rows.Close()
			_ = db.Close()
			return nil, messageIndexMetadata{}, false
		}
		values[key] = value
	}
	if err := rows.Close(); err != nil {
		_ = db.Close()
		return nil, messageIndexMetadata{}, false
	}
	if values["schema_version"] != strconv.Itoa(messageIndexSchemaVersion) || values["source_fingerprint"] != fingerprint || values["build_complete"] != "1" {
		_ = db.Close()
		return nil, messageIndexMetadata{}, false
	}
	messageCount, err1 := strconv.ParseInt(values["message_count"], 10, 64)
	threadCount, err2 := strconv.ParseInt(values["thread_count"], 10, 64)
	if err1 != nil || err2 != nil {
		_ = db.Close()
		return nil, messageIndexMetadata{}, false
	}
	return db, messageIndexMetadata{IndexedAt: values["indexed_at"], MessageCount: messageCount, ThreadCount: threadCount}, true
}

func sqliteDSN(path string, readOnly bool) string {
	u := url.URL{Scheme: "file", Path: path}
	query := u.Query()
	query.Set("_busy_timeout", "5000")
	if readOnly {
		query.Set("mode", "ro")
		query.Set("_query_only", "1")
	}
	u.RawQuery = query.Encode()
	return u.String()
}

func buildMessageIndex(sources messageSources, finalPath string, progress func(int64)) (metadata messageIndexMetadata, resultErr error) {
	if err := os.MkdirAll(filepath.Dir(finalPath), 0700); err != nil {
		return metadata, fmt.Errorf("create message index cache directory: %w", err)
	}
	tmp, err := os.CreateTemp(filepath.Dir(finalPath), "index-build-*.sqlite")
	if err != nil {
		return metadata, err
	}
	tmpPath := tmp.Name()
	if err := tmp.Close(); err != nil {
		return metadata, err
	}
	if err := os.Remove(tmpPath); err != nil {
		return metadata, err
	}
	defer func() {
		if resultErr != nil {
			_ = os.Remove(tmpPath)
			_ = os.Remove(tmpPath + "-journal")
		}
	}()

	db, err := sql.Open("sqlite3", sqliteDSN(tmpPath, false))
	if err != nil {
		return metadata, err
	}
	closed := false
	defer func() {
		if !closed {
			_ = db.Close()
		}
	}()
	if _, err := db.Exec(`
		PRAGMA journal_mode = OFF;
		PRAGMA synchronous = OFF;
		PRAGMA temp_store = MEMORY;
		CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
		CREATE TABLE threads (
			thread_id TEXT PRIMARY KEY,
			chat_id INTEGER,
			chat_identifier TEXT,
			display_name TEXT,
			message_count INTEGER NOT NULL DEFAULT 0,
			attachment_count INTEGER NOT NULL DEFAULT 0,
			first_timestamp_utc TEXT,
			last_timestamp_utc TEXT,
			participants_json TEXT NOT NULL DEFAULT '[]',
			participant_names_json TEXT NOT NULL DEFAULT '[]',
			html_path TEXT NOT NULL DEFAULT ''
		);
		CREATE TABLE thread_participants (
			thread_id TEXT NOT NULL,
			handle TEXT NOT NULL,
			normalized_handle TEXT NOT NULL,
			display_name TEXT NOT NULL,
			PRIMARY KEY (thread_id, normalized_handle)
		);
		CREATE TABLE messages (
			source_line INTEGER PRIMARY KEY,
			stable_id TEXT NOT NULL,
			thread_id TEXT NOT NULL,
			message_id INTEGER NOT NULL,
			guid TEXT NOT NULL,
			timestamp_raw INTEGER,
			timestamp_utc TEXT,
			direction TEXT NOT NULL,
			handle TEXT NOT NULL,
			service TEXT NOT NULL,
			text TEXT,
			subject TEXT,
			chat_identifier TEXT,
			chat_display_name TEXT,
			attachment_count INTEGER NOT NULL,
			raw_json TEXT NOT NULL
		);
		CREATE INDEX messages_thread_idx ON messages(thread_id);
		CREATE INDEX messages_timestamp_idx ON messages(timestamp_utc, source_line);
		CREATE INDEX thread_participants_handle_idx ON thread_participants(normalized_handle, thread_id);
	`); err != nil {
		return metadata, fmt.Errorf("create message index schema: %w", err)
	}

	contactNames, _ := loadContactDisplayNames(sources.Contacts)
	tx, err := db.Begin()
	if err != nil {
		return metadata, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback()
		}
	}()

	threadStmt, err := tx.Prepare(`INSERT INTO threads (
		thread_id, chat_id, chat_identifier, display_name, message_count,
		attachment_count, first_timestamp_utc, last_timestamp_utc,
		participants_json, participant_names_json, html_path
	) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
	if err != nil {
		return metadata, err
	}
	participantStmt, err := tx.Prepare(`INSERT OR IGNORE INTO thread_participants
		(thread_id, handle, normalized_handle, display_name) VALUES (?, ?, ?, ?)`)
	if err != nil {
		_ = threadStmt.Close()
		return metadata, err
	}

	err = scanJSONLines[messageThreadJSONL](sources.Threads.Path, func(line int64, thread messageThreadJSONL, _ []byte) error {
		if strings.TrimSpace(thread.ThreadID) == "" {
			return fmt.Errorf("thread record %d has no thread_id", line)
		}
		names := make([]string, 0, len(thread.Participants))
		for _, handle := range thread.Participants {
			normalized := normalizeMessageIdentity(handle)
			name := contactNames[normalized]
			if name == "" {
				name = handle
			}
			names = append(names, name)
			if normalized != "" {
				if _, err := participantStmt.Exec(thread.ThreadID, handle, normalized, name); err != nil {
					return err
				}
			}
		}
		participantsJSON, _ := json.Marshal(thread.Participants)
		namesJSON, _ := json.Marshal(names)
		_, err := threadStmt.Exec(
			thread.ThreadID, nullableInt64(thread.ChatID), nullableString(thread.ChatIdentifier), nullableString(thread.DisplayName),
			thread.MessageCount, thread.AttachmentCount, nullableString(thread.FirstTimestampUTC), nullableString(thread.LastTimestampUTC),
			string(participantsJSON), string(namesJSON), thread.HTMLPath,
		)
		return err
	})
	_ = threadStmt.Close()
	_ = participantStmt.Close()
	if err != nil {
		return metadata, fmt.Errorf("index %s: %w", sources.Threads.Path, err)
	}

	messageStmt, err := tx.Prepare(`INSERT INTO messages (
		source_line, stable_id, thread_id, message_id, guid, timestamp_raw,
		timestamp_utc, direction, handle, service, text, subject,
		chat_identifier, chat_display_name, attachment_count, raw_json
	) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
	if err != nil {
		return metadata, err
	}
	ensureThreadStmt, err := tx.Prepare(`INSERT OR IGNORE INTO threads (thread_id) VALUES (?)`)
	if err != nil {
		_ = messageStmt.Close()
		return metadata, err
	}
	messageParticipantStmt, err := tx.Prepare(`INSERT OR IGNORE INTO thread_participants
		(thread_id, handle, normalized_handle, display_name) VALUES (?, ?, ?, ?)`)
	if err != nil {
		_ = messageStmt.Close()
		_ = ensureThreadStmt.Close()
		return metadata, err
	}

	var messageCount int64
	err = scanJSONLines[MessageRecord](sources.Messages.Path, func(line int64, message MessageRecord, raw []byte) error {
		if strings.TrimSpace(message.ThreadID) == "" {
			return fmt.Errorf("message record %d has no thread_id", line)
		}
		stableID := stableMessageID(message)
		if _, err := messageStmt.Exec(
			line, stableID, message.ThreadID, message.MessageID, message.GUID,
			nullableInt64(message.TimestampRaw), nullableString(message.TimestampUTC), message.Direction,
			message.Handle, message.Service, nullableString(message.Text), nullableString(message.Subject),
			nullableString(message.ChatIdentifier), nullableString(message.ChatDisplayName), message.AttachmentCount, string(raw),
		); err != nil {
			return err
		}
		if _, err := ensureThreadStmt.Exec(message.ThreadID); err != nil {
			return err
		}
		if normalized := normalizeMessageIdentity(message.Handle); normalized != "" {
			name := contactNames[normalized]
			if name == "" {
				name = message.Handle
			}
			if _, err := messageParticipantStmt.Exec(message.ThreadID, message.Handle, normalized, name); err != nil {
				return err
			}
		}
		messageCount++
		if messageCount%1000 == 0 {
			progress(messageCount)
		}
		return nil
	})
	_ = messageStmt.Close()
	_ = ensureThreadStmt.Close()
	_ = messageParticipantStmt.Close()
	if err != nil {
		return metadata, fmt.Errorf("index %s: %w", sources.Messages.Path, err)
	}
	progress(messageCount)

	if _, err := tx.Exec(`UPDATE threads SET
		message_count = (SELECT COUNT(*) FROM messages m WHERE m.thread_id = threads.thread_id),
		attachment_count = COALESCE((SELECT SUM(m.attachment_count) FROM messages m WHERE m.thread_id = threads.thread_id), 0),
		first_timestamp_utc = COALESCE((SELECT MIN(NULLIF(m.timestamp_utc, '')) FROM messages m WHERE m.thread_id = threads.thread_id), first_timestamp_utc),
		last_timestamp_utc = COALESCE((SELECT MAX(NULLIF(m.timestamp_utc, '')) FROM messages m WHERE m.thread_id = threads.thread_id), last_timestamp_utc)`); err != nil {
		return metadata, err
	}
	var threadCount int64
	if err := tx.QueryRow("SELECT COUNT(*) FROM threads").Scan(&threadCount); err != nil {
		return metadata, err
	}
	indexedAt := time.Now().UTC().Format(time.RFC3339Nano)
	meta := map[string]string{
		"schema_version":     strconv.Itoa(messageIndexSchemaVersion),
		"source_fingerprint": sources.Fingerprint,
		"indexed_at":         indexedAt,
		"message_count":      strconv.FormatInt(messageCount, 10),
		"thread_count":       strconv.FormatInt(threadCount, 10),
		"build_complete":     "1",
	}
	metaStmt, err := tx.Prepare("INSERT INTO meta (key, value) VALUES (?, ?)")
	if err != nil {
		return metadata, err
	}
	for key, value := range meta {
		if _, err := metaStmt.Exec(key, value); err != nil {
			_ = metaStmt.Close()
			return metadata, err
		}
	}
	if err := metaStmt.Close(); err != nil {
		return metadata, err
	}
	if err := tx.Commit(); err != nil {
		return metadata, err
	}
	committed = true
	if err := db.Close(); err != nil {
		return metadata, err
	}
	closed = true
	if err := os.Chmod(tmpPath, 0600); err != nil {
		return metadata, err
	}
	if err := os.Rename(tmpPath, finalPath); err != nil {
		return metadata, fmt.Errorf("publish message index: %w", err)
	}
	return messageIndexMetadata{IndexedAt: indexedAt, MessageCount: messageCount, ThreadCount: threadCount}, nil
}

func scanJSONLines[T any](path string, fn func(line int64, value T, raw []byte) error) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 64*1024), 64*1024*1024)
	var line int64
	for scanner.Scan() {
		line++
		raw := scanner.Bytes()
		if len(strings.TrimSpace(string(raw))) == 0 {
			continue
		}
		var value T
		if err := json.Unmarshal(raw, &value); err != nil {
			return fmt.Errorf("line %d: %w", line, err)
		}
		if err := fn(line, value, raw); err != nil {
			return fmt.Errorf("line %d: %w", line, err)
		}
	}
	return scanner.Err()
}

func loadContactDisplayNames(source messageSourceFile) (map[string]string, error) {
	names := make(map[string]string)
	if source.Path == "" {
		return names, nil
	}
	file, err := os.Open(source.Path)
	if err != nil {
		return names, err
	}
	defer file.Close()
	decoder := json.NewDecoder(file)
	token, err := decoder.Token()
	if err != nil {
		return names, err
	}
	if delimiter, ok := token.(json.Delim); !ok || delimiter != '[' {
		return names, errors.New("contact identity file is not a JSON array")
	}

	if source.Kind == "identities" {
		for decoder.More() {
			var identity struct {
				Name      string `json:"contact_name"`
				Value     string `json:"value"`
				ValueType string `json:"value_type"`
			}
			if err := decoder.Decode(&identity); err != nil {
				return names, err
			}
			rememberContactName(names, identity.Value, identity.Name)
		}
	} else {
		for decoder.More() {
			var contact struct {
				Name          string   `json:"name"`
				Phones        []string `json:"phones"`
				Emails        []string `json:"emails"`
				LabeledValues []struct {
					Value string `json:"value"`
				} `json:"labeled_values"`
			}
			if err := decoder.Decode(&contact); err != nil {
				return names, err
			}
			for _, value := range append(contact.Phones, contact.Emails...) {
				rememberContactName(names, value, contact.Name)
			}
			for _, labeled := range contact.LabeledValues {
				rememberContactName(names, labeled.Value, contact.Name)
			}
		}
	}
	_, err = decoder.Token()
	return names, err
}

func rememberContactName(names map[string]string, value, name string) {
	key := normalizeMessageIdentity(value)
	name = strings.TrimSpace(name)
	if key == "" || name == "" {
		return
	}
	current := names[key]
	if current == "" || normalizeMessageIdentity(current) == key {
		names[key] = name
	}
}

func normalizeMessageIdentity(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	lower := strings.ToLower(value)
	if strings.Contains(lower, "@") {
		return lower
	}
	var digits strings.Builder
	for _, r := range value {
		if r >= '0' && r <= '9' {
			digits.WriteRune(r)
		}
	}
	if digits.Len() > 0 {
		phone := digits.String()
		if len(phone) > 10 {
			phone = phone[len(phone)-10:]
		}
		return phone
	}
	return lower
}

func stableMessageID(message MessageRecord) string {
	if strings.TrimSpace(message.GUID) != "" {
		return "guid:" + message.GUID
	}
	return fmt.Sprintf("message:%d", message.MessageID)
}

func nullableString(value *string) any {
	if value == nil {
		return nil
	}
	return *value
}

func nullableInt64(value *int64) any {
	if value == nil {
		return nil
	}
	return *value
}

func getMessageContacts(db *sql.DB, search string, limit, offset int) (MessageContactPage, error) {
	limit = clampPageLimit(limit, defaultSummaryPageSize, maxSummaryPageSize)
	if offset < 0 {
		offset = 0
	}
	where, args := participantSearchWhere("p", search)
	var total int64
	countSQL := "SELECT COUNT(DISTINCT p.normalized_handle) FROM thread_participants p" + where
	if err := db.QueryRow(countSQL, args...).Scan(&total); err != nil {
		return MessageContactPage{}, err
	}

	querySQL := `SELECT
		MIN(p.handle), p.normalized_handle,
		MAX(CASE WHEN p.display_name <> '' THEN p.display_name ELSE p.handle END),
		COUNT(DISTINCT p.thread_id), COUNT(m.source_line),
		MIN(NULLIF(m.timestamp_utc, '')), MAX(NULLIF(m.timestamp_utc, ''))
		FROM thread_participants p
		JOIN messages m ON m.thread_id = p.thread_id` + where + `
		GROUP BY p.normalized_handle
		ORDER BY LOWER(MAX(CASE WHEN p.display_name <> '' THEN p.display_name ELSE p.handle END)), MIN(p.handle)
		LIMIT ? OFFSET ?`
	args = append(args, limit, offset)
	rows, err := db.Query(querySQL, args...)
	if err != nil {
		return MessageContactPage{}, err
	}
	defer rows.Close()
	contacts := make([]MessageContactSummary, 0, limit)
	for rows.Next() {
		var contact MessageContactSummary
		var first, last sql.NullString
		if err := rows.Scan(&contact.Handle, &contact.NormalizedHandle, &contact.DisplayName, &contact.ThreadCount, &contact.MessageCount, &first, &last); err != nil {
			return MessageContactPage{}, err
		}
		contact.FirstTimestampUTC = nullStringPointer(first)
		contact.LastTimestampUTC = nullStringPointer(last)
		contacts = append(contacts, contact)
	}
	if err := rows.Err(); err != nil {
		return MessageContactPage{}, err
	}
	return MessageContactPage{Contacts: contacts, Total: total, Limit: limit, Offset: offset, HasMore: int64(offset+len(contacts)) < total}, nil
}

func getMessageThreads(db *sql.DB, handles []string, search string, limit, offset int) (MessageThreadPage, error) {
	limit = clampPageLimit(limit, defaultSummaryPageSize, maxSummaryPageSize)
	if offset < 0 {
		offset = 0
	}
	where, args := threadWhere(handles, search)
	var total int64
	if err := db.QueryRow("SELECT COUNT(*) FROM threads t"+where, args...).Scan(&total); err != nil {
		return MessageThreadPage{}, err
	}
	querySQL := `SELECT t.thread_id, t.chat_id, t.chat_identifier, t.display_name,
		t.message_count, t.attachment_count, t.first_timestamp_utc,
		t.last_timestamp_utc, t.participants_json, t.participant_names_json, t.html_path
		FROM threads t` + where + `
		ORDER BY COALESCE(t.last_timestamp_utc, '') DESC, t.thread_id
		LIMIT ? OFFSET ?`
	args = append(args, limit, offset)
	rows, err := db.Query(querySQL, args...)
	if err != nil {
		return MessageThreadPage{}, err
	}
	defer rows.Close()
	threads := make([]MessageThreadSummary, 0, limit)
	for rows.Next() {
		var thread MessageThreadSummary
		var chatID sql.NullInt64
		var chatIdentifier, displayName, first, last sql.NullString
		var participantsJSON, participantNamesJSON string
		if err := rows.Scan(
			&thread.ThreadID, &chatID, &chatIdentifier, &displayName,
			&thread.MessageCount, &thread.AttachmentCount, &first, &last,
			&participantsJSON, &participantNamesJSON, &thread.HTMLPath,
		); err != nil {
			return MessageThreadPage{}, err
		}
		thread.ChatID = nullInt64Pointer(chatID)
		thread.ChatIdentifier = nullStringPointer(chatIdentifier)
		thread.DisplayName = nullStringPointer(displayName)
		thread.FirstTimestampUTC = nullStringPointer(first)
		thread.LastTimestampUTC = nullStringPointer(last)
		if err := json.Unmarshal([]byte(participantsJSON), &thread.Participants); err != nil {
			return MessageThreadPage{}, err
		}
		if err := json.Unmarshal([]byte(participantNamesJSON), &thread.ParticipantNames); err != nil {
			return MessageThreadPage{}, err
		}
		threads = append(threads, thread)
	}
	if err := rows.Err(); err != nil {
		return MessageThreadPage{}, err
	}
	return MessageThreadPage{Threads: threads, Total: total, Limit: limit, Offset: offset, HasMore: int64(offset+len(threads)) < total}, nil
}

func queryMessages(db *sql.DB, query MessageQuery) (MessagePage, error) {
	query.Limit = clampPageLimit(query.Limit, defaultMessagePageSize, maxMessagePageSize)
	if query.Offset < 0 {
		query.Offset = 0
	}
	where, args := messageWhere(query)
	var total int64
	if err := db.QueryRow("SELECT COUNT(*) FROM messages m"+where, args...).Scan(&total); err != nil {
		return MessagePage{}, err
	}
	selectSQL := `SELECT m.raw_json, m.stable_id FROM messages m` + where + `
		ORDER BY CASE WHEN m.timestamp_utc IS NULL OR m.timestamp_utc = '' THEN 1 ELSE 0 END,
		m.timestamp_utc, m.source_line LIMIT ? OFFSET ?`
	args = append(args, query.Limit, query.Offset)
	rows, err := db.Query(selectSQL, args...)
	if err != nil {
		return MessagePage{}, err
	}
	defer rows.Close()
	messages := make([]MessageRecord, 0, query.Limit)
	for rows.Next() {
		var raw, stableID string
		if err := rows.Scan(&raw, &stableID); err != nil {
			return MessagePage{}, err
		}
		var message MessageRecord
		if err := json.Unmarshal([]byte(raw), &message); err != nil {
			return MessagePage{}, err
		}
		message.ID = stableID
		messages = append(messages, message)
	}
	if err := rows.Err(); err != nil {
		return MessagePage{}, err
	}
	next := query.Offset + len(messages)
	return MessagePage{
		Messages: messages, Total: total, Limit: query.Limit, Offset: query.Offset,
		NextOffset: next, HasMore: int64(next) < total,
	}, nil
}

func participantSearchWhere(alias, search string) (string, []any) {
	search = strings.TrimSpace(search)
	if search == "" {
		return "", nil
	}
	pattern := likePattern(search)
	return fmt.Sprintf(` WHERE (LOWER(%s.handle) LIKE ? ESCAPE '\' OR LOWER(%s.display_name) LIKE ? ESCAPE '\' OR LOWER(%s.normalized_handle) LIKE ? ESCAPE '\')`, alias, alias, alias), []any{pattern, pattern, pattern}
}

func threadWhere(handles []string, search string) (string, []any) {
	clauses := make([]string, 0, 2)
	args := make([]any, 0)
	normalized := normalizedHandles(handles)
	if len(normalized) > 0 {
		placeholders := strings.TrimSuffix(strings.Repeat("?,", len(normalized)), ",")
		clauses = append(clauses, "EXISTS (SELECT 1 FROM thread_participants selected WHERE selected.thread_id = t.thread_id AND selected.normalized_handle IN ("+placeholders+"))")
		for _, handle := range normalized {
			args = append(args, handle)
		}
	}
	if strings.TrimSpace(search) != "" {
		pattern := likePattern(search)
		clauses = append(clauses, `(LOWER(t.thread_id) LIKE ? ESCAPE '\' OR LOWER(COALESCE(t.chat_identifier, '')) LIKE ? ESCAPE '\' OR LOWER(COALESCE(t.display_name, '')) LIKE ? ESCAPE '\' OR EXISTS (SELECT 1 FROM thread_participants searched WHERE searched.thread_id = t.thread_id AND (LOWER(searched.handle) LIKE ? ESCAPE '\' OR LOWER(searched.display_name) LIKE ? ESCAPE '\')) OR EXISTS (SELECT 1 FROM messages searched_message WHERE searched_message.thread_id = t.thread_id AND (LOWER(COALESCE(searched_message.text, '')) LIKE ? ESCAPE '\' OR LOWER(COALESCE(searched_message.subject, '')) LIKE ? ESCAPE '\' OR LOWER(searched_message.handle) LIKE ? ESCAPE '\' OR LOWER(searched_message.service) LIKE ? ESCAPE '\' OR LOWER(searched_message.guid) LIKE ? ESCAPE '\' OR LOWER(COALESCE(searched_message.chat_identifier, '')) LIKE ? ESCAPE '\' OR LOWER(COALESCE(searched_message.chat_display_name, '')) LIKE ? ESCAPE '\')))`)
		args = append(args, pattern, pattern, pattern, pattern, pattern, pattern, pattern, pattern, pattern, pattern, pattern, pattern)
	}
	if len(clauses) == 0 {
		return "", args
	}
	return " WHERE " + strings.Join(clauses, " AND "), args
}

func messageWhere(query MessageQuery) (string, []any) {
	clauses := make([]string, 0, 3)
	args := make([]any, 0)
	normalized := normalizedHandles(query.Handles)
	if len(normalized) > 0 {
		placeholders := strings.TrimSuffix(strings.Repeat("?,", len(normalized)), ",")
		clauses = append(clauses, "EXISTS (SELECT 1 FROM thread_participants selected WHERE selected.thread_id = m.thread_id AND selected.normalized_handle IN ("+placeholders+"))")
		for _, handle := range normalized {
			args = append(args, handle)
		}
	}
	if strings.TrimSpace(query.ThreadID) != "" {
		clauses = append(clauses, "m.thread_id = ?")
		args = append(args, strings.TrimSpace(query.ThreadID))
	}
	if strings.TrimSpace(query.Search) != "" {
		pattern := likePattern(query.Search)
		clauses = append(clauses, `(LOWER(COALESCE(m.text, '')) LIKE ? ESCAPE '\' OR LOWER(COALESCE(m.subject, '')) LIKE ? ESCAPE '\' OR LOWER(m.handle) LIKE ? ESCAPE '\' OR LOWER(m.service) LIKE ? ESCAPE '\' OR LOWER(m.guid) LIKE ? ESCAPE '\' OR LOWER(COALESCE(m.chat_identifier, '')) LIKE ? ESCAPE '\' OR LOWER(COALESCE(m.chat_display_name, '')) LIKE ? ESCAPE '\')`)
		args = append(args, pattern, pattern, pattern, pattern, pattern, pattern, pattern)
	}
	if len(clauses) == 0 {
		return "", args
	}
	return " WHERE " + strings.Join(clauses, " AND "), args
}

func normalizedHandles(handles []string) []string {
	seen := make(map[string]struct{}, len(handles))
	result := make([]string, 0, len(handles))
	for _, handle := range handles {
		normalized := normalizeMessageIdentity(handle)
		if normalized == "" {
			continue
		}
		if _, ok := seen[normalized]; ok {
			continue
		}
		seen[normalized] = struct{}{}
		result = append(result, normalized)
	}
	return result
}

func likePattern(search string) string {
	value := strings.ToLower(strings.TrimSpace(search))
	value = strings.ReplaceAll(value, `\`, `\\`)
	value = strings.ReplaceAll(value, `%`, `\%`)
	value = strings.ReplaceAll(value, `_`, `\_`)
	return "%" + value + "%"
}

func clampPageLimit(limit, fallback, maximum int) int {
	if limit <= 0 {
		return fallback
	}
	if limit > maximum {
		return maximum
	}
	return limit
}

func nullStringPointer(value sql.NullString) *string {
	if !value.Valid {
		return nil
	}
	copy := value.String
	return &copy
}

func nullInt64Pointer(value sql.NullInt64) *int64 {
	if !value.Valid {
		return nil
	}
	copy := value.Int64
	return &copy
}

func exportMessageQuery(db *sql.DB, caseRoot string, query MessageQuery, format string, now time.Time, redacted map[string]bool) (result MessageExportResult, resultErr error) {
	format = strings.ToLower(strings.TrimSpace(format))
	if format != "json" && format != "csv" {
		return result, fmt.Errorf("unsupported message export format: %s", format)
	}
	exportDir := filepath.Join(caseRoot, "exports", "cerberus")
	if err := os.MkdirAll(exportDir, 0750); err != nil {
		return result, err
	}
	tmp, err := os.CreateTemp(exportDir, ".messages-export-*")
	if err != nil {
		return result, err
	}
	tmpPath := tmp.Name()
	closed := false
	defer func() {
		if !closed {
			_ = tmp.Close()
		}
		if resultErr != nil {
			_ = os.Remove(tmpPath)
		}
	}()

	where, args := messageWhere(query)
	rows, err := db.Query(`SELECT m.raw_json, m.stable_id FROM messages m`+where+`
		ORDER BY CASE WHEN m.timestamp_utc IS NULL OR m.timestamp_utc = '' THEN 1 ELSE 0 END,
		m.timestamp_utc, m.source_line`, args...)
	if err != nil {
		return result, err
	}
	defer rows.Close()

	var count int64
	if format == "json" {
		if _, err := io.WriteString(tmp, "[\n"); err != nil {
			return result, err
		}
		first := true
		for rows.Next() {
			var raw, stableID string
			if err := rows.Scan(&raw, &stableID); err != nil {
				return result, err
			}
			if !first {
				if _, err := io.WriteString(tmp, ",\n"); err != nil {
					return result, err
				}
			}
			first = false
			if redacted[stableID] {
				var payload map[string]interface{}
				if err := json.Unmarshal([]byte(raw), &payload); err != nil {
					return result, err
				}
				payload["text"] = "[REDACTED]"
				encoded, err := json.Marshal(payload)
				if err != nil {
					return result, err
				}
				raw = string(encoded)
			}
			if _, err := io.WriteString(tmp, raw); err != nil {
				return result, err
			}
			count++
		}
		if _, err := io.WriteString(tmp, "\n]\n"); err != nil {
			return result, err
		}
	} else {
		writer := csv.NewWriter(tmp)
		if err := writer.Write(messageCSVHeader()); err != nil {
			return result, err
		}
		for rows.Next() {
			var raw, stableID string
			if err := rows.Scan(&raw, &stableID); err != nil {
				return result, err
			}
			var message MessageRecord
			if err := json.Unmarshal([]byte(raw), &message); err != nil {
				return result, err
			}
			if redacted[stableID] {
				text := "[REDACTED]"
				message.Text = &text
			}
			if err := writer.Write(messageCSVRow(message)); err != nil {
				return result, err
			}
			count++
		}
		writer.Flush()
		if err := writer.Error(); err != nil {
			return result, err
		}
	}
	if err := rows.Err(); err != nil {
		return result, err
	}
	if err := tmp.Sync(); err != nil {
		return result, err
	}
	if err := tmp.Close(); err != nil {
		return result, err
	}
	closed = true
	filename := fmt.Sprintf("messages-%s.%s", now.Format("20060102-150405.000000000"), format)
	finalPath := filepath.Join(exportDir, filename)
	if err := os.Rename(tmpPath, finalPath); err != nil {
		return result, err
	}
	return MessageExportResult{Path: finalPath, Count: count, Format: format}, nil
}

func messageCSVHeader() []string {
	return []string{
		"thread_id", "chat_id", "chat_identifier", "chat_display_name", "message_id", "guid",
		"timestamp_utc", "direction", "handle", "service", "text", "subject", "attachment_count",
		"attachment_export_paths", "attachment_mime_types", "is_read", "is_delivered", "is_sent",
		"is_audio_message", "reply_to_guid", "balloon_bundle_id",
	}
}

func messageCSVRow(message MessageRecord) []string {
	return []string{
		message.ThreadID,
		formatOptionalInt(message.ChatID),
		formatOptionalString(message.ChatIdentifier),
		formatOptionalString(message.ChatDisplayName),
		strconv.FormatInt(message.MessageID, 10),
		message.GUID,
		formatOptionalString(message.TimestampUTC),
		message.Direction,
		message.Handle,
		message.Service,
		formatOptionalString(message.Text),
		formatOptionalString(message.Subject),
		strconv.Itoa(message.AttachmentCount),
		strings.Join(message.AttachmentExportPaths, "|"),
		strings.Join(message.AttachmentMIMETypes, "|"),
		strconv.FormatBool(message.IsRead),
		strconv.FormatBool(message.IsDelivered),
		strconv.FormatBool(message.IsSent),
		strconv.FormatBool(message.IsAudioMessage),
		formatOptionalString(message.ReplyToGUID),
		formatOptionalString(message.BalloonBundleID),
	}
}

func formatOptionalString(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

func formatOptionalInt(value *int64) string {
	if value == nil {
		return ""
	}
	return strconv.FormatInt(*value, 10)
}
