package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCreateCaseFromBackup(t *testing.T) {
	// Create a fake iOS backup directory.
	backupDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(backupDir, "Manifest.plist"), []byte("fake"), 0644); err != nil {
		t.Fatalf("create fake manifest: %v", err)
	}

	app := NewApp()
	caseName, err := app.CreateCaseFromBackup("test-real-backup", backupDir)
	if err != nil {
		t.Fatalf("CreateCaseFromBackup failed: %v", err)
	}
	if caseName != "test-real-backup" {
		t.Fatalf("unexpected case name: %s", caseName)
	}

	// Locate the symlink and verify it points at the backup.
	root, err := findProjectRoot()
	if err != nil {
		t.Fatalf("findProjectRoot failed: %v", err)
	}
	linkPath := filepath.Join(root, "cases", "test-real-backup", "backup")
	target, err := os.Readlink(linkPath)
	if err != nil {
		t.Fatalf("read backup symlink: %v", err)
	}
	if target != backupDir {
		t.Fatalf("backup symlink points to %s, want %s", target, backupDir)
	}
}
