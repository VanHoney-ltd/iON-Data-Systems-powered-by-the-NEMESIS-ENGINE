package main

import (
	"testing"
)

func TestGetInstalledApps_CleanBackup(t *testing.T) {
	app := NewApp()
	apps, err := app.GetInstalledApps("ui-test-clean-backup")
	if err != nil {
		t.Fatalf("GetInstalledApps failed: %v", err)
	}
	if len(apps) == 0 {
		t.Fatal("expected at least one installed app")
	}
	t.Logf("found %d apps", len(apps))
	for _, a := range apps[:5] {
		t.Logf("  %s (%s) - %s", a.BundleID, a.Name, a.DataSize)
	}
}

func TestGetInstalledApps_TestyyEncrypted(t *testing.T) {
	// testyy exists but is encrypted; this confirms we detect the backup
	// without crashing even when we cannot decrypt it yet.
	app := NewApp()
	apps, err := app.GetInstalledApps("testyy")
	if err == nil && len(apps) > 0 {
		t.Logf("testyy unencrypted and has %d apps", len(apps))
		return
	}
	t.Logf("testyy not decryptable yet: %v", err)
}

func TestGetInstalledApps_Pcr(t *testing.T) {
	app := NewApp()
	apps, err := app.GetInstalledApps("pcr")
	if err != nil {
		t.Fatalf("GetInstalledApps failed: %v", err)
	}
	if len(apps) == 0 {
		t.Fatal("expected pcr case to have installed apps")
	}
	t.Logf("pcr has %d apps", len(apps))
	for _, a := range apps[:5] {
		t.Logf("  %s (%s) - %s", a.BundleID, a.Name, a.ContainerPath)
	}
}

func TestSearchAppData(t *testing.T) {
	app := NewApp()
	hits, err := app.SearchAppData("pcr", "5153806644")
	if err != nil {
		t.Fatalf("SearchAppData failed: %v", err)
	}
	t.Logf("found %d hits for 5153806644", len(hits))
	for _, h := range hits[:min(5, len(hits))] {
		t.Logf("  %s: %s", h.AppName, h.FilePath)
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
