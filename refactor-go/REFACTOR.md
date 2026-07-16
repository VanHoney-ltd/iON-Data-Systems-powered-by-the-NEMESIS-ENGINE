# iON Refactor — Superseded Specification

**Status**: Superseded  
**Superseded by**: [iON NEMESIS Kernel-First Replatform Design](../docs/superpowers/specs/2026-05-14-ion-nemesis-kernel-first-design.md)  
**Approved implementation plan**: [iON NEMESIS Kernel-First Implementation Plan](../docs/superpowers/plans/2026-05-14-ion-nemesis-kernel-first.md)  
**Last reconciled**: 2026-07-01

## Notice

The previous version of this document described a "STYGiON" refactor based on a Rust CLI core, a Go/Wails orchestrator, and a Svelte 5 frontend. That direction contradicted the approved NEMESIS kernel-first plan and has been retired. This file is retained only as a reconciled reference.

## Approved Direction

The authoritative replatform creates a clean repository at `/home/ghost/iON-nemesis` as an Elixir/OTP NEMESIS kernel. Milestone 1 delivers a CLI-driven execution kernel with:

- SQLite-backed state, events, artifacts, and validation results.
- Oban durable jobs.
- `Chronos` as the first synthetic-intake agent.
- Strict `chronos.intake.v1` output validation.
- iONlog-style audit events.
- Reproducibility metadata and archive/PII guardrails.

The existing `/home/ghost/iON` tree remains reference-only for milestone 1. No new work in this tree should follow the superseded STYGiON specification.

## Out of Scope for Milestone 1

Per the approved plan, the following are explicitly deferred:

- Mutating or deleting files in `/home/ghost/iON`.
- Importing real decrypted backup data.
- Copying the old repository wholesale.
- Implementing Phoenix API/realtime control plane.
- Implementing SvelteKit/Tauri UI.
- Implementing every production agent.
- Shipping DuckDB analytics views, except documenting the planned boundary.

## Reference Documents

- Approved design spec: `docs/superpowers/specs/2026-05-14-ion-nemesis-kernel-first-design.md`
- Approved implementation plan: `docs/superpowers/plans/2026-05-14-ion-nemesis-kernel-first.md`
- Agent orchestration guidance: `refactor-go/AGENTS.md`
