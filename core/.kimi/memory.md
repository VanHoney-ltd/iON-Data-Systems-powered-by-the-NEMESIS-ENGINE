# iON2 Project Memory — Kimi Code Context

> **Purpose:** This file exists so Kimi Code can instantly understand the iON2 codebase
> without re-reading all source files. Update it whenever architecture changes.
>
> **Usage:** At session start, say: "Read .kimi/memory.md" or point Kimi to this file.
>
> **BRANDING REMINDER:** This project is **iON Data Systems** (or iON Data Management
> Systems). It is NOT to be described as a "forensics suite," "forensic tool," or
> "court-admissible" product. It is a data acquisition, extraction, and management
> platform built to a high standard of data integrity. Do NOT use forensics
> terminology when describing this software.

---

## Project Identity

- **Name:** iON2
- **Version:** 2.1.0
- **Language:** Rust (edition 2021)
- **Type:** iOS & Android data acquisition and management suite
- **Crate:** `iON2` (lib + 20+ binaries)

## Architecture Overview

Multi-agent data acquisition platform. Each agent is a specialized CLI binary. Cases are directory-based data collections with standardized layout.

```
case/
├── backup/              # Raw iOS backup (or Android equivalent)
├── source/backup/       # Alternative backup location
├── evidence/            # Extracted artifacts per agent
├── logs/                # Agent execution logs
├── reports/             # Generated reports
├── prepared/            # Decrypted/processed data
│   ├── db/              # SQLite databases (WAL-replayed)
│   └── helios/          # Domain-extracted backup contents
│       ├── full/
│       ├── sms_only/
│       └── metadata/
├── clean/               # Sanitized exports
└── index/               # Search indexes
```

Case root resolution priority:
1. `~/.iON/case_registry.json` (persisted root + phone numbers + contacts)
2. `iON_CASE_ROOTS` / `iON_HOME` / `iON_CASE_ROOT` env vars
3. `~/iON/cases/<name>`
4. CWD and parents → `./cases/<name>`
5. `~/Documents/ghostdevops/...`

## The 20 Agents (Data Acquisition Modules)

| Binary | Module | Purpose | Entry Point | Impl Status |
|--------|--------|---------|-------------|-------------|
| `ion` | `ion/` | Main launcher TUI | `src/ion/main.rs` | ✅ Full |
| `chronos` | `chronos/` | iOS backup acquisition | `src/main.rs` | ✅ Full |
| `helios` | `helios/` | Encrypted backup decryption | `src/helios/main.rs` | ✅ Full |
| `orpheus` | `orpheus/` | SQLite reconnaissance | `src/orpheus/main.rs` | ✅ Full |
| `cerberus` | `cerberus/` | SMS/MMS extraction | `src/cerberus/main.rs` | ✅ Full |
| `hermes` | `hermes/` | Call history + voicemail | `src/hermes/main.rs` | ✅ Full |
| `nyx` | `nyx/` | Safari/browser data | `src/nyx/main.rs` | ✅ Full |
| `charon` | `charon/` | Photos/media extraction | `src/charon/main.rs` | ✅ Full* |
| `obolus` | `obolus/` | Notes extraction | `src/obolus/main.rs` | ✅ Full* |
| `psyche` | `psyche/` | Behavioral AI analysis | `src/psyche/main.rs` | ✅ Full* |
| `aether` | `aether/` | GPS/location intelligence | `src/aether/main.rs` | ✅ Full |
| `vigil` | `vigil/` | Video evidence inventory | `src/vigil/main.rs` | ✅ Full |
| `echo` | `echo/` | Audio evidence inventory | `src/echo/main.rs` | ✅ Full |
| `plutus` | `plutus/` | Financial artifacts | `src/plutus/main.rs` | ✅ Full |
| `atlas` | `atlas/` | GPS from app databases | `src/atlas/main.rs` | ✅ Full |
| `talos` | `talos/` | Android acquisition | `src/talos/main.rs` | ✅ Full |
| `styg` | `styg/` | Secure container format | `src/styg/main.rs` | ✅ Full |
| `xwin` | `xwin/` | Live iOS file pull | `src/xwin/main.rs` | ✅ Full |
| `nemesis` | `nemesis/` | Master orchestrator | `src/nemesis/main.rs` | ✅ Full |
| `vox` | `vox.rs` | Media transcription (WhisperX) | `src/vox.rs` | ✅ Full |

