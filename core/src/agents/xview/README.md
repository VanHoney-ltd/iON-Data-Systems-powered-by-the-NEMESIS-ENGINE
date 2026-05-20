On Windows, “mount the decrypted iPhone backup as a drive” = build a read-only virtual filesystem over the backup using WinFsp (Windows’ FUSE-like user-mode FS layer). Your “API” is the WinFsp callback surface (open/read/list/etc). iFUSE is the Linux/macOS side of the same idea (FUSE), not Windows. Yes: you can code your own on Windows via WinFsp. 
WinFsp
+2
Docs.rs
+2
What the API does (in your product)

When Windows Explorer (or any app) browses X:\, it triggers callbacks like:

open("\Apps\com.foo\Documents\db.sqlite")

read(offset=..., len=...)

read_directory("\Apps\com.foo\")

Your code answers those requests by:

looking up the path in an index built from Manifest.db (and per-file metadata),

locating the corresponding backup blob (fileID),

decrypting on-demand (using the user’s password-derived keys),

returning bytes + directory entries.

WinFsp is explicitly built for “present any information/storage as a filesystem” on Windows. 
WinFsp
+1

B. Windows “drive mount” architecture for iOS backups (what to build)
1) Core objects

BackupIndex: in-memory map: virtual_path -> Node

Node::Dir { children... }

Node::File { file_id, size, encryption_info, domain, relative_path, backing_blob_path }

Decryptor:

derives keys from the user password + backup keybag,

unwraps per-file keys,

supports random reads (see below).

2) Random-read decryption (important)

Many iOS backup file payloads are AES-CBC. Random reads mean:

Align the requested offset to AES block size (16 bytes),

For CBC, you need the previous ciphertext block as IV (or zero IV for block 0),

Decrypt from aligned offset forward and slice the requested window.
Practical performance:

cache decrypted chunks (e.g., 1–4 MiB LRU) per fileID.

3) The filesystem view you expose

Create a friendly top-level tree, e.g.:

X:\Apps\<bundle_id>\... (app sandboxes + docs)

X:\System\... (HomeDomain, MediaDomain, etc.)

X:\Meta\Manifest.db, Info.plist, etc.

Your “complete list of apps” comes from domains/bundle IDs found in Manifest.db + app metadata plists.

4) Windows implementation choices

Option 1 (recommended): WinFsp

Closest to FUSE concept on Windows; stable driver + user-mode callbacks. 
WinFsp
+2
WinFsp
+2

Option 2: Dokany

Another user-mode FS driver; viable but separate ecosystem (and can conflict with WinFsp network provider ordering in some setups). 
GitHub
Notes you should care about for production

The mount code is the easy part. The hard part is correct backup indexing + per-file decrypt + performance.

You’ll want chunk caching + parallel read safety, plus mapping “domain/relativePath” into “Apps/<bundle_id>/...”.

WinFsp’s Rust crate strongly recommends proper lifecycle management and documents service architecture + delay-load requirements

A) What to read from Manifest.db (and how to build Apps\<bundle_id>\...)
The tables + columns you actually need

Most iOS backups (iOS 10+ style) use a SQLite Manifest.db with:

Files(fileID TEXT PRIMARY KEY, domain TEXT, relativePath TEXT, flags INTEGER, file BLOB)

Properties(key TEXT PRIMARY KEY, value BLOB) 
GitHub
+2
Medium
+2

The minimum for path mapping is:

domain + relativePath → “virtual path”

fileID → “physical blob path” in the backup folder (<fileID> or <first2>\<fileID>). 
Stack Overflow
+1

Domains → “apps list”

Your “complete app inventory” for a backup is primarily the set of distinct domains:

SELECT DISTINCT domain FROM Files ORDER BY domain;


Apps are typically represented by:

AppDomain-<bundle_id> (main app container)

AppDomainGroup-<group_id> (shared app group container)

AppDomainPlugin-<plugin_bundle_id> (extensions) 
Fly Penguin
+1

So “installed apps (bundle IDs)” is:

