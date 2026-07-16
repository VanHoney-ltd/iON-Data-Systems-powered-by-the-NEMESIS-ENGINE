package main

import (
	"strings"
	"testing"
)

func TestParseAcquisitionProgress(t *testing.T) {
	tests := []struct {
		name    string
		line    string
		overall int
		file    int
		ok      bool
	}{
		{
			name:    "overall progress",
			line:    "[idevicebackup2] Overall progress: 42%",
			overall: 42,
			file:    42,
			ok:      true,
		},
		{
			name:    "file progress",
			line:    "[idevicebackup2] File progress: 87%",
			overall: 87,
			file:    87,
			ok:      true,
		},
		{
			name:    "generic percent",
			line:    "[idevicebackup2] Receiving file CameraRollDomain/DCIM/100APPLE/IMG_0001.JPG 13%",
			overall: 13,
			file:    13,
			ok:      true,
		},
		{
			name:    "current file and overall percent",
			line:    "[idevicebackup2] [=                                                 ] 0% (64 Bytes/11.3 MB) [================================================= ] 99% (10.9 GB/11.0 GB)",
			overall: 99,
			file:    0,
			ok:      true,
		},
		{
			name: "no percent",
			line: "[idevicebackup2] Started com.apple.mobilebackup2 service",
			ok:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			progress, ok := parseAcquisitionProgress(tt.line)
			if ok != tt.ok {
				t.Fatalf("ok=%v, want %v", ok, tt.ok)
			}
			if !ok {
				return
			}
			if progress.OverallPercent != tt.overall {
				t.Fatalf("overall=%d, want %d", progress.OverallPercent, tt.overall)
			}
			if progress.FilePercent != tt.file {
				t.Fatalf("file=%d, want %d", progress.FilePercent, tt.file)
			}
			if progress.Label == "" {
				t.Fatalf("expected non-empty label")
			}
		})
	}
}

func TestReadChronosLinesAllowsLongLines(t *testing.T) {
	longLine := strings.Repeat("x", 80*1024) + "\nshort\n"
	var lines []string
	if err := readChronosLines(strings.NewReader(longLine), func(line string) {
		lines = append(lines, line)
	}); err != nil {
		t.Fatalf("readChronosLines failed: %v", err)
	}
	if len(lines) != 2 {
		t.Fatalf("got %d lines, want 2", len(lines))
	}
	if len(lines[0]) != 80*1024 {
		t.Fatalf("first line length=%d, want %d", len(lines[0]), 80*1024)
	}
	if lines[1] != "short" {
		t.Fatalf("second line=%q, want short", lines[1])
	}
}

func TestShouldLogChronosLine(t *testing.T) {
	tests := []struct {
		name string
		line string
		want bool
	}{
		{
			name: "keeps errors",
			line: "[idevicebackup2] Could not perform backup protocol version exchange, error code -1",
			want: true,
		},
		{
			name: "drops service chatter",
			line: `[idevicebackup2] Started "com.apple.mobilebackup2" service on port 58188.`,
			want: false,
		},
		{
			name: "drops protocol chatter",
			line: "[idevicebackup2] Negotiated Protocol Version 2.1",
			want: false,
		},
		{
			name: "keeps pairing milestone",
			line: "📱 Step 1/3: Pairing device...",
			want: true,
		},
		{
			name: "drops blank lines",
			line: "   ",
			want: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := shouldLogChronosLine(tt.line); got != tt.want {
				t.Fatalf("shouldLogChronosLine()=%v, want %v", got, tt.want)
			}
		})
	}
}
