# iON Agent Orchestration — Aligned with NEMESIS Kernel-First Plan

**Status**: Aligned with the approved NEMESIS kernel-first plan.  
**Authoritative plan**: [iON NEMESIS Kernel-First Implementation Plan](../docs/superpowers/plans/2026-05-14-ion-nemesis-kernel-first.md)  
**Authoritative design spec**: [iON NEMESIS Kernel-First Replatform Design](../docs/superpowers/specs/2026-05-14-ion-nemesis-kernel-first-design.md)  
**Last reconciled**: 2026-07-01

## Notice

The previous STYGiON two-agent framework described in this file has been retired. All agent work must follow the approved NEMESIS kernel-first plan.

## Required Skills

The approved plan requires agentic workers to use one of the following skills:

- `superpowers:subagent-driven-development` (recommended)
- `superpowers:executing-plans`

## Core Workflow

1. **Read the approved plan** — Start every task from `docs/superpowers/plans/2026-05-14-ion-nemesis-kernel-first.md`.
2. **Analyze** — Review relevant files in `/home/ghost/iON-nemesis` and the approved plan.
3. **Plan** — Produce a detailed, atomic implementation plan aligned with milestone 1 scope.
4. **Validate** — Check the plan against the approved constraints before building.
5. **Implement** — Execute one task at a time.
6. **Verify** — Run `make test`, `make verify`, and any relevant checks.
7. **Handoff** — Return results for review before starting the next task.

## Strict Boundaries

- The approved NEMESIS plan is the single source of truth.
- The new implementation lives in `/home/ghost/iON-nemesis`, not in `/home/ghost/iON`.
- `/home/ghost/iON` is reference-only for milestone 1; do not mutate, delete, or copy legacy files into the new repo unless explicitly selected, reviewed, and safe.
- Milestone 1 is a kernel-first vertical slice. Phoenix, UI, DuckDB analytics, and real decrypted data are deferred.
- No real decrypted backup data may be used in milestone 1.
- Tests and validation must use synthetic input only.

## Key Technical Contracts

- OTP application boots a supervision tree with `Nemesis.AgentRegistry`, `Nemesis.JobPlanner`, `Nemesis.TaskRouter`, `Nemesis.StateStore`, `Nemesis.JobQueue`, `Nemesis.EventLog`, `Nemesis.ArtifactIndex`, `Nemesis.OutputValidator`, `Nemesis.ReproManifest`, and `Nemesis.FailureRecovery`.
- Oban uses SQLite for durable jobs and persisted state.
- Chronos is the first executable agent; it runs against synthetic input and writes a validated `chronos.intake.v1` artifact.
- Required reproducibility commands: `make bootstrap`, `make test`, `make run`, `make run-agents`, `make verify`, `make archive`.
- Required data-safety deliverables: `.gitignore`, archive exclusion manifest, `docs/DATA_POLICY.md`, `docs/PII_REDACTION.md`, `docs/DEVELOPMENT_DATA_RULES.md`, `scripts/pii_scan.sh`, `scripts/sanitize_dev_data.sh`, and `scripts/pre_archive_validation.sh`.

## Definition of Done

- Implementation matches the approved plan and this document.
- All new code is properly typed and documented.
- Unit and integration tests pass.
- `make test`, `make verify`, and `make archive` succeed where applicable.
- No real operator data enters commits, build outputs, archives, logs, docs, examples, screenshots, or test fixtures.