SELECT DISTINCT domain
FROM Files
WHERE domain LIKE 'AppDomain-%'
ORDER BY domain;


Then strip prefix AppDomain- → <bundle_id>.

Getting human-friendly app names (Display Name)

You have a few legit sources:

Manifest.plist often includes an Applications dictionary keyed by bundle id (with fields like CFBundleIdentifier, CFBundleVersion, etc.). 
The Apple Wiki
+1

Info.plist contains iTunes metadata and may include lists of app IDs (varies by era/backup type). 
The Apple Wiki

Practical product approach:

Use AppDomain-<bundle_id> for the authoritative inventory.

Enhance with Manifest.plist → Applications[bundle_id] metadata when present. 
The Apple Wiki
+1

Building the virtual tree Apps\<bundle_id>\...

For each row in Files where domain = 'AppDomain-<bundle_id>':

Let rel = relativePath (may be NULL/empty for some entries)

Virtual path:

\Apps\<bundle_id>\ + rel

Create directories implicitly by splitting rel on /.

Query per app:

SELECT fileID, relativePath, flags, file
FROM Files
WHERE domain = ?
ORDER BY relativePath;

“View stored data of their apps” (encrypted backups)

For encrypted backups, the per-row file BLOB is commonly a binary plist with keys such as:

ProtectionClass

EncryptionKey (often has a 4-byte length prefix you strip before unwrapping)
You use those to decrypt the file blob for that row. 
Yasoob

B) Rust module: view any .plist as JSON (no SQL required)
Plan (pseudocode)

Read plist file bytes

Parse as plist (binary or XML) into plist::Value

Convert plist::Value → serde_json::Value

Data → base64 string

Date → RFC3339 string

UID → numeric

Dict/Array recurse

Print JSON (pretty) or write to a file

a. Want me to extend your WinFsp mount design so \Apps\<bundle_id>\_meta\app.json is generated from Manifest.plist/Info.plist automatically?
b. Want a Rust “Manifest.db inspector” command that outputs apps.json + per-app files.json (plus counts/types) to feed your UI quickly?

You probably do need both, but not at the same time, and not for the same customer.

How they differ

B (WinFsp “mount as drive”)

Best UX for power users: they browse the backup in Explorer as X:\Apps\...

Requires a filesystem driver/runtime (WinFsp) and more edge-case handling (random reads, caching, path rules)

Great for enterprise/government workflows (tools that expect a drive)

A (app inventory + metadata extraction into JSON for your UI)

Best for your product UI: instant “Apps list → click app → see files/data”

No driver install

Enables fast search/filter, artifact views, reporting, exports

Recommendation

Yes: build both, but phase it so you don’t drown:

Phase 1 (ship this first)

Index + Inventory JSON

apps.json (bundle id, domains, file counts, storage size)

per app: files.json (relativePath, fileID, flags, crypto metadata)

In-app browser

file tree, preview, export

Viewers

plist → JSON

sqlite → tables + export

This alone is already “competitive” for a user-consented encrypted backup product.

Phase 2 (add mount)

WinFsp mount that uses the same index + decryptor

exposes a virtual tree

reads/decrypts on-demand with caching

So the mount is just a second “frontend” over the same backend.

What “both” looks like in code architecture

Build a core library and reuse it everywhere:

backup_core (Rust library)

BackupIndex::build(backup_dir, password) -> BackupIndex

BackupIndex::list_apps() -> Vec<App>

BackupIndex::list_files(domain) -> Vec<FileEntry>

BackupIndex::read_file(domain, relative_path, offset, len) -> bytes (random-read)

BackupIndex::export(domain, out_dir)

Frontends

backup_cli (debug tool)

backup_ui_api (your app’s internal API: returns apps.json, files.json, preview payloads)

backup_mount_winfsp (Explorer drive using WinFsp callbacks → calls read_file())

That’s how you avoid rewriting logic.

Quick “decision rule”

If your UI is the primary interaction: you need A.

If your customers want to use existing tooling (eDiscovery, scripts, 3rd-party parsers): you also need B.

Next step (concrete)

