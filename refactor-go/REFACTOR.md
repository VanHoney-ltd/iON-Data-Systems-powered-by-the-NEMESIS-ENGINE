# STYGiON Refactor – Authoritative Specification

**Last Updated**: April 30, 2026  
**Status**: Active – All agents and developers must follow this document as the single source of truth.

## Project Vision

STYGiON is a high-performance, privacy-first desktop application for browsing, searching, and managing large encrypted personal backups (up to 90GB+ per backup). It combines a blazing-fast Rust core with a modern, responsive Go + Svelte 5 desktop UI.

**Core Goals**

- Extreme performance on large encrypted datasets
- Zero trust architecture (decryption happens only in memory, never persisted unencrypted)
- Smooth UI even with 14,000+ media items
- Full streaming architecture – never load entire backups or large files into memory
- Maintainable, well-separated codebase across Rust, Go, and Svelte 5

---

## Target Architecture

### 1. Rust CLI Core (`stygion-cli`)

**Responsibility**: All heavy computation and I/O.
ng only (range requests supported)
Memory usage must remain low even with 90GB backups

3. Svelte 5 Frontend
   Responsibility: All user interface and interaction.

Built with Svelte 5 Runes only ($state, $derived, $effect)
No legacy Svelte 3/4 syntax or stores where runes can replace them
Fully virtualized lists and galleries (14k+ items at 60 fps)
Fast search with real-time filtering
Agent/debug views for development

Performance Requirements

Gallery must render smoothly with thousands of thumbnails
Search results update without jank
Lazy loading + virtualization mandatory
Proper memory management (release large objects when not needed)

Authoritative Constraints (Non-Negotiable)

Memory Safety
Never load entire backups or large media files into memory
Streaming only for all large data

Performance
UI must remain responsive during heavy Rust operations
Target: < 100ms UI response time for search/filter

Technology Rules
Rust: Latest stable + idiomatic code
Go: Wails v2 or v3 (latest), clean architecture
Svelte: Svelte 5 runes exclusively – no onMount, stores, etc. unless absolutely necessary
No external network calls after initial setup (offline-first)

Security
Decryption keys never persisted
All decrypted content stays in memory only as long as needed

Agent Rules
See AGENTS.md for strict separation of responsibilities
Big Pickle plans only – never writes production code
Big Pickle implements only – never changes architecture

Success Metrics

Can open and browse a 90GB encrypted backup in < 15 seconds (cold start)
Gallery scrolls at 60 fps with 14,000+ items
Memory usage < 800 MB peak while viewing large backups
Full search across metadata completes in < 300 ms
All core flows work offline
90%+ unit + integration test coverage for Rust core

Current Refactor Priorities (Ordered)

Stabilize Rust CLI streaming + progress reporting
Complete Go streaming pipeline and asset serving
Implement high-performance Svelte 5 virtualized gallery
Add robust search and filtering
Polish UI/UX and error handling
Add comprehensive testing suite
Performance tuning and edge-case handling

Definition of Done for Any Change

Fully complies with this document
Follows AGENTS.md workflow
Passes all tests (unit + integration)
No memory regression on large datasets
Code reviewed against architecture constraints
Documentation updated if needed

This document overrides all previous specs, comments, and discussions.
Any deviation must be proposed via a formal architecture change request and approved before implementation.
Related Documents:

- Parses and decrypts backup metadata and media indices
- Streams data as NDJSON
- Handles all cryptographic operations
- Progress reporting via structured JSON
- Command-line interface for headless/script use

**Input**

```bash
stygion-cli --path /path/to/backup --json

Output

NDJSON stream to stdout
Progress events: {"type":"progress","value":0.45,"msg":"Decrypting index..."}
Final result objects for files, folders, metadata, thumbnails, etc.

Constraints

Must never load full backup into memory
Must support cancellation
All file I/O must be streaming/buffered
Error handling must be robust and reported via JSON


2. Go Wails Orchestrator (stygion-app)
Responsibility: Process management, bridging, and asset serving.

Spawns and manages the Rust CLI as a subprocess
Reads NDJSON stream using bufio.Scanner
Converts Rust events into Wails runtime events
Serves decrypted media/assets via custom AssetHandler (streaming)
Handles window management, tray, settings, etc.
Coordinates background tasks and cancellation

Key Rules

No blocking operations on the main thread
All Rust communication must be non-blocking and cancellable
Large files served via streami
```
