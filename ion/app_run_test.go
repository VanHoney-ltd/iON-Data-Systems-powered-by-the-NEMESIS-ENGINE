package main

import (
	"os"
	"path/filepath"
	"testing"
)

// TestCreateAndRunVigilOnCleanBackup creates a case from the small desktop
// backup and runs the vigil agent synchronously to confirm the Rust core finds
// the case and produces output.
func TestCreateAndRunVigilOnCleanBackup(t *testing.T) {
	backup := "/home/ghost/Desktop/ios-backup-clean/0460cc80cf848fb5f518e18ca81726ae6bea909d"
	if _, err := os.Stat(backup); err != nil {
		t.Skip("clean desktop backup not found")
	}

	app := NewApp()
	caseName := "ui-test-clean-backup"
	if _, err := app.CreateCaseFromBackup(caseName, backup); err != nil {
		t.Fatalf("create case: %v", err)
	}

	root, err := resolveCaseRoot(caseName)
	if err != nil {
		t.Fatalf("resolve case root: %v", err)
	}

	// Clean any stale evidence so we can verify a fresh run.
	_ = os.RemoveAll(filepath.Join(root, "evidence", "vigil"))

	result := app.ProcessBackup("vigil", caseName)
	if len(result) == 0 {
		t.Fatalf("ProcessBackup returned no output")
	}

	// The vigil evidence directory should now exist.
	if _, err := os.Stat(filepath.Join(root, "evidence", "vigil", "summary.json")); err != nil {
		t.Fatalf("vigil summary not created: %v", err)
	}
}