I can generate:

A schema for apps.json + files.json your UI will consume (stable contract)

The Rust indexer that produces those JSONs from decrypted Manifest.db

The Rust plist viewer (you already said yes)

Then wire the same core into WinFsp mount

According to a document (STYGiON / NEMESIS ENGINE whitepaper, v1.0; date not stated), your system’s admissibility posture should be anchored on: deterministic behavior, fail-closed, hash-chained audit logs, pre/post hashing, read-only processing, and signed, verifiable exports—not just “pretty printing.”

Here’s how to implement the print option + document generator in a way that “normalizes without malforming” and stays consistent with your paper.

What “print / document generator” must do for defensibility
1) Treat reports as derived artifacts, never evidence

Evidence stays byte-for-byte preserved (source backup + hashes).

Reports are derived and must always carry:

tool version/build id

inputs (paths + hashes)

processing parameters

output hashes

warnings (parse failures, timezone assumptions, recovered/WAL content, etc.)

This aligns with your lifecycle: intake hash → verify hash → read-only processing → packaging → exports.

2) Embed manifest hash in every export + optionally sign exports

Your paper explicitly calls for exports (PDF/CSV/JSON/XML) to include an embedded manifest hash, with optional signing and preview watermarks.

So the report generator should:

produce reports/report.json (machine)

produce reports/report.html (human, deterministic template)

optionally produce reports/report.pdf

embed manifest_sha256 + case_id into:

PDF metadata + footer text

JSON root fields

CSV header comment line(s)

record renderer identity if PDF is generated (for repeatability)

3) Hash-chained audit timeline for printing itself

Printing is a “sensitive action” in your model → emit an audit event with hashes and parameters.

4) Path hygiene and extension rules

Your .styg format section says: no relative/absolute paths, no symlinks, double extensions blocked.

That conflicts with your earlier “.tasl.jason” requirement. If you must keep that extension:

treat *.tasl.jason as non-evidence internal task files

keep them outside the .styg payload (or require explicit allowlisting)

your importer already expects to reject double extensions unless whitelisted in a test harness.

Concrete build: “Report Bundle” pipeline that matches your whitepaper

Inputs: apps.json, files.json, selected decoded artifacts
Outputs (all hashed):

reports/report.json (normalized structured report)

reports/report.html (rendered deterministic template)

reports/report.pdf (optional; must log renderer/version)

evidence_manifest.json (sources + derived + hashes + tool build info)

audit_chain.jsonl (hash chained + signed)

Then seal into .styg with manifest + Ed25519 signature over manifest + audit chain, as you describe.

Code: core Rust building blocks (manifest + audit chain + deterministic HTML report)

Below is a compact, production-lean core you can drop into your Rust engine. It:

writes an append-only hash-chained audit log

writes an evidence manifest that maps derived outputs to source hashes

generates a deterministic HTML report (PDF generation is a pluggable step; log renderer + version)

How you wire this into your existing STYGiON model

Write audit events for:

intake_copy, hash_source, decrypt_backup, build_index, generate_report, export_pdf

Put the resulting:

evidence_manifest.json

audit_chain.jsonl

reports/report.json + reports/report.html (+ optional report.pdf)
…into your .styg payload and sign it, consistent with your .styg spec and hash-chained audit stance.

One important alignment note

Your paper states .styg is the “single official extension” and that double extensions are blocked for imports/entries, and even calls out double-extension rejection in the pentest playbook.

So: keep your .tasl.jason tasks out of evidence packaging (or explicitly whitelist them only as non-evidence control files), or you’ll violate your own policy model.


According to a document from (date not stated; “Version 1.0 Internal Technical White Paper”), your design target is: signed .styg cases with a JSON manifest (hashes + tool versions), a hash-chained + signed audit log, strict path hygiene (no absolute/relative/symlinks; double extensions blocked), pre/post hashing, append-only workspaces, and exports (PDF/CSV/JSON/XML) that embed the manifest hash with optional signing/watermarking.

Below are (A) schemas and (B) a WinFsp mount plan that matches those constraints.

