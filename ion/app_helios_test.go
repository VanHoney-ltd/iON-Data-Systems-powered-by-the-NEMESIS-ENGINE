package main

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestResolveIOSBackupRootDirect(t *testing.T) {
	root := fakeIOSBackupRoot(t, t.TempDir())

	resolved, err := resolveIOSBackupRoot(root)
	if err != nil {
		t.Fatalf("resolveIOSBackupRoot failed: %v", err)
	}
	if resolved != root {
		t.Fatalf("resolved=%s, want %s", resolved, root)
	}
}

func TestResolveIOSBackupRootFromCaseWrapper(t *testing.T) {
	wrapper := t.TempDir()
	older := fakeIOSBackupRoot(t, filepath.Join(wrapper, "old-device"))
	newer := fakeIOSBackupRoot(t, filepath.Join(wrapper, "00008110-00050D5C0E20201E"))
	if err := os.Chtimes(older, mustTime(t, "2026-01-01T00:00:00Z"), mustTime(t, "2026-01-01T00:00:00Z")); err != nil {
		t.Fatalf("set older mtime: %v", err)
	}
	if err := os.Chtimes(newer, mustTime(t, "2026-02-01T00:00:00Z"), mustTime(t, "2026-02-01T00:00:00Z")); err != nil {
		t.Fatalf("set newer mtime: %v", err)
	}

	resolved, err := resolveIOSBackupRoot(wrapper)
	if err != nil {
		t.Fatalf("resolveIOSBackupRoot failed: %v", err)
	}
	if resolved != newer {
		t.Fatalf("resolved=%s, want %s", resolved, newer)
	}
}

func fakeIOSBackupRoot(t *testing.T, root string) string {
	t.Helper()
	if err := os.MkdirAll(root, 0755); err != nil {
		t.Fatalf("create fake backup root: %v", err)
	}
	for _, name := range []string{"Manifest.plist", "Info.plist", "Status.plist"} {
		if err := os.WriteFile(filepath.Join(root, name), []byte("fake"), 0644); err != nil {
			t.Fatalf("write %s: %v", name, err)
		}
	}
	return root
}

func mustTime(t *testing.T, value string) time.Time {
	t.Helper()
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		t.Fatalf("parse time %s: %v", value, err)
	}
	return parsed
}
