# STYGiON Refactor – Agent Orchestration Framework

This document defines the standardized two-agent workflow used for the complete refactor of the STYGiON project (Rust + Go/Wails + Svelte 5).

## Project Overview

STYGiON is a high-performance, privacy-first backup & media management application capable of handling 90GB+ encrypted backups with a responsive desktop UI.

**Core Architecture**

- **Rust CLI Core** – Heavy I/O, decryption, parsing, NDJSON streaming
- **Go (Wails) Orchestrator** – Process management, streaming, event emission, asset serving
- **Svelte 5 Frontend** – Modern runes-based UI with virtualized lists (14k+ items)

Authoritative specification lives in `REFACTOR.md`.

---

## Agent Roles

### Big Pickle  – Planner / Architect

- Responsible for **system design, planning, validation, and high-level decisions**.
- Must deeply analyze `REFACTOR.md` before any output.
- Produces detailed plans, architecture refinements, task breakdowns, validation checklists, and example prompts.
- **Never writes production code**.

### Big Pickle – Builder / Implementer

- Responsible for **writing, testing, and refining production code**.
- Follows plans and constraints provided by Big Pickle exactly.
- Implements features, fixes, performance improvements, and tests.
- **Never redesigns architecture** or deviates from the approved plan.

---

## Core Workflow (OpenCode Build Mode)

1. **Analyze** – Big Pickle reads `REFACTOR.md` + current codebase
2. **Plan** – Big Pickle creates detailed implementation plan + validation checklist
3. **Validate** – Big Pickle and human review plan for alignment and completeness
4. **Build** – Big Pickle executes the plan (one task at a time)
5. **Verify** – Tests pass + manual review against Definition of Done
6. **Handoff** – Results returned to Big Pickle for next iteration if needed

---

## Strict Boundaries & Guardrails

- Big Pickle **must not** produce implementation code
- Big Pickle **must not** alter architecture, introduce new major components, or ignore `REFACTOR.md`
- All agents must treat `REFACTOR.md` as the single source of truth
- No external network calls after initial setup
- No blocking operations in Go main thread
- No legacy Svelte syntax – only Svelte 5 Runes (`$state`, `$derived`, etc.)
- Large files must be streamed, never fully loaded into memory

---

## Key Technical Contracts

**Rust CLI**

- Accepts `--path <backup_dir> --json`
- Outputs NDJSON to stdout
- Emits progress via `{"type":"progress", "value":0.45, "msg":"..."}`

**Go (Wails) Orchestrator**

- Uses `os/exec` + `bufio.Scanner` for Rust communication
- Emits events via Wails runtime
- Serves assets via streaming `AssetHandler`

**Svelte 5 Frontend**

- Must use runes reactivity
- Virtualized rendering for large datasets
- High-performance search and gallery views

---

## Task Lifecycle Stages

Every major task must follow:

1. **Analyze** – Review relevant files and `REFACTOR.md`
2. **Plan** – Detailed breakdown (Big Pickle)
3. **Validate** – Checklist review
4. **Implement** – Atomic, testable changes (Big Pickle)
5. **Test** – Unit + integration + performance
6. **Verify** – Meets Definition of Done

---

## Definition of Done (DoD)

- Code follows all constraints in `REFACTOR.md`
- All new code is properly typed and documented
- Unit + integration tests added/updated
- UI changes maintain 60 fps on large datasets
- No memory spikes on large backups
- Passes validation checklist
- Human review confirms alignment

---

## Example Task Prompts

### Rust CLI Example

"Enhance the Rust parser to support new metadata fields while maintaining NDJSON streaming output and progress reporting."

### Go Orchestrator Example

"Improve streaming pipeline between Rust process and frontend with proper error handling and cancellation support."

### Svelte 5 Example

"Implement virtualized gallery view using Svelte 5 runes that can smoothly handle 14,000+ items with search filtering."

---

## Usage Instructions

1. Always start a new task by having Big Pickle read the latest `REFACTOR.md`
2. Use this `AGENTS.md` as the governing document for all agent interactions
3. Keep handoffs clean: Big Pickle output becomes the exact input for Big Pickle
4. Maintain a running `TASK.md` for the current sprint/epic

**This framework ensures architectural integrity while maximizing implementation velocity.**