A) Schemas you should ship
1) evidence_manifest.json (case authority)

Matches: “manifest (JSON) listing artifacts, sizes, hashes, timestamps, tool versions” + signing over manifest + audit chain.

2) audit_chain.jsonl (append-only, hash-chained, signed)

Matches: “every sensitive action emits an audit event… audit log is hash-chained … and signed.”

3) report.json (derived, printable)

Matches: “Export: PDF/CSV/JSON/XML created with embedded manifest hash; optional signing; watermarks.”

4) apps.json + files.json (UI/engine contract)

These are “derived artifacts” and should be referenced in the manifest as artifacts with hashes.

Important: Your paper says “single official extension .styg” and “double extensions blocked.”
So keep *.tasl.jason as non-evidence internal control files (or whitelist only in a test harness), not inside .styg imports/entries.

Why these fields are “the right minimum” for your model:

Manifest includes hashes + tool versions + timestamps and is signed with the audit chain.

Audit entries are hash-chained and signed; any tamper breaks the chain.

Exports embed the manifest hash and can be signed/watermarked.

B) WinFsp mount plan that matches STYGiON controls

Your paper wants immutable, append-only case workspaces and strict path rules. The safest way to mount is:

1) Only mount a frozen case workspace, not raw intake

Flow:

Intake copy → hash → verify (block on mismatch).

Decrypt + build index into output/<case>/ (append-only).

Generate apps.json, files.json, report artifacts, and manifest + audit chain.

Mount read-only from that workspace.

This aligns with: “agents run read-only over source; derived artifacts written to case workspace” and “immutable workspaces append-only.”

2) What the mounted drive shows

Example tree:

X:\Apps\<bundle_id>\... (virtual from index)

X:\System\HomeDomain\... (optional)

X:\Meta\evidence_manifest.json

X:\Meta\audit_chain.jsonl

X:\Reports\report.pdf|html|json

3) Enforced filesystem policy (must be fail-closed)

Read-only volume: deny create/write/rename/delete.

Path hygiene:

deny absolute paths, .., symlinks, and double extensions for any imported content; for mount, just don’t expose dangerous names. This mirrors your “no relative/absolute/symlinks; double extensions blocked.”

Deterministic directory ordering (sort by UTF-8 + stable collation) to support repeatability.

4) Audit: what to log (without killing performance)

Don’t log every ReadFile call (Explorer will spam you). Instead:

Log these as “sensitive actions” (hash-chained, signed):

mount_start (case_id, mountpoint, host, tool build)

mount_stop

artifact_open only when a file handle is created (path, artifact/source hash reference) with rate-limiting

export_request (user clicked export/print)

print_report (report inputs/outputs + hashes)

This matches: “every sensitive action emits an audit event” while staying usable.

5) Mapping reads to evidence for provenance

When a file is opened in the mount:

Resolve virtual path → (domain, relativePath, fileID)

Include those identifiers in the artifact_open audit params and/or in an internal “open-handle context”.
This allows you to later show “this viewed file came from X with hash Y”.

6) Verification gates before mounting

On mount start:

Verify .styg signature + audit chain OR verify workspace manifest + audit chain (depending on mode). Your paper explicitly calls for validating signature + hash chain before opening proprietary .styg.

Re-hash a sample set or full set (configurable) and block on mismatch.

7) Printing/report generation from the mount

Don’t “print from mounted raw files”. Instead:

Printing uses report.json + manifest hash (embedded) and produces PDF/CSV/JSON/XML outputs with embedded manifest hash and optional signing/watermarking, exactly as stated.

stygion/
  Cargo.toml
  crates/
    stygion_core/
      Cargo.toml
      src/lib.rs
    stygion_schemas/
      Cargo.toml
      src/lib.rs
      src/schemas.rs
    db_inspector/
      Cargo.toml
      src/lib.rs
    nemesis/
      Cargo.toml
      src/main.rs
    mount_winfsp/
      Cargo.toml
      build.rs
      src/main.rs
  examples/
    license.basic.json
    license.enterprise.json
    license.gov.json
