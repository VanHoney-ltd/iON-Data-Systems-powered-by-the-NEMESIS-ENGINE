# iON2 Quick Prime — Paste this at session start

Read `/home/ghost/iON/core/.kimi/memory.md` for full project context.

Current working directory should be `/home/ghost/iON/core`.

Key things to know right now:
- 20-agent Rust data acquisition suite — **iON Data Systems** (iON2 v2.1.0)
- Case-based: each data collection is a directory with standardized layout
- Agents: chronos (backup), helios (decrypt), cerberus (SMS), hermes (calls), nyx (browser), charon (photos), obolus (notes), psyche (AI analysis), aether (GPS), vigil (video), echo (audio), plutus (financial), atlas (app GPS), talos (Android), styg (secure containers), xwin (live pull), nemesis (orchestrator), vox (transcription), orpheus (SQLite recon)
- Shared: case.rs (Case struct, registry, workspace), common/ (path resolution), ui_core.rs (UI state), contact_index.rs (name↔phone)
- charon/obolus/psyche have stub core.rs files — full impl is in their main.rs
- GTK4 GUI at src/ui/gtk_shell.rs (3260 lines)

**BRANDING RULE:** Never describe this as "forensics," "forensic," or "court-admissible."
It is a data acquisition, extraction, and management platform.

If you need to verify current state, check `git status` or `git diff`.