\* *Full implementation is in `main.rs`; `core.rs` and submodules are stubs returning empty data.*

## Key Shared Modules

| Module | File(s) | Purpose |
|--------|---------|---------|
| `case` | `src/case.rs` | Case struct, workspace layout, registry I/O, phone/contact resolution |
| `common` | `src/common/` | BackupResolver, TargetPath, PreparedPath utilities |
| `contacts` | `src/contacts/` | AddressBook extraction, contact resolution |
| `contact_index` | `src/contact_index.rs` | Bidirectional name↔phone index for cross-agent lookups |
| `ui_core` | `src/ui_core.rs` | UiState, UiPaths, agent execution, log parsing, timeline events |
| `ui_export` | `src/ui_export.rs` | Aggregates all agent outputs → unified JSON/CSV for web UI |
| `ui` | `src/ui/` | GTK4/Relm4 view models |
| `ui_shell` | `src/ui_shell/` | GTK4 GUI entry point (`ion_ui` binary) |

## GTK Shell Architecture (`src/ui/gtk_shell.rs`)

**3,260 lines.** Single-file Relm4 component. Key structures:

- `ShellModel` — main component state (case, snapshot, selected route, zoom, search, selections)
- `ShellInput` — message enum (SelectRoute, Refresh, ToggleMessage, etc.)
- `PageCard` — content card struct (title, subtitle, body)
- `WorkspaceRoute` — 16 sidebar routes defined in `src/ui/mod.rs`

**Sidebar sections:** System Control, Device Access, Data Core, Communication, Files & Media, Financial, Intelligence, Output, Background Systems, Advanced/Low-Level, Bridge/Compatibility

**Topbar:** Search entry, status display, action buttons (Refresh, Pair, Mount, Unmount, Run Psyche, Write Evidence Doc, Prev/Next App)

**Content area:** 3-card layout (`card_visible(0)`, `card_visible(1)`, `card_visible(2)`) with dynamic content per route

### Route Implementation Status

| Route | Cards Function | Status | Notes |
|-------|---------------|--------|-------|
| Nemesis | `overview_cards()` | ✅ | Case summary, agent readiness, recent logs |
| Chronos | `chronos_cards()` | ✅ | Backup roots, live device, toolchain |
| Helios | `helios_cards()` | ✅ | Decrypted extracts, manifest status |
| Orpheus | `orpheus_cards()` | ⚠️ | App list + prev/next, but no DB browser |
| Contacts | `contacts_cards()` + `rebuild_contacts_list()` | ⚠️ | Flat checkbox list; single phone/email only |
| Cerberus | `cerberus_cards()` + `rebuild_cerberus_list()` | ❌ | 120-char truncation, no threading, no detail pane |
| Hermes | `hermes_cards()` | ✅ | Call records, voicemail |
| Charon | `charon_cards()` | ✅ | Assets, faces, albums, timeline |
| Vox | `generic_agent_cards()` | ❌ | Placeholder only |
| Plutus | `generic_agent_cards()` | ❌ | Placeholder only |
| Psyche | `generic_agent_cards()` | ❌ | Placeholder only |
| Obolus | `obolus_cards()` | ⚠️ | Under "OUTPUT" section — wrong category |
| Nyx | `nyx_cards()` | ✅ | History, bookmarks, tabs, autofill, searches |
| Aether | `generic_agent_cards()` | ❌ | Placeholder only |
| Tartarus | `generic_agent_cards()` | ❌ | Placeholder only |
| Xwin | `generic_agent_cards()` | ❌ | Placeholder only |

