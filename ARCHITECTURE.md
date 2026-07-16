# iON Project Architecture

## Status (as of 2026-07-01)

This repository (`/home/ghost/iON`) is the **legacy iON codebase**. It remains useful as reference material and for existing Rust forensic tooling, but new kernel work is happening in a separate NEMESIS repository.

## Components

### Active / maintained

- **`core/`** — Rust crate `minios` v2.1.0. The forensic engine: iOS backup acquisition/decryption, agent framework, evidence pipeline, and a small desktop HTTP/SSE server (`desktop-serve`).
- **`ion/`** — Wails v2 + Svelte 3 desktop shell. It now has correct Wails bindings and event plumbing, and a hardened Go backend that resolves the Rust core binary at runtime.
- **`nemesis-desktop/`** — Wails v2 + React-TS desktop shell. It launches the Rust `desktop-serve` binary and talks to it over HTTP/SSE. The TypeScript build has been fixed.
- **`HELiOS/`** — Thin Rust CLI wrapper around `core` for selective encrypted-backup decryption.

### Deprecated / superseded

- **`refactor-go/REFACTOR.md`** — The previous "STYGiON" Rust/Go/Svelte5 refactor spec. It has been reconciled and marked as superseded by the NEMESIS kernel-first plan.
- The original `ion/frontend/src/App.svelte.fixed` and `.backup` files have been removed.

## Approved forward direction

See:
- `docs/superpowers/specs/2026-05-14-ion-nemesis-kernel-first-design.md`
- `docs/superpowers/plans/2026-05-14-ion-nemesis-kernel-first.md`

Milestone 1 creates a fresh `/home/ghost/iON-nemesis` Elixir/OTP kernel repo. It does **not** mutate files in `/home/ghost/iON`.
