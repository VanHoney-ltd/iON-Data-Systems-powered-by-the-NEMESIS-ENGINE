# iON Data Security Systems - Nemesis Engine

Local-first forensic evidence and AI orchestration system.

## Layout

- `core/` - Rust Nemesis engine, agents, `run-all`, `desktop-serve`
- `nemesis-desktop/` - new Wails + React desktop shell
- `HELiOS/` - standalone backup extraction helper crate
- `docs/` - supporting documentation
- `*.py` - current audio/transcription helper scripts

Case data is intentionally not committed. Runtime data belongs under:

```text
backups/cases/<case-name>/
```

## CLI Commands

Build or install the Rust CLI as `ion` or `minios`. Both names point at the same
core command surface.

Probe for an attached iOS device:

```bash
ion device status
ion device status --json
```

Start a fresh encrypted iOS backup:

```bash
BACKUP_PASSWORD='owner-provided-password' ion backup start <case-name>
```

Run every extraction and analysis agent, then stage UI-ready case files:

```bash
ion nemesis run <case-name>
```

Check local Ollama models:

```bash
ion llm status
```

Run Psyche with bounded local Ollama synthesis:

```bash
ion psyche run <case-name> --llm --model <ollama-model-name> --limit 5
```

Launch the selectable Psyche TUI:

```bash
ion psyche tui <case-name>
```

List and run individual agents:

```bash
ion agents list
ion agents run plutus <case-name>
```

Backward-compatible commands still work:

Run every agent and stage UI files:

```bash
cargo run --manifest-path core/Cargo.toml --bin minios -- run-all <case-name>
```

Start the local desktop API server:

```bash
cargo run --manifest-path core/Cargo.toml --bin minios -- desktop-serve 127.0.0.1:17870
```

Run evidence review:

```bash
cargo run --manifest-path core/Cargo.toml --bin minios -- evidence-review <case-name>
```

## Desktop

Build the Wails shell:

```bash
cd nemesis-desktop
wails build
```

The Wails app is a thin desktop host. Rust remains the single orchestration brain.

## Arch/AUR Packaging

The package template lives at:

```text
packaging/aur/PKGBUILD
```

Build locally from the repository root:

```bash
cd packaging/aur
makepkg -f
```
