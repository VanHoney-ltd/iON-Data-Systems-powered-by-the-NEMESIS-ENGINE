# iON NEMESIS Kernel-First Replatform Design

Date: 2026-05-14

## Purpose

Replatform iON Data Management Systems around the NEMESIS ENGINE without mutating the existing `/home/ghost/iON` tree during the first implementation milestone. The first milestone proves a production-shaped execution kernel with one real end-to-end agent run.

The existing `/home/ghost/iON` tree remains reference-only until deletion manifests, retained-data manifests, and sensitive-data rules are reviewed. A new clean repository will be created at `/home/ghost/iON-nemesis`.

## Approved Scope

Milestone 1 is a kernel-first vertical slice:

- Create a new clean project at `/home/ghost/iON-nemesis`.
- Initialize a fresh local git repository there.
- Build an Elixir/OTP NEMESIS application.
- Use CLI-driven execution first; defer Phoenix HTTP/API and UI.
- Use Oban with SQLite from day one for durable jobs and persisted state.
- Implement Chronos as the first real executable agent.
- Use synthetic input only.
- Validate Chronos output with strict `chronos.intake.v1` schema.
- Provide reproducible commands:
  - `make bootstrap`
  - `make test`
  - `make run`
  - `make run-agents`
  - `make verify`
  - `make archive`
- Create the first commit with:
  - `Initial iON Data Management Systems NEMESIS replatform`
- Create a new GitHub repository and push the first commit if GitHub authentication is available.

Out of scope for milestone 1:

- Mutating or deleting files in `/home/ghost/iON`.
- Importing real decrypted backup data.
- Copying the old repository wholesale.
- Implementing Phoenix API/realtime control plane.
- Implementing SvelteKit/Tauri UI.
- Implementing every production agent.
- Shipping DuckDB analytics views, except documenting the planned boundary.

## Repository Strategy

The implementation creates `/home/ghost/iON-nemesis` instead of cleaning `/home/ghost/iON` in place.

This avoids accidental carryover of:

- symlinks
- backup/archive junk
- Rust build outputs
- Go/Wails UI files
- virtualenv content
- private/decrypted data paths
- generated or duplicate artifacts

The old tree may be inspected as reference, but no legacy file is copied unless it is intentionally selected, reviewed, and safe for the new repository.

## Kernel Architecture

NEMESIS is implemented as an OTP application, not scripts. Milestone 1 starts the minimum production-shaped supervision tree needed for a real Chronos run:

- `Nemesis.AgentRegistry`: registers available agents and their input/output contracts.
- `Nemesis.JobPlanner`: turns a CLI request into an executable job plan.
- `Nemesis.TaskRouter`: dispatches agent tasks.
- `Nemesis.StateStore`: persists case/project state in SQLite.
- `Nemesis.JobQueue`: wraps Oban job enqueueing and execution.
- `Nemesis.EventLog`: records iONlog-style audit events.
- `Nemesis.ArtifactIndex`: records generated artifacts and validation state.
- `Nemesis.OutputValidator`: validates versioned agent outputs.
- `Nemesis.ReproManifest`: writes reproducibility metadata for each run.
- `Nemesis.FailureRecovery`: records failed jobs with enough context to inspect or retry.

Phoenix PubSub/API and DuckDB analytics are documented as later boundaries. The first kernel should not depend on them to boot or run Chronos.

## Chronos Vertical Slice

Chronos is the first complete agent. For milestone 1 it runs against a synthetic source directory.

Chronos responsibilities:

- accept a documented input schema
- create or update a case record
- stage selected synthetic files into an artifact directory
- compute basic file metadata and checksums
- emit start, staging, validation, and completion events
- write a `chronos.intake.v1` JSON output artifact
- persist output and artifact records
- generate reproducibility metadata

The Chronos job succeeds only when:

- input validates
- staged artifacts exist in the expected output location
- `chronos.intake.v1` output validates against the documented schema
- SQLite state is persisted
- iONlog events were recorded
- artifact index records every produced file
- reproducibility manifest records command, timestamps, versions, checksums, and selected model/provider policy
- tests prove the sample run works with synthetic data

## Data Safety

No real decrypted backup data is used in milestone 1.

The new repository includes guardrails so private material does not enter commits, build outputs, archives, logs, docs, examples, screenshots, or test fixtures.

Required safety files:

- `.gitignore`
- archive exclusion manifest
- `docs/DATA_POLICY.md`
- `docs/PII_REDACTION.md`
- `docs/DEVELOPMENT_DATA_RULES.md`
- `scripts/pii_scan.sh`
- `scripts/sanitize_dev_data.sh`
- `scripts/pre_archive_validation.sh`

The intended policy is:

- real data stays quarantined outside public build paths
- public tests use synthetic datasets only
- generated docs and examples never include operator data
- logs redact sensitive values
- archives fail if obvious private data or quarantined paths are detected

## Reproducibility

Milestone 1 includes:

- `Makefile`
- optional `justfile`
- `scripts/bootstrap.sh`
- `scripts/run_all_agents.sh`
- `scripts/verify_outputs.sh`
- `scripts/reproducibility_manifest.sh`
- `scripts/archive_ion.sh`

Required docs:

- `docs/ARCHITECTURE.md`
- `docs/NEMESIS_ENGINE.md`
- `docs/AGENT_CONTRACTS.md`
- `docs/BUILD_MODELS.md`
- `docs/REPRODUCIBILITY.md`
- `docs/MIGRATION_FROM_RUST.md`
- `docs/DATA_POLICY.md`
- `docs/PII_REDACTION.md`
- `docs/DEVELOPMENT_DATA_RULES.md`

`docs/BUILD_MODELS.md` records the selected model/provider policy. The implementation must run `opencode models` before choosing any model/provider for build assistance or AI-related execution. Claude models are prohibited.

## GitHub Handoff

After implementation plan approval, the implementation phase will:

1. Create `/home/ghost/iON-nemesis`.
2. Initialize a fresh git repository.
3. Produce the milestone 1 files.
4. Run verification.
5. Commit with `Initial iON Data Management Systems NEMESIS replatform`.
6. Create a new GitHub repository.
7. Push the first commit.

If `gh` authentication or git remote credentials are unavailable, the local commit remains complete and the implementation reports the exact command needed to create/push the GitHub repository.

## Acceptance Criteria

Milestone 1 is complete when:

- `/home/ghost/iON-nemesis` exists as a fresh repo.
- NEMESIS OTP application boots.
- Oban and SQLite are configured and used for the Chronos job.
- Chronos runs from CLI against synthetic input.
- Chronos writes staged artifacts and a valid `chronos.intake.v1` output.
- SQLite state, iONlog events, and artifact index records are persisted.
- Reproducibility manifest is generated for the sample run.
- PII/archive validation scripts exist and run during verification.
- `make bootstrap`, `make test`, `make run`, `make run-agents`, `make verify`, and `make archive` exist.
- Documentation files listed above exist.
- The first local commit is created.
- A GitHub repo is created and pushed, or the blocker is reported with exact next commands.