### Known UI Issues

1. **Cerberus message truncation:** `rebuild_cerberus_list()` hard-cuts at 120 chars
2. **No message threading:** Flat list instead of conversation grouping
3. **No message detail pane:** Can't read full text or see metadata
4. **Contacts single phone:** `ContactRecord` has `phone: Option<String>` not `Vec<String>`
5. **Contacts missing fields:** No birthday, URLs, job title, nickname, blocked status
6. **Obolus mis-categorized:** Listed under "OUTPUT" instead of data section
7. **Generic routes:** Vox, Plutus, Psyche, Aether, Tartarus, Xwin show "pending shell wiring"

## Critical Patterns

1. **Stub vs Full Split:** `charon`, `obolus`, `psyche` have stub `core.rs` files. Real logic is in `main.rs`. Refactoring these to use the lib interface is future work.
2. **Deterministic Evidence:** Agents never guess paths. They use `BackupResolver` or prepared artifacts.
3. **Dual Output:** Every agent produces JSON (machine) + CSV (human).
4. **F-Stage Records:** Agents write status to `output/F/{agent}/stage.json`.
5. **Audit Trail:** `output/agent_audit.jsonl` with redacted phone numbers.
6. **Copy Manifest:** Charon produces `copy_manifest.json` consumed by Aether, Vigil, Echo.

## Key Dependencies

- `rusqlite` — SQLite analysis (bundled, backup feature)
- `aes`, `aes-gcm`, `aes-kw`, `pbkdf2` — iOS backup encryption
- `plist` — Apple property lists
- `exif` (kamadak-exif) — Photo metadata
- `tokio` — Async orchestration (nemesis)
- `ratatui`, `crossterm` — Terminal UI
- `gtk4` 0.11.2, `relm4` 0.11.0 — Native GUI (optional feature `gtk_shell`, alias `full`)
- `ed25519-dalek`, `sha2` — Cryptographic integrity (styg)
- `reqwest` — Network (nemesis MCP)
- `csv`, `serde_json` — Data export
- `rayon` — Parallel processing
- `clap` — CLI parsing
- `walkdir` — Directory traversal

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `BACKUP_PASSWORD` | Required for chronos encrypted backup |
| `iON_CASE_ROOTS` | Comma-separated additional case root paths |
| `iON_HOME` | Base directory for iON data |
| `iON_CASE_ROOT` | Single case root override |

## Build

```bash
cd /home/ghost/iON/core
cargo build --release --features full  # All binaries including GUI
cargo build --release --bin ion
cargo build --release --bin chronos
cargo build --release --features full --bin ion_ui  # GUI only
```

## Active Work (2026-04-21)

- **UI Redesign in progress:** Cerberus message view (threading, expandable rows, detail pane)
- **Contact expansion in progress:** Multi-phone, multi-email, birthday, URLs, blocked status
- **Refactor deferred:** charon/obolus/psyche lib modules — ship UI first

## Fixed (2026-04-22)

- **gtk4/relm4 upgraded** to 0.11.2 / 0.11.0
- **afcclient** auto-builds via `build.rs` (gracefully warns if system libs missing)
- **GUI builds by default** with `--features full`
- **Placeholder backends replaced:** Hermes ffmpeg audio conversion, Nyx plist parsing, Psyche heuristic sentiment analysis
- **Runtime dependency checker** (`common::deps`) gives install instructions for missing tools
- **Hardcoded paths** fixed: `ion_backup_path()` respects `iON_BACKUP_DIR`
- **Obolus** moved from OUTPUT to DATA CORE sidebar section
- **Aether** wired into evidence panel system

## Last Updated

2026-04-22 — Backend hardening, dependency upgrade, build system fixes, placeholder elimination.
