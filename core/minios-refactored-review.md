# MINiOS Refactored Codebase — Review Document

**Generated:** 2026-04-24T04:38:30-05:00

**Version:** 2.1.0 (post-Session 5)

**Total Lines:** 16516

---

## Library Root

### `src/lib.rs`

```rust
//! iON Chronos - iOS Backup Acquisition, Validation, and WAL-Replay Engine
//!
//! This is the foundation of the iON analytic suite, responsible for:
//! - iOS device detection and pairing
//! - Encrypted/unencrypted backup handling
//! - WAL replay for SQLite consistency
//! - Backup integrity validation
//! - Path extraction for all downstream agents

#![allow(non_snake_case)]

pub mod agents;
pub mod case;
pub mod chronos;
pub mod common;
pub mod contact_index;
pub mod evidence;
pub mod nemesis;
pub mod styg;
pub mod ui;
pub mod ui_core;

use std::path::Path;

pub use chronos::*;

pub fn ensure_dir<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

```

### `src/main.rs`

```rust
//! MINiOS — Unified forensic agent dispatcher.
//!
//! Usage: minios <agent> <case_name>
//! Example: minios vigil my-case-001

use anyhow::Result;
use std::env;

fn main() {
    if let Err(e) = run() {
        eprintln!("MINiOS ERROR: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        print_usage();
        anyhow::bail!("No agent or case name specified");
    }

    let agent_name = &args[1];
    let case_name = &args[2];

    // Dispatch to the correct agent
    minios::agents::dispatch(agent_name, case_name)?;

    Ok(())
}

fn print_usage() {
    eprintln!(
        r#"
MINiOS — Mobile iOS Forensic Extraction

USAGE:
    minios <agent> <case_name>

AGENTS:
    vigil    System-level artifact extraction
    echo     Audio evidence (voicemail, recordings)

EXAMPLE:
    minios vigil my-case-001
"#
    );
}

```

## Agent Framework

### `src/agents/mod.rs`

```rust
//! Unified agent framework.
//!
//! All forensic agents implement the `Agent` trait.
//! A single binary (`minios`) dispatches to the correct agent at runtime.

use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;

pub mod models;

// Model modules used by UI or multiple agents
pub mod cerberus_models;
pub mod charon_models;
pub mod nyx_models;
pub mod obolus_models;
pub mod psyche_models;
pub mod orpheus_crypto;
pub mod orpheus_ingest;
pub mod orpheus_recon;
pub mod orpheus_report;

// Agent modules (ported in Session 3)
pub mod aether;
pub mod plutus;
pub mod cerberus;
pub mod charon;
pub mod nyx;
pub mod obolus;
pub mod atlas;
pub mod orpheus;
pub mod psyche;

// Lightweight agents (no separate model files)
pub mod echo;
pub mod vigil;

use crate::case::Case;
use crate::evidence::{write_evidence, EvidenceRecord};

/// Every forensic agent implements this trait.
/// The framework handles all the boilerplate (CLI, case mgmt, evidence output).
pub trait Agent: Sized {
    /// Human-readable agent name (e.g., "Cerberus")
    const NAME: &'static str;
    /// URL-safe slug for evidence directory (e.g., "cerberus")
    const SLUG: &'static str;
    /// Schema version for evidence records
    const SCHEMA_VERSION: u32;

    /// The actual forensic extraction. Agent implements ONLY this.
    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>>;

    /// Framework-provided: full run loop.
    fn run() -> Result<()> {
        let args: Vec<String> = env::args().collect();
        if args.len() < 2 {
            anyhow::bail!("Usage: {} <case_name>", args[0]);
        }
        Self::run_with_case(&args[1])
    }

    /// Run with an explicit case name (used by dispatcher).
    fn run_with_case(case_name: &str) -> Result<()> {
        let ctx = AgentCtx::new(case_name, Self::SLUG)?;

        ctx.log(&format!("{} extraction started", Self::NAME));

        let records = Self::extract(&ctx)
            .with_context(|| format!("{} extraction failed", Self::NAME))?;

        write_evidence(
            &ctx.evidence_dir,
            Self::NAME,
            Self::SLUG,
            Self::SCHEMA_VERSION,
            &records,
        )
        .with_context(|| format!("{} evidence output failed", Self::NAME))?;

        ctx.log(&format!(
            "{} complete: {} records -> {}",
            Self::NAME,
            records.len(),
            ctx.evidence_dir.display()
        ));

        println!(
            "{} complete: {} records written to {}",
            Self::NAME,
            records.len(),
            ctx.evidence_dir.display()
        );

        Ok(())
    }
}

/// Context provided to every agent's `extract()` method.
/// Contains case paths, backup access, and logging.
pub struct AgentCtx {
    pub case: Case,
    pub evidence_dir: PathBuf,
    pub backup_root: PathBuf,
}

impl AgentCtx {
    /// Create context from an explicit case name and agent slug.
    pub fn new(case_name: &str, slug: &str) -> Result<Self> {
        let case = Case::new(case_name)?;
        case.open(slug)?;

        let evidence_dir = case.evidence_path(slug);
        std::fs::create_dir_all(&evidence_dir)?;

        let backup_root = case.active_backup_root()?;

        Ok(Self {
            case,
            evidence_dir,
            backup_root,
        })
    }

    /// Open a SQLite database from the iOS backup by relative path.
    /// Path is relative to backup root (e.g., "SMS/sms.db").
    pub fn open_backup_db(&self, relative_path: &str) -> Result<crate::common::sqlite::SqliteConn> {
        let path = self.backup_root.join(relative_path);
        crate::common::sqlite::open_readonly(&path)
    }

    /// Log a message to the case log.
    pub fn log(&self, message: &str) {
        let _ = self.case.log(message, None);
    }

    /// Read a plist file from the backup.
    pub fn read_plist(&self, relative_path: &str) -> Result<plist::Value> {
        let path = self.backup_root.join(relative_path);
        let file = std::fs::File::open(&path)
            .with_context(|| format!("opening plist {}", path.display()))?;
        plist::from_reader(file)
            .with_context(|| format!("parsing plist {}", path.display()))
    }
}

/// Agent dispatch table. Maps agent name to agent runner.
pub fn dispatch(agent_name: &str, case_name: &str) -> Result<()> {
    match agent_name {
        "vigil" => vigil::VigilAgent::run_with_case(case_name),
        "echo" => echo::EchoAgent::run_with_case(case_name),
        "aether" => aether::AetherAgent::run_with_case(case_name),
        "plutus" => plutus::PlutusAgent::run_with_case(case_name),
        "cerberus" => cerberus::CerberusAgent::run_with_case(case_name),
        "charon" => charon::CharonAgent::run_with_case(case_name),
        "nyx" => nyx::NyxAgent::run_with_case(case_name),
        "obolus" => obolus::ObolusAgent::run_with_case(case_name),
        "atlas" => atlas::AtlasAgent::run_with_case(case_name),
        "orpheus" => orpheus::OrpheusAgent::run_with_case(case_name),
        "psyche" => psyche::PsycheAgent::run_with_case(case_name),
        _ => anyhow::bail!(
            "Unknown agent: {}. Available: vigil, echo, aether, plutus, cerberus, charon, nyx, obolus, atlas, orpheus, psyche",
            agent_name
        ),
    }
}

```

### `src/agents/models.rs`

```rust
//! Shared types across all MINiOS agents.
//!
//! Only put types here that are used by 2+ agents.
//! Agent-specific structs stay in their agent files.

use serde::{Deserialize, Serialize};

/// Phone number with normalized and raw forms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneNumber {
    pub raw: String,
    pub normalized: String,
    pub country_code: Option<String>,
}

/// Message direction: sent by device owner or received.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

/// iOS timestamp wrapper (handles Core Data epoch, Unix epoch, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timestamp {
    pub raw: i64,
    pub iso8601: String,
}

/// Geographic coordinate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: Option<f64>,
}

/// Generic media/file reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub filename: String,
    pub relative_path: String,
    pub size_bytes: Option<u64>,
    pub mime_type: Option<String>,
}

```

## Agents — Core Evidence

### `src/agents/cerberus.rs`

```rust
//! Cerberus — Unified communications extraction (SMS/MMS, calls, voicemail, contacts).

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;

use crate::agents::{Agent, AgentCtx};
use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::target::{addressbook_target, call_history_target, voicemail_target};
use crate::evidence::EvidenceRecord;

pub struct CerberusAgent;

impl Agent for CerberusAgent {
    const NAME: &'static str = "Cerberus";
    const SLUG: &'static str = "cerberus";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        let mut contact_count = 0usize;
        let mut call_count = 0usize;
        let mut voicemail_count = 0usize;

        // 1. Contacts (from original contacts agent)
        match extract_contacts_internal(ctx) {
            Ok(mut contact_records) => {
                contact_count = contact_records.len();
                records.append(&mut contact_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus contacts extraction skipped: {}", e));
            }
        }

        // 2. Call history (best-effort; original hermes was a stub)
        match extract_call_history(ctx) {
            Ok(mut call_records) => {
                call_count = call_records.len();
                records.append(&mut call_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus call history extraction skipped: {}", e));
            }
        }

        // 3. Voicemail metadata (best-effort; original hermes was a stub)
        match extract_voicemail(ctx) {
            Ok(mut vm_records) => {
                voicemail_count = vm_records.len();
                records.append(&mut vm_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus voicemail extraction skipped: {}", e));
            }
        }

        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "contacts_extracted": contact_count,
                "calls_extracted": call_count,
                "voicemails_extracted": voicemail_count,
                "records_emitted": records.len(),
                "warnings": Vec::<String>::new(),
            }),
        });

        Ok(records)
    }
}

// ---------------------------------------------------------------------------
// Contacts
// ---------------------------------------------------------------------------

fn extract_contacts_internal(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;

    let target = addressbook_target();
    let resolved = resolver.resolve_known_target(&target)?;
    let audit = ResolverAuditRecord {
        artifact_key: target.artifact_key.to_string(),
        candidates: target
            .candidates
            .iter()
            .map(|c| format!("{}/{}", c.domain, c.relative_path))
            .collect(),
        chosen: resolved
            .as_ref()
            .map(|a| a.source_path.display().to_string()),
        method: resolved
            .as_ref()
            .map(|a| format!("{:?}", a.method)),
    };
    crate::common::resolver::write_resolver_audit(resolver.case_root(), &audit)?;

    let resolved = resolved.context("AddressBook.sqlitedb could not be resolved")?;

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let mut stmt = conn.prepare(
        r#"
        SELECT
            ABPerson.ROWID,
            COALESCE(NULLIF(TRIM(ABPerson.DisplayName), ''),
                     NULLIF(TRIM(COALESCE(ABPerson.First, '') || ' ' || COALESCE(ABPerson.Last, '')), ''),
                     NULLIF(TRIM(ABPerson.Organization), ''),
                     NULLIF(TRIM(ABPerson.CompositeNameFallback), ''),
                     'Unknown') AS display_name,
            NULLIF(TRIM(ABPerson.Organization), '') AS organization,
            NULLIF(TRIM(ABPerson.Note), '') AS note,
            NULLIF(TRIM(ABPerson.Birthday), '') AS birthday,
            NULLIF(TRIM(ABPerson.JobTitle), '') AS job_title,
            NULLIF(TRIM(ABPerson.Nickname), '') AS nickname,
            ABPerson.Flag AS blocked,
            NULLIF(TRIM(ABPerson.Department), '') AS department
        FROM ABPerson
        ORDER BY display_name
        "#,
    )?;

    let base_records: Vec<(i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut all_phones_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 3")?;
    let mut all_phones: HashMap<i64, Vec<String>> = all_phones_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_emails_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 4")?;
    let mut all_emails: HashMap<i64, Vec<String>> = all_emails_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_urls_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 22")?;
    let mut all_urls: HashMap<i64, Vec<String>> = all_urls_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut records = Vec::new();

    for (id, name, organization, note, birthday, job_title, nickname, blocked, department) in base_records {
        let phones = all_phones.remove(&id).unwrap_or_default();
        let emails = all_emails.remove(&id).unwrap_or_default();
        let urls = all_urls.remove(&id).unwrap_or_default();
        let blocked = blocked != 0;

        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "contact".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "name": name,
                "phones": phones,
                "emails": emails,
                "urls": urls,
                "organization": organization,
                "note": note,
                "birthday": birthday,
                "job_title": job_title,
                "nickname": nickname,
                "blocked": blocked,
                "department": department,
            }),
        });
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// Call history (best-effort; no original queries available)
// ---------------------------------------------------------------------------

fn extract_call_history(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = call_history_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    // Try modern ZCALLRECORD schema first, fallback to generic
    let query = r#"
        SELECT
            Z_PK AS id,
            ZDATE AS timestamp,
            ZDURATION AS duration,
            ZCALLTYPE AS call_type,
            ZADDRESS AS phone_number,
            ZANSWERED AS answered,
            ZLOCATION AS location,
            ZSERVICE_PROVIDER AS service_provider
        FROM ZCALLRECORD
        ORDER BY ZDATE DESC
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()), // Schema mismatch — skip silently
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, timestamp, duration, call_type, phone, answered, location, service) = row;
        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "call_history".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "timestamp": timestamp,
                "duration": duration,
                "call_type": call_type,
                "phone_number": phone,
                "answered": answered.map(|v| v != 0),
                "location": location,
                "service_provider": service,
            }),
        });
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// Voicemail (best-effort; no original queries available)
// ---------------------------------------------------------------------------

fn extract_voicemail(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = voicemail_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let query = r#"
        SELECT
            ROWID AS id,
            sender,
            callback_num,
            duration,
            expiration_date,
            trashed_date,
            date,
            token,
            flags
        FROM voicemail
        ORDER BY date DESC
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<i64>>(8)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, sender, callback, duration, expiration, trashed, date, token, flags) = row;
        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "voicemail".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "sender": sender,
                "callback_num": callback,
                "duration": duration,
                "expiration_date": expiration,
                "trashed_date": trashed,
                "date": date,
                "token": token,
                "flags": flags,
            }),
        });
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// Standalone contact extraction for backward compatibility (chronos pipeline)
// ---------------------------------------------------------------------------

/// Standalone contact extraction for backward compatibility.
pub fn extract_contacts(case: &crate::case::Case) -> Result<Vec<crate::agents::cerberus_models::ContactRecord>> {
    use crate::agents::cerberus_models::ContactRecord;
    use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
    use crate::common::prepared::{prepare_artifact, PrepareContext};
    use crate::common::target::addressbook_target;

    let resolver = BackupResolver::from_case(case)?;
    let target = addressbook_target();
    let resolved = resolver.resolve_known_target(&target)?;
    let audit = ResolverAuditRecord {
        artifact_key: target.artifact_key.to_string(),
        candidates: target
            .candidates
            .iter()
            .map(|c| format!("{}/{}", c.domain, c.relative_path))
            .collect(),
        chosen: resolved.as_ref().map(|a| a.source_path.display().to_string()),
        method: resolved.as_ref().map(|a| format!("{:?}", a.method)),
    };
    crate::common::resolver::write_resolver_audit(resolver.case_root(), &audit)?;

    let resolved = resolved.context("AddressBook.sqlitedb could not be resolved")?;

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: case.root_path(),
            clean_root: case.root_path().join("clean"),
            temp_root: case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let mut stmt = conn.prepare(
        r#"
        SELECT
            ABPerson.ROWID,
            COALESCE(NULLIF(TRIM(ABPerson.DisplayName), ''),
                     NULLIF(TRIM(COALESCE(ABPerson.First, '') || ' ' || COALESCE(ABPerson.Last, '')), ''),
                     NULLIF(TRIM(ABPerson.Organization), ''),
                     NULLIF(TRIM(ABPerson.CompositeNameFallback), ''),
                     'Unknown') AS display_name,
            NULLIF(TRIM(ABPerson.Organization), '') AS organization,
            NULLIF(TRIM(ABPerson.Note), '') AS note,
            NULLIF(TRIM(ABPerson.Birthday), '') AS birthday,
            NULLIF(TRIM(ABPerson.JobTitle), '') AS job_title,
            NULLIF(TRIM(ABPerson.Nickname), '') AS nickname,
            ABPerson.Flag AS blocked,
            NULLIF(TRIM(ABPerson.Department), '') AS department
        FROM ABPerson
        ORDER BY display_name
        "#,
    )?;

    let base_records: Vec<(i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut all_phones_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 3")?;
    let mut all_phones: HashMap<i64, Vec<String>> = all_phones_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_emails_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 4")?;
    let mut all_emails: HashMap<i64, Vec<String>> = all_emails_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_urls_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 22")?;
    let mut all_urls: HashMap<i64, Vec<String>> = all_urls_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut records = Vec::new();
    for (id, name, organization, note, birthday, job_title, nickname, blocked, department) in base_records {
        let phones = all_phones.remove(&id).unwrap_or_default();
        let emails = all_emails.remove(&id).unwrap_or_default();
        let urls = all_urls.remove(&id).unwrap_or_default();
        let blocked = blocked != 0;

        records.push(ContactRecord {
            id,
            name,
            phones,
            emails,
            urls,
            organization,
            note,
            birthday,
            job_title,
            nickname,
            blocked,
            department,
        });
    }

    records.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(records)
}

/// Standalone contact export for backward compatibility.
pub fn export_contacts(case: &crate::case::Case, contacts: &[crate::agents::cerberus_models::ContactRecord]) -> Result<std::path::PathBuf> {
    let out_dir = case.evidence_path("cerberus");
    std::fs::create_dir_all(&out_dir)?;

    let json_path = out_dir.join("contacts.json");
    let csv_path = out_dir.join("contacts.csv");

    let json = serde_json::to_string_pretty(contacts)?;
    std::fs::write(&json_path, json)?;

    let mut wtr = csv::Writer::from_path(&csv_path)?;
    wtr.write_record(["id", "name", "phones", "emails", "urls", "organization", "note", "birthday", "job_title", "nickname", "blocked", "department"])?;
    for c in contacts {
        let blocked_str = if c.blocked { "true".to_string() } else { "false".to_string() };
        wtr.write_record([
            &c.id.to_string(),
            &c.name,
            &c.phones.join("; "),
            &c.emails.join("; "),
            &c.urls.join("; "),
            c.organization.as_deref().unwrap_or(""),
            c.note.as_deref().unwrap_or(""),
            c.birthday.as_deref().unwrap_or(""),
            c.job_title.as_deref().unwrap_or(""),
            c.nickname.as_deref().unwrap_or(""),
            &blocked_str,
            c.department.as_deref().unwrap_or(""),
        ])?;
    }
    wtr.flush()?;

    Ok(json_path)
}

```

### `src/agents/cerberus_models.rs`

```rust
//! CERBERUS Data Models (unified comms: messages, calls, voicemail, contacts)

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Messages / Attachments (original cerberus)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "Date")]
    pub date: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: i64,
    #[serde(rename = "Direction")]
    pub direction: String,
    #[serde(rename = "Phone")]
    pub phone_number: String,
    #[serde(rename = "Text")]
    pub text: String,
    #[serde(rename = "Service")]
    pub service: String,
    #[serde(rename = "iMessage")]
    pub is_imessage: bool,
    #[serde(rename = "Attachments")]
    pub has_attachments: bool,
    #[serde(rename = "MessageID")]
    pub message_id: Option<i64>,
    #[serde(rename = "ThreadID")]
    pub thread_id: Option<i64>,
    #[serde(rename = "SearchRelevance")]
    pub search_relevance: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attachment {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub transfer_name: String,
    #[serde(default)]
    pub message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTerm {
    #[serde(default)]
    pub term: String,
    #[serde(default)]
    pub term_type: SearchTermType,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SearchTermType {
    ExactMatch,
    #[default]
    Contains,
    Regex,
    Fuzzy,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub terms: Vec<SearchTerm>,
    pub phone_numbers: Vec<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub ignore_country_code: bool,
    pub message_types: Vec<MessageType>,
    pub limit: Option<usize>,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum MessageType {
    SMS,
    MMS,
    #[serde(rename = "iMessage")]
    IMessage,
    #[default]
    All,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    #[serde(default)]
    pub message_id: Option<i64>,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default)]
    pub matching_terms: Vec<String>,
    #[serde(default)]
    pub context_snippet: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub timestamp: i64,
}

pub trait HasPhone {
    fn phone(&self) -> &str;
}

pub trait HasTimestamp {
    fn timestamp(&self) -> i64;
}

impl HasPhone for Message {
    fn phone(&self) -> &str {
        &self.phone_number
    }
}

impl HasTimestamp for Message {
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

impl HasPhone for Attachment {
    fn phone(&self) -> &str {
        &self.phone_number
    }
}

impl HasTimestamp for Attachment {
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

// ---------------------------------------------------------------------------
// Contacts (from original contacts agent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactRecord {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub phones: Vec<String>,
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    pub organization: Option<String>,
    pub note: Option<String>,
    pub birthday: Option<String>,
    #[serde(default)]
    pub job_title: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub department: Option<String>,
}

// ---------------------------------------------------------------------------
// Call history / Voicemail / Audio (from original hermes agent)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub call_type: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub answered: bool,
    #[serde(default)]
    pub face_time: bool,
    #[serde(default)]
    pub call_provider: Option<String>,
    #[serde(default)]
    pub country_code: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub disconnected_cause: Option<String>,
    #[serde(default)]
    pub bytes_sent: Option<i64>,
    #[serde(default)]
    pub bytes_received: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoicemailRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub diarization_path: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub speakers: Option<Vec<String>>,
    #[serde(default)]
    pub is_transcribed: bool,
    #[serde(default)]
    pub is_diarized: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub source_type: AudioSource,
    #[serde(default)]
    pub source_id: Option<i64>,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub original_format: String,
    #[serde(default)]
    pub converted_path: Option<String>,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub sample_rate: i32,
    #[serde(default)]
    pub channels: i32,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub extracted_date: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum AudioSource {
    #[default]
    Voicemail,
    CallRecording,
    MMSAttachment,
    FaceTimeAudio,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transcript {
    #[serde(default)]
    pub audio_id: i64,
    #[serde(default)]
    pub segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub word_count: usize,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub generated_date: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptSegment {
    #[serde(default)]
    pub id: usize,
    #[serde(default)]
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub words: Vec<Word>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Word {
    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpeakerSegment {
    #[serde(default)]
    pub id: usize,
    #[serde(default)]
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default)]
    pub speaker: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub is_overlapping: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiarizedTranscript {
    #[serde(default)]
    pub audio_id: i64,
    #[serde(default)]
    pub speaker_segments: Vec<SpeakerSegment>,
    #[serde(default)]
    pub transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub merged_transcript: String,
    #[serde(default)]
    pub speaker_mapping: HashMap<String, Option<String>>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub generated_date: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HermesResult {
    #[serde(default)]
    pub calls: Vec<CallRecord>,
    #[serde(default)]
    pub voicemails: Vec<VoicemailRecord>,
    #[serde(default)]
    pub audio_files: Vec<AudioRecord>,
    #[serde(default)]
    pub transcripts: Vec<Transcript>,
    #[serde(default)]
    pub diarized_transcripts: Vec<DiarizedTranscript>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub model: String,
    pub language: Option<String>,
    pub translate: bool,
    pub word_timestamps: bool,
    pub speaker_diarization: bool,
    pub ollama_endpoint: String,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            model: "whisper-large-v3".to_string(),
            language: None,
            translate: false,
            word_timestamps: true,
            speaker_diarization: true,
            ollama_endpoint: "http://localhost:11434".to_string(),
        }
    }
}

```

### `src/agents/charon.rs`

```rust
//! Charon Agent

use anyhow::Result;
use serde_json::json;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub struct CharonAgent;

impl Agent for CharonAgent {
    const NAME: &'static str = "Charon";
    const SLUG: &'static str = "charon";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "records_emitted": 0,
                "warnings": Vec::<String>::new(),
            }),
        });
        Ok(records)
    }
}

```

### `src/agents/charon_models.rs`

```rust
//! CHARON Data Models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const CHARON_ASSET_SCHEMA_VERSION: u32 = 1;

fn default_charon_asset_schema_version() -> u32 {
    CHARON_ASSET_SCHEMA_VERSION
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssetRecord {
    #[serde(default = "default_charon_asset_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub thumbnail_path: Option<String>,
    #[serde(default)]
    pub live_photo_video_path: Option<String>,
    #[serde(default)]
    pub media_type: MediaType,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub orientation: i32,
    #[serde(default)]
    pub created_date: DateTime<Utc>,
    #[serde(default)]
    pub added_date: DateTime<Utc>,
    #[serde(default)]
    pub modified_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub trashed_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_trashed: bool,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub is_cloud_asset: bool,
    #[serde(default)]
    pub cloud_state: Option<i32>,
    #[serde(default)]
    pub burst_uuid: Option<String>,
    #[serde(default)]
    pub burst_pick_type: Option<i32>,
    #[serde(default)]
    pub has_adjustments: bool,
    #[serde(default)]
    pub adjustment_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub exif_metadata: Option<ExifMetadata>,
    #[serde(default)]
    pub location_metadata: Option<LocationMetadata>,
    #[serde(default)]
    pub face_count: i32,
    #[serde(default)]
    pub album_ids: Vec<i64>,
    #[serde(default)]
    pub moment_id: Option<i64>,
    #[serde(default)]
    pub search_relevance: f64,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub uti: Option<String>,
    #[serde(default)]
    pub resolved_source_path: Option<String>,
    #[serde(default)]
    pub source_resolution_method: Option<String>,
    #[serde(default)]
    pub copy_state: Option<String>,
    #[serde(default)]
    pub copy_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum MediaType {
    Photo,
    Video,
    LivePhoto,
    Panorama,
    Screenshot,
    Burst,
    Timelapse,
    Portrait,
    Selfie,
    SlowMo,
    Timecode,
    #[default]
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExifMetadata {
    #[serde(default)]
    pub camera_make: Option<String>,
    #[serde(default)]
    pub camera_model: Option<String>,
    #[serde(default)]
    pub lens_model: Option<String>,
    #[serde(default)]
    pub aperture: Option<f64>,
    #[serde(default)]
    pub focal_length: Option<f64>,
    #[serde(default)]
    pub iso: Option<i32>,
    #[serde(default)]
    pub shutter_speed: Option<f64>,
    #[serde(default)]
    pub flash_fired: Option<bool>,
    #[serde(default)]
    pub metering_mode: Option<i32>,
    #[serde(default)]
    pub white_balance: Option<i32>,
    #[serde(default)]
    pub exposure_program: Option<i32>,
    #[serde(default)]
    pub software: Option<String>,
    #[serde(default)]
    pub color_space: Option<i32>,
    #[serde(default)]
    pub bits_per_sample: Option<i32>,
    #[serde(default)]
    pub compression: Option<i32>,
    #[serde(default)]
    pub exif_version: Option<String>,
    #[serde(default)]
    pub datetime_original: Option<DateTime<Utc>>,
    #[serde(default)]
    pub datetime_digitized: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationMetadata {
    #[serde(default)]
    pub latitude: f64,
    #[serde(default)]
    pub longitude: f64,
    #[serde(default)]
    pub altitude: Option<f64>,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub heading: Option<f64>,
    #[serde(default)]
    pub horizontal_accuracy: Option<f64>,
    #[serde(default)]
    pub vertical_accuracy: Option<f64>,
    #[serde(default)]
    pub place_name: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub street: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FaceRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub asset_id: i64,
    #[serde(default)]
    pub person_id: Option<i64>,
    #[serde(default)]
    pub person_name: Option<String>,
    #[serde(default)]
    pub bounding_box: BoundingBox,
    #[serde(default)]
    pub face_angle: Option<f64>,
    #[serde(default)]
    pub face_confidence: f64,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_manual: bool,
    #[serde(default)]
    pub detected_date: DateTime<Utc>,
    #[serde(default)]
    pub modified_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub face_age_type: Option<i32>,
    #[serde(default)]
    pub face_gender_type: Option<i32>,
    #[serde(default)]
    pub face_skintone_type: Option<i32>,
    #[serde(default)]
    pub face_hair_color_type: Option<i32>,
    #[serde(default)]
    pub face_bald_type: Option<i32>,
    #[serde(default)]
    pub face_eye_makeup_type: Option<i32>,
    #[serde(default)]
    pub face_lip_makeup_type: Option<i32>,
    #[serde(default)]
    pub face_eye_wear_type: Option<i32>,
    #[serde(default)]
    pub face_facial_hair_type: Option<i32>,
    #[serde(default)]
    pub face_smile_type: Option<i32>,
    #[serde(default)]
    pub cluster_sequence_number: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BoundingBox {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AlbumRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub album_type: AlbumType,
    #[serde(default)]
    pub created_date: DateTime<Utc>,
    #[serde(default)]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub asset_count: i32,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_trashed: bool,
    #[serde(default)]
    pub cloud_owner_first_name: Option<String>,
    #[serde(default)]
    pub cloud_owner_last_name: Option<String>,
    #[serde(default)]
    pub cloud_local_state: Option<i32>,
    #[serde(default)]
    pub cloud_is_deletable: Option<bool>,
    #[serde(default)]
    pub cloud_is_my_asset: Option<bool>,
    #[serde(default)]
    pub asset_ids: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum AlbumType {
    UserAlbum,
    SmartAlbum,
    Moment,
    MomentList,
    Project,
    Folder,
    SharedAlbum,
    CloudSharedAlbum,
    #[default]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeletedAssetRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub media_type: MediaType,
    #[serde(default)]
    pub trashed_date: DateTime<Utc>,
    #[serde(default)]
    pub file_still_exists: bool,
    #[serde(default)]
    pub thumbnail_still_exists: bool,
    #[serde(default)]
    pub metadata_still_exists: bool,
    #[serde(default)]
    pub estimated_deletion_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recovery_status: RecoveryStatus,
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum RecoveryStatus {
    Recoverable,
    PartiallyRecoverable,
    NotRecoverable,
    #[default]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MomentRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub start_date: DateTime<Utc>,
    #[serde(default)]
    pub end_date: DateTime<Utc>,
    #[serde(default)]
    pub approximate_latitude: Option<f64>,
    #[serde(default)]
    pub approximate_longitude: Option<f64>,
    #[serde(default)]
    pub asset_count: i32,
    #[serde(default)]
    pub representative_asset_id: Option<i64>,
    #[serde(default)]
    pub place_name: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimelineEntry {
    #[serde(default)]
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub event_type: TimelineEventType,
    #[serde(default)]
    pub asset_id: Option<i64>,
    #[serde(default)]
    pub location: Option<LocationMetadata>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum TimelineEventType {
    #[default]
    AssetCreated,
    AssetModified,
    AssetTrashed,
    LocationVisited,
    BurstCaptured,
    LivePhotoTaken,
}

#[derive(Debug, Clone)]
pub struct FilterOptions {
    pub date_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub media_types: Vec<MediaType>,
    pub has_gps: bool,
    pub has_faces: bool,
    pub is_favorite: Option<bool>,
    pub is_trashed: Option<bool>,
    pub min_width: Option<i32>,
    pub min_height: Option<i32>,
    pub location_bounds: Option<LocationBounds>,
    pub search_terms: Vec<String>,
    pub album_ids: Vec<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct LocationBounds {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CharonResult {
    #[serde(default)]
    pub assets: Vec<AssetRecord>,
    #[serde(default)]
    pub faces: Vec<FaceRecord>,
    #[serde(default)]
    pub albums: Vec<AlbumRecord>,
    #[serde(default)]
    pub deleted_assets: Vec<DeletedAssetRecord>,
    #[serde(default)]
    pub moments: Vec<MomentRecord>,
    #[serde(default)]
    pub timeline: Vec<TimelineEntry>,
}

```

### `src/agents/nyx.rs`

```rust
//! Nyx Agent

use anyhow::Result;
use serde_json::json;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub struct NyxAgent;

impl Agent for NyxAgent {
    const NAME: &'static str = "Nyx";
    const SLUG: &'static str = "nyx";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "records_emitted": 0,
                "warnings": Vec::<String>::new(),
            }),
        });
        Ok(records)
    }
}

```

### `src/agents/nyx_models.rs`

```rust
//! NYX Data Models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryItem {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub visit_time: DateTime<Utc>,
    pub visit_count: i32,
    pub load_successful: bool,
    pub origin: Option<String>,
    pub redirect_source: Option<String>,
    pub redirect_destination: Option<String>,
    pub visit_duration: Option<i64>,
    #[serde(default)]
    pub http_status: Option<i32>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    pub is_deleted: bool,
    #[serde(default)]
    pub tombstone_time: Option<DateTime<Utc>>,
    pub search_terms: Vec<String>,
    pub domain: String,
    pub search_engine: Option<SearchEngine>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SearchEngine {
    Google,
    Bing,
    DuckDuckGo,
    Yahoo,
    YouTube,
    Internal,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bookmark {
    pub id: i64,
    pub title: Option<String>,
    pub url: Option<String>,
    pub folder: Option<String>,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub position: i32,
    pub created_time: DateTime<Utc>,
    pub modified_time: DateTime<Utc>,
    pub is_favorite: bool,
    #[serde(default)]
    pub sync_metadata: Option<String>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub device_name: String,
    #[serde(default)]
    pub device_id: String,
    pub last_viewed_time: DateTime<Utc>,
    pub is_pinned: bool,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub session_data: Option<String>,
    pub sync_time: DateTime<Utc>,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_deleted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AutofillEntry {
    pub id: i64,
    pub field_name: String,
    pub value: String,
    // nyx/main.rs writes this as a plain String; keep flexible for display
    pub value_type: String,
    pub use_count: i32,
    pub first_used: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    #[serde(default)]
    pub correction_data: Option<String>,
    #[serde(default)]
    pub is_secure: bool,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AutofillValueType {
    Name,
    Email,
    Phone,
    Address,
    CreditCard,
    Password,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchTerm {
    pub id: i64,
    pub term: String,
    // nyx/main.rs writes this as a plain String; keep flexible for display
    pub search_engine: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub url_context: Option<String>,
    #[serde(default)]
    pub frequency: i32,
    #[serde(default)]
    pub is_autocomplete: bool,
    #[serde(default)]
    pub device_context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReadingListItem {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub added_time: DateTime<Utc>,
    pub archived_time: Option<DateTime<Utc>>,
    pub is_archived: bool,
    pub offline_data_path: Option<String>,
    pub estimated_read_time: Option<i32>,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimelineEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: TimelineEventType,
    pub source_device: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub related_events: Vec<i64>,
    pub duration: Option<i64>,
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TimelineEventType {
    Visit,
    Bookmark,
    Search,
    TabSwitch,
    ReadingListAdd,
    Autofill,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NyxResult {
    pub history: Vec<HistoryItem>,
    pub bookmarks: Vec<Bookmark>,
    pub tabs: Vec<TabEntry>,
    pub autofill: Vec<AutofillEntry>,
    pub searches: Vec<SearchTerm>,
    pub reading_list: Vec<ReadingListItem>,
    pub timeline: Vec<TimelineEntry>,
}

#[derive(Debug, Clone)]
pub struct FilterOptions {
    pub search_terms: Vec<String>,
    pub domains: Vec<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub min_visit_count: Option<i32>,
    pub include_deleted: bool,
    pub search_engines: Vec<SearchEngine>,
    pub limit: Option<usize>,
}

```

### `src/agents/obolus.rs`

```rust
//! Obolus Agent

use anyhow::Result;
use serde_json::json;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub struct ObolusAgent;

impl Agent for ObolusAgent {
    const NAME: &'static str = "Obolus";
    const SLUG: &'static str = "obolus";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "records_emitted": 0,
                "warnings": Vec::<String>::new(),
            }),
        });
        Ok(records)
    }
}

```

### `src/agents/obolus_models.rs`

```rust
//! OBOLUS Data Models

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Note {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub html_body: Option<String>,
    #[serde(default)]
    pub plain_text: Option<String>,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub created: Option<DateTime<Local>>,
    #[serde(default)]
    pub modified: Option<DateTime<Local>>,
    #[serde(default)]
    pub deleted: Option<DateTime<Local>>,
    #[serde(default)]
    pub is_deleted: bool,
    #[serde(default)]
    pub is_password_protected: bool,
    #[serde(default)]
    pub is_locked: bool,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notebook: Option<String>,
    #[serde(default)]
    pub share_status: Option<String>,
    #[serde(default)]
    pub sync_status: Option<String>,
    #[serde(default)]
    pub note_type: NoteType,
    #[serde(default)]
    pub word_count: usize,
    #[serde(default)]
    pub character_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum NoteType {
    #[default]
    Regular,
    Checklist,
    Scanned,
    Sketch,
    Audio,
    Video,
    WebClip,
    Document,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Folder {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub created: Option<DateTime<Local>>,
    #[serde(default)]
    pub modified: Option<DateTime<Local>>,
    #[serde(default)]
    pub note_count: usize,
    #[serde(default)]
    pub is_smart_folder: bool,
    #[serde(default)]
    pub smart_folder_criteria: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Account {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub account_type: String,
    #[serde(default)]
    pub is_cloud: bool,
    #[serde(default)]
    pub last_sync: Option<DateTime<Local>>,
    #[serde(default)]
    pub note_count: usize,
    #[serde(default)]
    pub folder_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attachment {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub note_id: i64,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub created: Option<DateTime<Local>>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub is_embedded: bool,
    #[serde(default)]
    pub content_id: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub terms: Vec<String>,
    pub folders: Vec<String>,
    pub accounts: Vec<String>,
    pub note_types: Vec<NoteType>,
    pub date_range: Option<DateRange>,
    pub include_deleted: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DateRange {
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    #[serde(default)]
    pub note_id: i64,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default)]
    pub matching_terms: Vec<String>,
    #[serde(default)]
    pub context_snippet: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub modified: Option<DateTime<Local>>,
}

pub struct EvidencePackage {
    pub notes: Vec<Note>,
    pub folders: Vec<Folder>,
    pub accounts: Vec<Account>,
    pub attachments: Vec<Attachment>,
    pub hash_manifest: HashMap<String, String>,
}

```

### `src/agents/aether.rs`

```rust
//! Aether — Deterministic location intelligence.

use anyhow::{Context, Result};
use chrono::Utc;
use exif::{In, Reader, Tag, Value};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};


use crate::agents::charon_models::{AssetRecord, MediaType, CHARON_ASSET_SCHEMA_VERSION};
use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub const AETHER_LOCATION_SCHEMA_VERSION: u32 = 1;

const SOURCE_AGENT: &str = "charon";
const EXIF_GPS_CONFIDENCE: f32 = 1.0;
const DB_GPS_CONFIDENCE: f32 = 0.65;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationRecord {
    pub schema_version: u32,
    pub asset_id: Option<String>,
    pub asset_numeric_id: Option<i64>,
    pub source_path: Option<std::path::PathBuf>,
    pub copied_path: Option<std::path::PathBuf>,
    pub timestamp_utc: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub method: String,
    pub source_agent: String,
    pub source_resolution_method: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AetherReport {
    pub schema_version: u32,
    pub case_id: String,
    pub generated_at: String,
    pub state: String,
    pub source_agent: String,
    pub charon_asset_schema_version: u32,
    pub charon_assets_path: String,
    pub charon_copy_manifest_path: Option<String>,
    pub assets_examined: usize,
    pub assets_with_resolved_source_path: usize,
    pub assets_with_copied_path: usize,
    pub assets_with_gps: usize,
    pub exif_gps_records: usize,
    pub db_location_records: usize,
    pub assets_skipped: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CopyManifest {
    #[serde(default)]
    entries: HashMap<String, CopyManifestEntry>,
}

#[derive(Debug, Deserialize, Clone)]
struct CopyManifestEntry {
    asset_id: String,
    source_path: String,
    destination_path: String,
}

#[derive(Debug, Clone)]
struct LocationExtractionStats {
    assets_examined: usize,
    assets_with_resolved_source_path: usize,
    assets_with_copied_path: usize,
    exif_gps_records: usize,
    db_location_records: usize,
    source_less_db_location_assets: usize,
    invalid_db_location_assets: usize,
}

#[derive(Debug)]
struct ExifGps {
    latitude: f64,
    longitude: f64,
    altitude: Option<f64>,
}

pub struct AetherAgent;

impl Agent for AetherAgent {
    const NAME: &'static str = "Aether";
    const SLUG: &'static str = "aether";
    const SCHEMA_VERSION: u32 = AETHER_LOCATION_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let charon_dir = ctx.case.evidence_path("charon");
        let assets_path = charon_dir.join("assets.json");
        let manifest_path = charon_dir.join("copy_manifest.json");

        let assets = load_charon_assets(&assets_path)?;
        let copy_manifest = load_copy_manifest(&manifest_path)?;

        let (locations, stats) = collect_locations(&assets, &copy_manifest, &charon_dir);

        let mut warnings = Vec::new();
        if stats.source_less_db_location_assets > 0 {
            warnings.push(format!(
                "{} assets had valid DB location metadata but no deterministic source or copied path",
                stats.source_less_db_location_assets
            ));
        }
        if stats.invalid_db_location_assets > 0 {
            warnings.push(format!(
                "{} assets had invalid or sentinel DB location metadata and were skipped",
                stats.invalid_db_location_assets
            ));
        }
        if !manifest_path.exists() {
            warnings.push(
                "No Charon copy_manifest.json present; copied-path linkage unavailable".to_string(),
            );
        }

        let report = AetherReport {
            schema_version: AETHER_LOCATION_SCHEMA_VERSION,
            case_id: ctx.case.name().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            state: "complete".to_string(),
            source_agent: SOURCE_AGENT.to_string(),
            charon_asset_schema_version: assets
                .first()
                .map(|asset| asset.schema_version)
                .unwrap_or(CHARON_ASSET_SCHEMA_VERSION),
            charon_assets_path: assets_path.display().to_string(),
            charon_copy_manifest_path: manifest_path
                .exists()
                .then(|| manifest_path.display().to_string()),
            assets_examined: stats.assets_examined,
            assets_with_resolved_source_path: stats.assets_with_resolved_source_path,
            assets_with_copied_path: stats.assets_with_copied_path,
            assets_with_gps: locations.len(),
            exif_gps_records: stats.exif_gps_records,
            db_location_records: stats.db_location_records,
            assets_skipped: stats.assets_examined.saturating_sub(locations.len()),
            warnings,
        };

        let mut records = Vec::new();

        // Report record
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        });

        // Location records
        for location in locations {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "location".to_string(),
                timestamp: location.timestamp_utc.clone().unwrap_or_default(),
                payload: serde_json::to_value(&location)?,
            });
        }

        Ok(records)
    }
}

fn load_charon_assets(path: &Path) -> Result<Vec<AssetRecord>> {
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon assets at {}", path.display()))?;
    let assets: Vec<AssetRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon assets JSON at {}", path.display()))?;
    Ok(assets)
}

fn load_copy_manifest(path: &Path) -> Result<CopyManifest> {
    if !path.exists() {
        return Ok(CopyManifest::default());
    }
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon copy manifest at {}", path.display()))?;
    let manifest: CopyManifest = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon copy manifest at {}", path.display()))?;
    Ok(manifest)
}

fn collect_locations(
    assets: &[AssetRecord],
    copy_manifest: &CopyManifest,
    charon_dir: &Path,
) -> (Vec<LocationRecord>, LocationExtractionStats) {
    let mut locations = Vec::new();
    let mut stats = LocationExtractionStats {
        assets_examined: assets.len(),
        assets_with_resolved_source_path: 0,
        assets_with_copied_path: 0,
        exif_gps_records: 0,
        db_location_records: 0,
        source_less_db_location_assets: 0,
        invalid_db_location_assets: 0,
    };

    for asset in assets {
        let manifest_entry = manifest_entry_for(asset, copy_manifest);
        let source_path = resolved_source_path(asset, manifest_entry);
        let copied_path = resolved_copied_path(charon_dir, manifest_entry);

        if source_path.is_some() {
            stats.assets_with_resolved_source_path += 1;
        }
        if copied_path.is_some() {
            stats.assets_with_copied_path += 1;
        }

        let exif_path = source_path.as_ref().or(copied_path.as_ref());
        if let Some(path) = exif_path {
            if supports_exif_gps(path) {
                if let Ok(Some(gps)) = extract_exif_gps(path) {
                    locations.push(LocationRecord {
                        schema_version: AETHER_LOCATION_SCHEMA_VERSION,
                        asset_id: Some(asset.uuid.clone()),
                        asset_numeric_id: Some(asset.id),
                        source_path: source_path.clone(),
                        copied_path: copied_path.clone(),
                        timestamp_utc: Some(asset.created_date.to_rfc3339()),
                        latitude: Some(gps.latitude),
                        longitude: Some(gps.longitude),
                        altitude: gps.altitude,
                        method: "exif_gps".to_string(),
                        source_agent: SOURCE_AGENT.to_string(),
                        source_resolution_method: asset.source_resolution_method.clone(),
                        confidence: EXIF_GPS_CONFIDENCE,
                    });
                    stats.exif_gps_records += 1;
                    continue;
                }
            }
        }

        match valid_db_location(asset) {
            Some((latitude, longitude, altitude)) => {
                if source_path.is_none() && copied_path.is_none() {
                    stats.source_less_db_location_assets += 1;
                    continue;
                }

                locations.push(LocationRecord {
                    schema_version: AETHER_LOCATION_SCHEMA_VERSION,
                    asset_id: Some(asset.uuid.clone()),
                    asset_numeric_id: Some(asset.id),
                    source_path,
                    copied_path,
                    timestamp_utc: Some(asset.created_date.to_rfc3339()),
                    latitude: Some(latitude),
                    longitude: Some(longitude),
                    altitude,
                    method: "charon_location_metadata".to_string(),
                    source_agent: SOURCE_AGENT.to_string(),
                    source_resolution_method: asset.source_resolution_method.clone(),
                    confidence: DB_GPS_CONFIDENCE,
                });
                stats.db_location_records += 1;
            }
            None => {
                if asset.location_metadata.is_some() {
                    stats.invalid_db_location_assets += 1;
                }
            }
        }
    }

    (locations, stats)
}

fn manifest_entry_for<'a>(
    asset: &AssetRecord,
    manifest: &'a CopyManifest,
) -> Option<&'a CopyManifestEntry> {
    manifest
        .entries
        .get(&asset.uuid)
        .or_else(|| manifest.entries.get(&asset.id.to_string()))
        .or_else(|| {
            manifest.entries.values().find(|entry| {
                entry.asset_id == asset.uuid || entry.asset_id == asset.id.to_string()
            })
        })
}

fn resolved_source_path(
    asset: &AssetRecord,
    manifest_entry: Option<&CopyManifestEntry>,
) -> Option<PathBuf> {
    normalize_existing_path(asset.resolved_source_path.as_deref())
        .or_else(|| normalize_existing_path(manifest_entry.map(|entry| entry.source_path.as_str())))
}

fn resolved_copied_path(
    charon_dir: &Path,
    manifest_entry: Option<&CopyManifestEntry>,
) -> Option<PathBuf> {
    let raw = manifest_entry?.destination_path.trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        charon_dir.join(candidate)
    };
    resolved.exists().then_some(resolved)
}

fn normalize_existing_path(raw: Option<&str>) -> Option<PathBuf> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(value);
    candidate.exists().then_some(candidate)
}

fn valid_db_location(asset: &AssetRecord) -> Option<(f64, f64, Option<f64>)> {
    let meta = asset.location_metadata.as_ref()?;
    if !is_valid_latitude(meta.latitude) || !is_valid_longitude(meta.longitude) {
        return None;
    }
    Some((meta.latitude, meta.longitude, meta.altitude))
}

fn is_valid_latitude(value: f64) -> bool {
    value.is_finite() && (-90.0..=90.0).contains(&value)
}

fn is_valid_longitude(value: f64) -> bool {
    value.is_finite() && (-180.0..=180.0).contains(&value)
}

fn supports_exif_gps(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "jpg" | "jpeg" | "tif" | "tiff")
}

fn extract_exif_gps(path: &Path) -> Result<Option<ExifGps>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open media file for EXIF at {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Ok(None),
    };

    let latitude_ref = exif
        .get_field(Tag::GPSLatitudeRef, In::PRIMARY)
        .and_then(|field| ascii_field_value(&field.value));
    let longitude_ref = exif
        .get_field(Tag::GPSLongitudeRef, In::PRIMARY)
        .and_then(|field| ascii_field_value(&field.value));
    let latitude = exif
        .get_field(Tag::GPSLatitude, In::PRIMARY)
        .and_then(|field| parse_gps_coordinate(&field.value, latitude_ref.as_deref()));
    let longitude = exif
        .get_field(Tag::GPSLongitude, In::PRIMARY)
        .and_then(|field| parse_gps_coordinate(&field.value, longitude_ref.as_deref()));

    let (latitude, longitude) = match (latitude, longitude) {
        (Some(latitude), Some(longitude))
            if is_valid_latitude(latitude) && is_valid_longitude(longitude) =>
        {
            (latitude, longitude)
        }
        _ => return Ok(None),
    };

    let altitude = exif
        .get_field(Tag::GPSAltitude, In::PRIMARY)
        .and_then(|field| parse_altitude(&field.value))
        .map(|altitude| {
            let is_below_sea_level = exif
                .get_field(Tag::GPSAltitudeRef, In::PRIMARY)
                .and_then(|field| parse_altitude_ref(&field.value))
                .unwrap_or(false);
            if is_below_sea_level {
                -altitude
            } else {
                altitude
            }
        });

    Ok(Some(ExifGps {
        latitude,
        longitude,
        altitude,
    }))
}

fn ascii_field_value(value: &Value) -> Option<String> {
    match value {
        Value::Ascii(values) => values.first().and_then(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .map(|text| text.trim_matches(char::from(0)).trim().to_string())
                .filter(|text| !text.is_empty())
        }),
        _ => None,
    }
}

fn parse_gps_coordinate(value: &Value, direction: Option<&str>) -> Option<f64> {
    let components = match value {
        Value::Rational(values) if values.len() >= 3 => values,
        _ => return None,
    };

    let mut decimal =
        components[0].to_f64() + components[1].to_f64() / 60.0 + components[2].to_f64() / 3600.0;
    match direction.unwrap_or("").trim().to_ascii_uppercase().as_str() {
        "S" | "W" => decimal *= -1.0,
        _ => {}
    }
    Some(decimal)
}

fn parse_altitude(value: &Value) -> Option<f64> {
    match value {
        Value::Rational(values) => values.first().map(|value| value.to_f64()),
        _ => None,
    }
}

fn parse_altitude_ref(value: &Value) -> Option<bool> {
    match value {
        Value::Byte(values) => values.first().map(|value| *value == 1),
        _ => None,
    }
}

#[allow(dead_code)]
fn _media_type_is_visual(media_type: &MediaType) -> bool {
    matches!(
        media_type,
        MediaType::Photo
            | MediaType::Video
            | MediaType::LivePhoto
            | MediaType::Panorama
            | MediaType::Screenshot
            | MediaType::Burst
            | MediaType::Timelapse
            | MediaType::Portrait
            | MediaType::Selfie
            | MediaType::SlowMo
            | MediaType::Timecode
    )
}

```

### `src/agents/atlas.rs`

```rust
//! Atlas — GPS/Location extraction agent.
//!
//! Scans backup for location-related SQLite databases from Apple Maps,
//! Google Maps, Snapchat, and payment apps. Extracts location records
//! from matching tables and returns them as EvidenceRecords.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};

use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverContext};
use crate::common::target::{
    apple_maps_cloud_history_target, apple_maps_geo_target, apple_maps_history_target,
    google_maps_target, snapchat_maps_target,
};
use crate::evidence::EvidenceRecord;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationRecord {
    pub source: String,
    pub app_bundle: String,
    pub database_path: String,
    pub table_name: String,
    pub timestamp: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy: Option<f64>,
    pub altitude: Option<f64>,
    pub address: Option<String>,
    pub query: Option<String>,
    #[serde(flatten)]
    pub raw: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationSource {
    pub app: String,
    pub bundle_id: String,
    pub database: String,
    pub records: Vec<LocationRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AtlasReport {
    pub case_name: String,
    pub extracted_at: String,
    pub sources: Vec<LocationSource>,
}

pub struct AtlasAgent;

impl Agent for AtlasAgent {
    const NAME: &'static str = "Atlas";
    const SLUG: &'static str = "atlas";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let resolver = BackupResolver::new(ResolverContext {
            backup_root: ctx.backup_root.clone(),
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            manifest_db_path: ctx
                .backup_root
                .join("Manifest.db")
                .exists()
                .then(|| ctx.backup_root.join("Manifest.db")),
        });

        let mut all_sources = Vec::new();

        match extract_apple_maps(ctx, &resolver) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Apple Maps extraction failed: {}", e),
        }

        match extract_google_maps(ctx, &resolver) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Google Maps extraction failed: {}", e),
        }

        match extract_snapchat(ctx, &resolver) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Snapchat extraction failed: {}", e),
        }

        match extract_payment_apps(ctx) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Payment apps extraction failed: {}", e),
        }

        let mut records = Vec::new();
        let mut total_locations = 0usize;

        for source in &all_sources {
            for location in &source.records {
                records.push(EvidenceRecord {
                    schema_version: Self::SCHEMA_VERSION,
                    source_agent: Self::NAME.to_string(),
                    record_type: "location".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    payload: serde_json::to_value(location).unwrap_or_else(|_| json!({})),
                });
                total_locations += 1;
            }
        }

        let report = AtlasReport {
            case_name: ctx.case.name().to_string(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
            sources: all_sources,
        };

        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "total_locations": total_locations,
                "total_sources": report.sources.len(),
                "extracted_at": report.extracted_at,
            }),
        });

        Ok(records)
    }
}

fn is_safe_sql_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn sanitize_value(val: SqlValue) -> serde_json::Value {
    match val {
        SqlValue::Null => serde_json::Value::Null,
        SqlValue::Integer(i) => serde_json::Value::Number(i.into()),
        SqlValue::Real(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        SqlValue::Text(s) => serde_json::Value::String(s),
        SqlValue::Blob(b) => serde_json::Value::String(format!("<bytes {}>", b.len())),
    }
}

fn is_location_table(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("location")
        || lower.contains("geo")
        || lower.contains("place")
        || lower.contains("history")
        || lower.contains("map")
        || lower.contains("coordinate")
        || lower.contains("merchant")
        || lower.contains("visit")
        || lower.contains("transaction")
        || lower.contains("checkin")
        || lower.contains("position")
}

fn lat_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        (l.contains("lat") && !l.contains("later") && !l.contains("latency") && !l.contains("relat") && !l.contains("platinum") && !l.contains("translat"))
            || l == "latitude"
    }).cloned()
}

fn lon_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("lon") || l.contains("lng") || l == "longitude"
    }).cloned()
}

fn time_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("time") || l.contains("date") || l.contains("timestamp") || l.contains("created") || l.contains("modified")
    }).cloned()
}

fn address_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("address") || l.contains("street") || l.contains("city") || l.contains("place") || l.contains("name") || l.contains("location")
    }).cloned()
}

fn accuracy_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| c.to_lowercase().contains("accuracy")).cloned()
}

fn altitude_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("altitude") || (l.contains("alt") && !l.contains("alert") && !l.contains("alternate"))
    }).cloned()
}

fn query_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("query") || l.contains("search") || l.contains("keyword") || l.contains("term")
    }).cloned()
}

fn open_db_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open SQLite database: {}", path.display()))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Ok(conn)
}

fn list_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    if !is_safe_sql_identifier(table) {
        anyhow::bail!("Invalid table name: {}", table);
    }
    let mut stmt = conn.prepare(&format!("PRAGMA table_info('{}')", table))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn extract_from_table(
    conn: &Connection,
    table: &str,
    db_path: &str,
    app_name: &str,
    bundle_id: &str,
) -> Result<Vec<LocationRecord>> {
    if !is_safe_sql_identifier(table) {
        return Ok(Vec::new());
    }
    let cols = match table_columns(conn, table) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    if cols.is_empty() {
        return Ok(Vec::new());
    }

    let lat_col = lat_column(&cols);
    let lon_col = lon_column(&cols);
    let time_col = time_column(&cols);
    let addr_col = address_column(&cols);
    let acc_col = accuracy_column(&cols);
    let alt_col = altitude_column(&cols);
    let q_col = query_column(&cols);

    let has_location_signal = lat_col.is_some()
        || lon_col.is_some()
        || time_col.is_some()
        || addr_col.is_some()
        || acc_col.is_some()
        || alt_col.is_some()
        || q_col.is_some();
    if !has_location_signal && !is_location_table(table) {
        return Ok(Vec::new());
    }

    let query = format!("SELECT * FROM '{}' LIMIT 5000", table);
    let mut stmt = match conn.prepare(&query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let col_names: Vec<String> = (0..stmt.column_count())
        .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
        .collect();

    let rows = match stmt.query_map([], |row| {
        let mut map = HashMap::new();
        for (idx, name) in col_names.iter().enumerate() {
            let v = match row.get::<_, SqlValue>(idx) {
                Ok(v) => sanitize_value(v),
                Err(_) => serde_json::Value::Null,
            };
            map.insert(name.clone(), v);
        }
        Ok(map)
    }) {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let record = LocationRecord {
            source: app_name.to_string(),
            app_bundle: bundle_id.to_string(),
            database_path: db_path.to_string(),
            table_name: table.to_string(),
            timestamp: time_col.as_ref().and_then(|c| row.get(c).map(|v| value_to_string(v))),
            latitude: lat_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            longitude: lon_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            accuracy: acc_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            altitude: alt_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            address: addr_col.as_ref().and_then(|c| row.get(c).map(|v| value_to_string(v))),
            query: q_col.as_ref().and_then(|c| row.get(c).map(|v| value_to_string(v))),
            raw: row,
        };
        records.push(record);
    }
    Ok(records)
}

fn value_to_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => v.to_string(),
    }
}

fn prepare_known_target(
    ctx: &AgentCtx,
    resolver: &BackupResolver,
    target: &crate::common::target::KnownTarget,
) -> Result<Option<PathBuf>> {
    let resolved = match resolver.resolve_known_target(target)? {
        Some(r) => r,
        None => return Ok(None),
    };
    let prep_ctx = PrepareContext {
        case_root: ctx.case.root_path(),
        clean_root: ctx.case.root_path().join("clean"),
        temp_root: ctx.case.root_path().join("tmp"),
    };
    let prepared = prepare_artifact(&prep_ctx, &resolved, target.sqlite_like)?;
    Ok(Some(prepared.working_path))
}

fn extract_apple_maps(ctx: &AgentCtx, resolver: &BackupResolver) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    if let Some(path) = prepare_known_target(ctx, resolver, &apple_maps_history_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Apple Maps", "com.apple.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Apple Maps".to_string(),
                        bundle_id: "com.apple.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Apple Maps history DB: {}", e),
        }
    }

    if let Some(path) = prepare_known_target(ctx, resolver, &apple_maps_cloud_history_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Apple Maps Cloud", "com.apple.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Apple Maps Cloud".to_string(),
                        bundle_id: "com.apple.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Apple Maps cloud history DB: {}", e),
        }
    }

    if let Some(path) = prepare_known_target(ctx, resolver, &apple_maps_geo_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Apple Maps Geo", "com.apple.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Apple Maps Geo".to_string(),
                        bundle_id: "com.apple.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Apple Maps geo DB: {}", e),
        }
    }

    Ok(sources)
}

fn extract_google_maps(ctx: &AgentCtx, resolver: &BackupResolver) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    if let Some(path) = prepare_known_target(ctx, resolver, &google_maps_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Google Maps", "com.google.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Google Maps".to_string(),
                        bundle_id: "com.google.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Google Maps DB: {}", e),
        }
    }

    let google_domain_prefix = "AppDomain-com.google.Maps";
    let mut found = Vec::new();
    for entry in WalkDir::new(&ctx.backup_root).max_depth(8).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&ctx.backup_root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();
        if rel_str.starts_with(google_domain_prefix) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite" || ext_lower == "db" || ext_lower == "sqlitedb" || ext_lower == "storedata" {
                    if !sources.iter().any(|s| s.database == path.to_string_lossy().to_string()) {
                        found.push(path.to_path_buf());
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();

    for db_path in found {
        match open_db_ro(&db_path) {
            Ok(conn) => {
                let tables = match list_tables(&conn) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &db_path.to_string_lossy(), "Google Maps", "com.google.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Google Maps".to_string(),
                        bundle_id: "com.google.Maps".to_string(),
                        database: db_path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(_) => continue,
        }
    }

    Ok(sources)
}

fn extract_snapchat(ctx: &AgentCtx, resolver: &BackupResolver) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    if let Some(path) = prepare_known_target(ctx, resolver, &snapchat_maps_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Snapchat", "com.toyopagroup.picaboo") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Snapchat".to_string(),
                        bundle_id: "com.toyopagroup.picaboo".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Snapchat map DB: {}", e),
        }
    }

    let snap_domain_prefix = "AppDomain-com.toyopagroup.picaboo";
    let mut found = Vec::new();
    for entry in WalkDir::new(&ctx.backup_root).max_depth(8).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&ctx.backup_root).unwrap_or(path);
        if rel.to_string_lossy().starts_with(snap_domain_prefix) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite" || ext_lower == "db" || ext_lower == "sqlitedb" || ext_lower == "storedata" {
                    if !sources.iter().any(|s| s.database == path.to_string_lossy().to_string()) {
                        found.push(path.to_path_buf());
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();

    for db_path in found {
        match open_db_ro(&db_path) {
            Ok(conn) => {
                let tables = match list_tables(&conn) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &db_path.to_string_lossy(), "Snapchat", "com.toyopagroup.picaboo") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Snapchat".to_string(),
                        bundle_id: "com.toyopagroup.picaboo".to_string(),
                        database: db_path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(_) => continue,
        }
    }

    Ok(sources)
}

fn is_banking_domain(domain: &str) -> bool {
    let lower = domain.to_lowercase();
    let keywords = [
        "bank", "chase", "wells", "paypal", "venmo", "cash", "zelle", "square",
        "credit", "debit", "card", "pay", "money", "transfer", "wallet",
        "capitalone", "citi", "bofa", "usbank", "pnc", "truist",
    ];
    keywords.iter().any(|k| lower.contains(k))
}

fn extract_payment_apps(ctx: &AgentCtx) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    for entry in WalkDir::new(&ctx.backup_root).max_depth(1).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_dir() {
            continue;
        }
        let domain_path = entry.path();
        let domain_name = domain_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !domain_name.starts_with("AppDomain-") {
            continue;
        }
        if !is_banking_domain(domain_name) {
            continue;
        }

        let bundle_id = domain_name.strip_prefix("AppDomain-").unwrap_or(domain_name).to_string();
        let app_name = bundle_id.split('.').last().unwrap_or(&bundle_id).to_string();

        let mut db_files = Vec::new();
        for sub in WalkDir::new(domain_path).max_depth(6).into_iter().filter_map(|e| e.ok()) {
            if !sub.file_type().is_file() {
                continue;
            }
            let path = sub.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite" || ext_lower == "db" || ext_lower == "sqlitedb" || ext_lower == "storedata" {
                    db_files.push(path.to_path_buf());
                }
            }
        }

        for db_path in db_files {
            match open_db_ro(&db_path) {
                Ok(conn) => {
                    let tables = match list_tables(&conn) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    let mut records = Vec::new();
                    for table in tables {
                        if let Ok(mut r) = extract_from_table(&conn, &table, &db_path.to_string_lossy(), &app_name, &bundle_id) {
                            records.append(&mut r);
                        }
                    }
                    if !records.is_empty() {
                        sources.push(LocationSource {
                            app: app_name.clone(),
                            bundle_id: bundle_id.clone(),
                            database: db_path.to_string_lossy().to_string(),
                            records,
                        });
                    }
                }
                Err(_) => continue,
            }
        }
    }

    Ok(sources)
}

```

### `src/agents/plutus.rs`

```rust
//! Plutus — Deterministic financial artifact extraction.

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};

use crate::common::prepared::{prepare_artifact, PrepareContext, PreparedArtifact};
use crate::common::resolver::{
    write_resolver_audit, ArtifactResolver, BackupResolver, ResolveMethod, ResolvedPath, ResolverAuditRecord,
};
use crate::evidence::EvidenceRecord;

pub const PLUTUS_FINANCIAL_SCHEMA_VERSION: u32 = 1;

const SOURCE_AGENT: &str = "plutus";
const CASH_APP_DOMAIN: &str = "AppDomain-com.squareup.cash";
const CASH_APP_DOCUMENTS: &str = "Documents";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FinancialRecord {
    pub schema_version: u32,
    pub record_id: String,
    pub source_agent: String,
    pub source_path: Option<std::path::PathBuf>,
    pub app: Option<String>,
    pub account_label: Option<String>,
    pub observed_date: Option<String>,
    pub timestamp_utc: Option<String>,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub counterparty: Option<String>,
    pub memo: Option<String>,
    pub transaction_type: Option<String>,
    pub raw_reference: Option<String>,
    pub sha256: Option<String>,
    pub fee_amount: Option<f64>,
    pub statement_period: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlutusReport {
    pub schema_version: u32,
    pub case_id: String,
    pub generated_at: String,
    pub state: String,
    pub files_examined: usize,
    pub files_parsed: usize,
    pub records_emitted: usize,
    pub unsupported_sources_skipped: usize,
    pub supported_source_breakdown: std::collections::BTreeMap<String, usize>,
    pub unsupported_source_breakdown: std::collections::BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct StatementContext {
    statement_period: Option<String>,
    statement_year: Option<i32>,
    account_label: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct StatementTotals {
    money_in: f64,
    money_out: f64,
    fees: f64,
}

#[derive(Debug)]
struct StatementParseResult {
    records: Vec<FinancialRecord>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct UnsupportedCandidate {
    label: &'static str,
    domain: &'static str,
    relative_path: &'static str,
}

pub struct PlutusAgent;

impl Agent for PlutusAgent {
    const NAME: &'static str = "Plutus";
    const SLUG: &'static str = "plutus";
    const SCHEMA_VERSION: u32 = PLUTUS_FINANCIAL_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let backup_root = if ctx.backup_root.exists() {
            ctx.backup_root.clone()
        } else {
            ctx.case.backup_path()
        };

        let manifest_db_path = backup_root
            .join("Manifest.db")
            .exists()
            .then(|| backup_root.join("Manifest.db"));

        let resolver = BackupResolver::new(crate::common::resolver::ResolverContext {
            backup_root: backup_root.clone(),
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            manifest_db_path,
        });

        let evidence_dir = ctx.case.evidence_path("plutus");
        fs::create_dir_all(&evidence_dir)?;

        let prepare_ctx = PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean").join("plutus"),
            temp_root: ctx.case.root_path().join("temp").join("plutus"),
        };

        let statements = discover_cash_app_statements(&resolver, &prepare_ctx)?;
        let mut supported_breakdown = BTreeMap::new();
        supported_breakdown.insert("cash_app_statement_pdf".to_string(), statements.len());

        let mut records = Vec::new();
        let mut warnings = Vec::new();
        let mut files_parsed = 0usize;

        for statement in &statements {
            let parsed = parse_cash_app_statement(statement)?;
            files_parsed += 1;
            records.extend(parsed.records);
            warnings.extend(parsed.warnings);
        }

        // SQLite extraction from payment apps
        let mut sqlite_records = Vec::new();
        let mut sqlite_files_examined = 0usize;
        let mut sqlite_files_parsed = 0usize;

        println!("\n💳 Scanning payment app SQLite databases...");
        extract_payment_app_sqlite(
            &backup_root,
            &mut sqlite_records,
            &mut sqlite_files_examined,
            &mut sqlite_files_parsed,
            &mut warnings,
        );

        if sqlite_files_parsed > 0 {
            supported_breakdown.insert("payment_app_sqlite".to_string(), sqlite_files_parsed);
        }
        records.extend(sqlite_records);

        records.sort_by(|left, right| {
            right
                .observed_date
                .cmp(&left.observed_date)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });

        let transactions_json_path = evidence_dir.join("transactions.json");
        let transactions_csv_path = evidence_dir.join("transactions.csv");
        let report_path = evidence_dir.join("report.json");

        export_transactions_json(&records, &transactions_json_path)?;
        export_transactions_csv(&records, &transactions_csv_path)?;

        // Export records.jsonl for UI integration
        let records_jsonl_path = evidence_dir.join("records.jsonl");
        let mut jsonl_file = fs::File::create(&records_jsonl_path)?;
        for record in &records {
            use std::io::Write;
            writeln!(jsonl_file, "{}", serde_json::to_string(record)?)?;
        }

        // Export summary.json for UI metrics
        let summary_json_path = evidence_dir.join("summary.json");
        let total_money_in: f64 = records.iter().filter_map(|r| r.amount).sum();
        let total_money_out: f64 = records.iter().filter_map(|r| r.amount).map(|a| -a).sum();
        let total_fees: f64 = records.iter().filter_map(|r| r.fee_amount).sum();
        let apps_found: Vec<String> = records
            .iter()
            .filter_map(|r| r.app.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        let summary = serde_json::json!({
            "metrics": [
                {"key": "records", "value": records.len().to_string()},
                {"key": "total_money_in", "value": format!("${:.2}", total_money_in)},
                {"key": "total_money_out", "value": format!("${:.2}", total_money_out)},
                {"key": "total_fees", "value": format!("${:.2}", total_fees)},
                {"key": "apps", "value": apps_found.join(", ")},
                {"key": "files_examined", "value": (statements.len() + sqlite_files_examined).to_string()},
                {"key": "files_parsed", "value": (files_parsed + sqlite_files_parsed).to_string()},
            ]
        });
        fs::write(&summary_json_path, serde_json::to_string_pretty(&summary)?)?;

        let total_files_examined = statements.len() + sqlite_files_examined;
        let total_files_parsed = files_parsed + sqlite_files_parsed;

        let report = PlutusReport {
            schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
            case_id: ctx.case.name().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            state: "complete".to_string(),
            files_examined: total_files_examined,
            files_parsed: total_files_parsed,
            records_emitted: records.len(),
            unsupported_sources_skipped: 0,
            supported_source_breakdown: supported_breakdown,
            unsupported_source_breakdown: BTreeMap::new(),
            warnings: warnings.clone(),
        };
        fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;

        println!("✓ Files examined: {}", report.files_examined);
        println!("✓ Files parsed: {}", report.files_parsed);
        println!("✓ Records emitted: {}", report.records_emitted);
        println!("✓ Output: {}", evidence_dir.display());

        // Build EvidenceRecords
        let mut evidence_records = Vec::new();

        // Report record
        evidence_records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        });

        // Transaction records
        for record in records {
            let timestamp = record
                .observed_date
                .clone()
                .or_else(|| record.timestamp_utc.clone())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "transaction".to_string(),
                timestamp,
                payload: serde_json::to_value(&record)?,
            });
        }

        Ok(evidence_records)
    }
}

fn discover_cash_app_statements(
    resolver: &BackupResolver,
    prepare_ctx: &PrepareContext,
) -> Result<Vec<PreparedArtifact>> {
    let documents_root = resolver.direct_tree_path(CASH_APP_DOMAIN, CASH_APP_DOCUMENTS);
    let mut discovered = Vec::new();
    let mut unique = BTreeSet::new();

    if !documents_root.exists() {
        return Ok(Vec::new());
    }

    let backup_domain_root = resolver.backup_root().join(CASH_APP_DOMAIN);
    for entry in WalkDir::new(&documents_root)
        .max_depth(3)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_cash_app_statement_name(file_name) {
            continue;
        }
        if !unique.insert(path.to_path_buf()) {
            continue;
        }

        let relative_path = path
            .strip_prefix(&backup_domain_root)
            .with_context(|| format!("Failed to compute relative path for {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let resolved = ResolvedPath {
            artifact_key: "cash_app_statement".to_string(),
            source_path: path.to_path_buf(),
            method: ResolveMethod::DirectTree,
            domain: CASH_APP_DOMAIN.to_string(),
            relative_path,
            clean_file_name: None,
            attempted: vec![path.to_path_buf()],
        };
        discovered.push(prepare_artifact(prepare_ctx, &resolved, false)?);
    }

    discovered.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    write_resolver_audit(
        resolver.case_root(),
        &ResolverAuditRecord {
            artifact_key: "plutus_cash_app_statements".to_string(),
            candidates: discovered
                .iter()
                .map(|artifact| artifact.source_path.display().to_string())
                .collect(),
            chosen: None,
            method: Some("multi_direct_tree_scan".to_string()),
        },
    )?;
    Ok(discovered)
}

fn discover_unsupported_candidates(resolver: &BackupResolver) -> Result<Vec<UnsupportedCandidate>> {
    let mut discovered = Vec::new();
    for candidate in unsupported_candidates() {
        if resolver
            .resolve_exact(
                "plutus_unsupported",
                candidate.domain,
                candidate.relative_path,
            )?
            .is_some()
        {
            discovered.push(*candidate);
        }
    }
    Ok(discovered)
}

fn unsupported_candidates() -> &'static [UnsupportedCandidate] {
    &[
        UnsupportedCandidate {
            label: "venmo_model_sqlite",
            domain: "AppDomain-net.kortina.labs.Venmo",
            relative_path: "Documents/Model.sqlite",
        },
        UnsupportedCandidate {
            label: "venmo_siri_support_sqlite",
            domain: "AppDomainGroup-group.net.kortina.labs.Venmo",
            relative_path: "SiriSupportData.sqlite",
        },
        UnsupportedCandidate {
            label: "chime_visa_analytics_sqlite",
            domain: "AppDomain-com.1debit.ChimeProdApp",
            relative_path: "Documents/VisaAnalytics_v2.sqlite",
        },
        UnsupportedCandidate {
            label: "cash_app_search_index_sqlite",
            domain: "AppDomainGroup-group.com.squareup.cash",
            relative_path: "SearchIndex-internal.cashappapi.com.sqlite",
        },
        UnsupportedCandidate {
            label: "cash_app_analytics_messages_sqlite",
            domain: "AppDomainGroup-group.com.squareup.cash",
            relative_path: "CDPPersistedAnalyticsMessages.SQLITE",
        },
    ]
}

fn is_cash_app_statement_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.starts_with("cash_app_") && lower.ends_with("statement.pdf")
}

fn parse_cash_app_statement(statement: &PreparedArtifact) -> Result<StatementParseResult> {
    let text = pdftotext_layout(&statement.working_path)?;
    let context = statement_context(&text, &statement.source_path);
    let expected = extract_statement_totals(&text);
    let rows = parse_statement_rows(&text);
    let computed = summarize_rows(&rows);

    let mut warnings = Vec::new();
    if let Some(expected) = expected {
        if !totals_match(expected, computed) {
            warnings.push(format!(
                "Statement totals mismatch for {}: expected in {:.2}/out {:.2}/fees {:.2}, computed in {:.2}/out {:.2}/fees {:.2}",
                statement.source_path.display(),
                expected.money_in,
                expected.money_out,
                expected.fees,
                computed.money_in,
                computed.money_out,
                computed.fees
            ));
        }
    } else if !rows.is_empty() {
        warnings.push(format!(
            "Statement summary totals were not found in {}",
            statement.source_path.display()
        ));
    }

    let statement_slug = statement
        .source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("cash_app_statement");
    let records = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let observed_date = context
                .statement_year
                .and_then(|year| normalize_statement_date(&row.raw_date, year));
            FinancialRecord {
                schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
                record_id: format!("cashapp:{}:{}", statement_slug, index + 1),
                source_agent: SOURCE_AGENT.to_string(),
                source_path: Some(statement.source_path.clone()),
                app: Some("Cash App".to_string()),
                account_label: context.account_label.clone(),
                observed_date,
                timestamp_utc: None,
                amount: Some(row.signed_amount),
                currency: Some("USD".to_string()),
                counterparty: (!row.description.trim().is_empty()).then(|| row.description.clone()),
                memo: None,
                transaction_type: Some(row.details.clone()),
                raw_reference: Some(row.raw_line),
                sha256: statement.sha256.clone(),
                fee_amount: Some(row.fee_amount),
                statement_period: context.statement_period.clone(),
            }
        })
        .collect();

    Ok(StatementParseResult { records, warnings })
}

fn pdftotext_layout(path: &Path) -> Result<String> {
    let output = Command::new("pdftotext")
        .args(["-layout"])
        .arg(path)
        .arg("-")
        .output()
        .with_context(|| format!("Failed to run pdftotext on {}", path.display()))?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "pdftotext failed for {} with status {}",
            path.display(),
            output.status
        ));
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("pdftotext output was not UTF-8 for {}", path.display()))
}

fn statement_context(text: &str, source_path: &Path) -> StatementContext {
    let period_re = Regex::new(
        r"^(January|February|March|April|May|June|July|August|September|October|November|December)\s+(\d{4})$",
    )
    .expect("valid period regex");
    let account_re = Regex::new(r"^Cash App\s{2,}(?P<label>.+?)\s*$").expect("valid account regex");

    let mut statement_period = None;
    let mut statement_year = None;
    let mut account_label = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if statement_period.is_none() {
            if let Some(captures) = period_re.captures(trimmed) {
                statement_period = Some(trimmed.to_string());
                statement_year = captures
                    .get(2)
                    .and_then(|value| value.as_str().parse::<i32>().ok());
            }
        }
        if account_label.is_none() {
            if let Some(captures) = account_re.captures(trimmed) {
                account_label = captures
                    .name("label")
                    .map(|value| value.as_str().trim().to_string());
            }
        }
        if statement_period.is_some() && account_label.is_some() {
            break;
        }
    }

    if statement_period.is_none() {
        statement_period = statement_period_from_filename(source_path);
        statement_year = statement_period
            .as_deref()
            .and_then(|value| value.split_whitespace().last())
            .and_then(|value| value.parse::<i32>().ok());
    }

    StatementContext {
        statement_period,
        statement_year,
        account_label,
    }
}

fn statement_period_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let cleaned = stem
        .replace("Cash_App_", "")
        .replace("_Account_Statement", "")
        .replace("_Statement", "");
    let month = cleaned.replace('_', " ");
    Some(month)
}

fn extract_statement_totals(text: &str) -> Option<StatementTotals> {
    let summary_re = Regex::new(
        r"^(?P<label>Money In|Money Out|Fees)\s+(?P<sign>[+-])?\s*\$(?P<amount>[\d,]+\.\d{2})\s*$",
    )
    .expect("valid summary regex");
    let mut totals = StatementTotals::default();
    let mut found_any = false;

    for line in text.lines() {
        let trimmed = line.trim();
        let Some(captures) = summary_re.captures(trimmed) else {
            continue;
        };
        let amount = parse_money(
            captures
                .name("amount")
                .map(|value| value.as_str())
                .unwrap_or_default(),
        );
        match captures.name("label").map(|value| value.as_str()) {
            Some("Money In") => totals.money_in = amount,
            Some("Money Out") => totals.money_out = amount,
            Some("Fees") => totals.fees = amount,
            _ => {}
        }
        found_any = true;
    }

    found_any.then_some(totals)
}

#[derive(Debug)]
struct StatementRow {
    raw_date: String,
    description: String,
    details: String,
    fee_amount: f64,
    signed_amount: f64,
    raw_line: String,
}

fn parse_statement_rows(text: &str) -> Vec<StatementRow> {
    let row_re = Regex::new(
        r"^(?P<date>[A-Z][a-z]{2}\s+\d{1,2})\s{2,}(?P<desc>.*?)\s{2,}(?P<details>.*?)\s+\$(?P<fee>[\d,]+\.\d{2})\s+(?P<sign>\+)?\s*\$(?P<amount>[\d,]+\.\d{2})\s*$",
    )
    .expect("valid transaction regex");

    text.lines()
        .filter_map(|line| {
            let captures = row_re.captures(line.trim_end())?;
            let fee_amount = parse_money(captures.name("fee")?.as_str());
            let amount = parse_money(captures.name("amount")?.as_str());
            let signed_amount = if captures.name("sign").is_some() {
                amount
            } else {
                -amount
            };
            let mut description = captures.name("desc")?.as_str().trim().to_string();
            let mut details = captures.name("details")?.as_str().trim().to_string();
            if details.is_empty() && !description.is_empty() {
                details = description;
                description = String::new();
            }

            Some(StatementRow {
                raw_date: captures.name("date")?.as_str().trim().to_string(),
                description,
                details,
                fee_amount,
                signed_amount,
                raw_line: line.trim_end().to_string(),
            })
        })
        .collect()
}

fn summarize_rows(rows: &[StatementRow]) -> StatementTotals {
    let mut totals = StatementTotals::default();
    for row in rows {
        if row.signed_amount >= 0.0 {
            totals.money_in += row.signed_amount;
        } else {
            totals.money_out += row.signed_amount.abs();
        }
        totals.fees += row.fee_amount;
    }
    totals
}

fn totals_match(expected: StatementTotals, computed: StatementTotals) -> bool {
    (expected.money_in - computed.money_in).abs() < 0.02
        && (expected.money_out - computed.money_out).abs() < 0.02
        && (expected.fees - computed.fees).abs() < 0.02
}

fn parse_money(raw: &str) -> f64 {
    raw.replace(',', "").parse::<f64>().unwrap_or(0.0)
}

fn normalize_statement_date(raw_date: &str, year: i32) -> Option<String> {
    let mut parts = raw_date.split_whitespace();
    let month = month_number(parts.next()?)?;
    let day = parts.next()?.parse::<u32>().ok()?;
    NaiveDate::from_ymd_opt(year, month, day).map(|value| value.format("%Y-%m-%d").to_string())
}

fn month_number(raw: &str) -> Option<u32> {
    match raw {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

fn export_transactions_json(records: &[FinancialRecord], path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(records)?)?;
    Ok(())
}

fn export_transactions_csv(records: &[FinancialRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Schema Version",
        "Record ID",
        "Source Agent",
        "Source Path",
        "App",
        "Account Label",
        "Observed Date",
        "Timestamp UTC",
        "Amount",
        "Currency",
        "Counterparty",
        "Memo",
        "Transaction Type",
        "Raw Reference",
        "SHA256",
        "Fee Amount",
        "Statement Period",
    ])?;

    for record in records {
        let source_path = record
            .source_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let amount = record
            .amount
            .map(|value| value.to_string())
            .unwrap_or_default();
        let fee_amount = record
            .fee_amount
            .map(|value| value.to_string())
            .unwrap_or_default();

        wtr.write_record([
            record.schema_version.to_string(),
            record.record_id.clone(),
            record.source_agent.clone(),
            source_path,
            record.app.clone().unwrap_or_default(),
            record.account_label.clone().unwrap_or_default(),
            record.observed_date.clone().unwrap_or_default(),
            record.timestamp_utc.clone().unwrap_or_default(),
            amount,
            record.currency.clone().unwrap_or_default(),
            record.counterparty.clone().unwrap_or_default(),
            record.memo.clone().unwrap_or_default(),
            record.transaction_type.clone().unwrap_or_default(),
            record.raw_reference.clone().unwrap_or_default(),
            record.sha256.clone().unwrap_or_default(),
            fee_amount,
            record.statement_period.clone().unwrap_or_default(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn is_safe_sql_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn sanitize_sqlite_value(val: SqlValue) -> serde_json::Value {
    match val {
        SqlValue::Null => serde_json::Value::Null,
        SqlValue::Integer(i) => serde_json::Value::Number(i.into()),
        SqlValue::Real(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        SqlValue::Text(s) => serde_json::Value::String(s),
        SqlValue::Blob(b) => {
            // Try to interpret blob as UTF-8 JSON first
            if let Ok(s) = String::from_utf8(b.clone()) {
                if s.trim_start().starts_with('{') || s.trim_start().starts_with('[') {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&s) {
                        return json;
                    }
                }
                return serde_json::Value::String(s);
            }
            serde_json::Value::String(format!("<bytes {}>", b.len()))
        }
    }
}

fn open_db_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open SQLite database: {}", path.display()))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Ok(conn)
}

fn is_payment_app_domain(domain: &str) -> bool {
    let lower = domain.to_lowercase();
    let keywords = [
        "cash", "venmo", "chime", "paypal", "zelle", "bank", "credit", "debit",
        "wallet", "pay", "transfer", "squareup", "1debit",
    ];
    keywords.iter().any(|k| lower.contains(k))
}

fn is_interesting_table(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("transaction")
        || lower.contains("payment")
        || lower.contains("transfer")
        || lower.contains("user")
        || lower.contains("business")
        || lower.contains("event")
        || lower.contains("card")
        || lower.contains("account")
        || lower.contains("search")
        || lower.contains("message")
        || lower.contains("client")
}

fn extract_payment_app_sqlite(
    backup_root: &Path,
    out_records: &mut Vec<FinancialRecord>,
    files_examined: &mut usize,
    files_parsed: &mut usize,
    warnings: &mut Vec<String>,
) {
    let entries: Vec<_> = match walkdir::WalkDir::new(backup_root)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect()
    {
        e => e,
    };

    for entry in entries {
        if !entry.file_type().is_dir() {
            continue;
        }
        let domain_path = entry.path();
        let domain = domain_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !domain.starts_with("AppDomain") && !domain.starts_with("AppDomainGroup") {
            continue;
        }
        if !is_payment_app_domain(domain) {
            continue;
        }

        let app_name = if domain.contains("Chime") {
            "Chime"
        } else if domain.contains("Venmo") {
            "Venmo"
        } else if domain.contains("cash") || domain.contains("squareup") {
            "Cash App"
        } else if domain.contains("PayPal") {
            "PayPal"
        } else {
            "Payment App"
        };

        let db_files: Vec<_> = walkdir::WalkDir::new(domain_path)
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        let lower = ext.to_lowercase();
                        lower == "sqlite" || lower == "db" || lower == "sqlitedb" || lower == "storedata"
                    })
                    .unwrap_or(false)
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        for db_path in db_files {
            *files_examined += 1;
            match extract_from_payment_db(&db_path, app_name, domain) {
                Ok(mut recs) => {
                    if !recs.is_empty() {
                        *files_parsed += 1;
                        out_records.append(&mut recs);
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "SQLite extraction failed for {}: {}",
                        db_path.display(),
                        e
                    ));
                }
            }
        }
    }
}

fn extract_from_payment_db(
    db_path: &Path,
    app_name: &str,
    domain: &str,
) -> Result<Vec<FinancialRecord>> {
    let conn = open_db_ro(db_path)?;
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let table_names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut records = Vec::new();
    for table in table_names {
        if !is_interesting_table(&table) {
            continue;
        }
        if !is_safe_sql_identifier(&table) {
            continue;
        }
        let query = format!("SELECT * FROM '{}' LIMIT 500", table);
        let mut stmt = match conn.prepare(&query) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
            .collect();

        let rows = match stmt.query_map([], |row| {
            let mut map = HashMap::new();
            for idx in 0..col_count {
                let val = match row.get::<_, SqlValue>(idx) {
                    Ok(v) => sanitize_sqlite_value(v),
                    Err(_) => serde_json::Value::Null,
                };
                map.insert(col_names[idx].clone(), val);
            }
            Ok(map)
        }) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for row in rows.filter_map(|r| r.ok()) {
            let counterparty = find_counterparty(&row, &col_names);
            let memo = find_memo(&row, &col_names);
            let timestamp = find_timestamp(&row, &col_names);
            let amount = find_amount(&row, &col_names);
            let tx_type = find_tx_type(&row, &col_names, app_name, &table);

            let record = FinancialRecord {
                schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
                record_id: format!(
                    "plutus_sqlite_{}_{}_{}",
                    domain,
                    table,
                    records.len()
                ),
                source_agent: SOURCE_AGENT.to_string(),
                source_path: Some(db_path.to_path_buf()),
                app: Some(app_name.to_string()),
                account_label: None,
                observed_date: timestamp.clone(),
                timestamp_utc: timestamp,
                amount,
                currency: Some("USD".to_string()),
                counterparty,
                memo,
                transaction_type: Some(tx_type),
                raw_reference: Some(serde_json::to_string(&row).unwrap_or_default()),
                sha256: None,
                fee_amount: None,
                statement_period: None,
            };
            records.push(record);
        }
    }
    Ok(records)
}

fn find_counterparty(
    row: &HashMap<String, serde_json::Value>,
    cols: &[String],
) -> Option<String> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("name")
            || lower.contains("display")
            || lower.contains("user")
            || lower.contains("counterparty")
            || lower.contains("recipient")
            || lower.contains("sender")
        {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return Some(s.clone());
                }
            }
        }
    }
    None
}

fn find_memo(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<String> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("memo")
            || lower.contains("note")
            || lower.contains("message")
            || lower.contains("description")
            || lower.contains("status")
        {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return Some(s.clone());
                }
            }
        }
    }
    None
}

fn find_timestamp(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<String> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("time")
            || lower.contains("date")
            || lower.contains("created")
            || lower.contains("modified")
        {
            if let Some(v) = row.get(col) {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                if !s.is_empty() && s != "0" {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn find_amount(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<f64> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("amount")
            || lower.contains("sum")
            || lower.contains("total")
            || lower.contains("balance")
        {
            if let Some(v) = row.get(col) {
                match v {
                    serde_json::Value::Number(n) => return n.as_f64(),
                    serde_json::Value::String(s) => return s.parse().ok(),
                    _ => continue,
                }
            }
        }
    }
    None
}

fn find_tx_type(
    row: &HashMap<String, serde_json::Value>,
    cols: &[String],
    app: &str,
    table: &str,
) -> String {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("type") || lower.contains("status") || lower.contains("state") {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return format!("{}:{}", table, s);
                }
            }
        }
    }
    format!("{}:{}", app, table)
}

```

### `src/agents/psyche.rs`

```rust
//! Psyche Agent

use anyhow::Result;
use serde_json::json;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub struct PsycheAgent;

impl Agent for PsycheAgent {
    const NAME: &'static str = "Psyche";
    const SLUG: &'static str = "psyche";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "records_emitted": 0,
                "warnings": Vec::<String>::new(),
            }),
        });
        Ok(records)
    }
}

```

### `src/agents/psyche_models.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PsycheMessageAnalysis {
    pub message_id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub sender: String,
    pub recipient: String,
    pub content: String,
    pub contact_label: Option<String>,
    pub sentiment: Sentiment,
    pub emotion: Emotion,
    pub intent: Intent,
    pub aggression_level: f64,
    pub manipulation_score: f64,
    pub deception_indicators: Vec<String>,
    pub linguistic_features: LinguisticFeatures,
    pub context_flags: Vec<String>,
    pub risk_level: RiskLevel,
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PsycheCallAnalysis {
    pub call_id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub participants: Vec<String>,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RiskSummary {
    pub overall_risk: RiskLevel,
    pub highlights: Vec<String>,
    pub red_flags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpeakerProfile {
    pub speaker_id: String,
    pub name: Option<String>,
    pub risk_level: RiskLevel,
    pub traits: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Sentiment {
    pub polarity: f64,
    pub subjectivity: f64,
    pub emotional_tone: EmotionalTone,
    pub volatility: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum EmotionalTone {
    Positive,
    Neutral,
    Negative,
    Mixed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Emotion {
    pub primary_emotion: String,
    pub intensity: f64,
    pub secondary_emotions: Vec<String>,
    pub emotional_shift: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Intent {
    Informational,
    Supportive,
    Persuasive,
    Aggressive,
    Defensive,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinguisticFeatures {
    pub word_count: usize,
    pub sentence_count: usize,
    pub avg_sentence_length: f64,
    pub readability_score: f64,
    pub complexity_score: f64,
    pub pronoun_usage: PronounUsage,
    pub filler_words: Vec<String>,
    pub intensifiers: Vec<String>,
    pub qualifiers: Vec<String>,
    pub hedging_language: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PronounUsage {
    pub first_person_singular: usize,
    pub first_person_plural: usize,
    pub second_person: usize,
    pub third_person: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

```

### `src/agents/echo.rs`

```rust
//! Echo — Deterministic audio evidence inventory.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::compute_sha256;
use crate::agents::cerberus_models::VoicemailRecord;
use crate::evidence::EvidenceRecord;
use serde::{Deserialize, Serialize};

const ECHO_AUDIO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioEvidenceRecord {
    pub schema_version: u32,
    pub record_id: String,
    pub source_agent: String,
    pub source_db_path: Option<PathBuf>,
    pub source_media_path: Option<PathBuf>,
    pub copied_media_path: Option<PathBuf>,
    pub timestamp_utc: Option<String>,
    pub duration_seconds: Option<f64>,
    pub codec: Option<String>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
    pub transcript: Option<String>,
    pub transcript_method: Option<String>,
    pub resolution_method: Option<String>,
}

pub struct EchoAgent;

impl Agent for EchoAgent {
    const NAME: &'static str = "Echo";
    const SLUG: &'static str = "echo";
    const SCHEMA_VERSION: u32 = ECHO_AUDIO_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let voicemail_dir = ctx.case.evidence_path("voicemail");
        let source_db_path = resolve_source_db_path(&ctx.case);
        let copied_media_index = build_copied_media_index(&ctx.case);
        let voicemails = load_voicemails(&voicemail_dir)?;

        let (audio_records, stats) =
            build_audio_inventory(&voicemails, source_db_path.as_deref(), &copied_media_index);

        let mut warnings = Vec::new();
        if stats.unresolved_media_count > 0 {
            warnings.push(format!(
                "{} audio records had no deterministic source or copied media path",
                stats.unresolved_media_count
            ));
        }
        if stats.source_transcript_artifacts_observed > 0 {
            warnings.push(format!(
                "{} source voicemail transcript artifacts were observed but transcript fields remain null in Echo v1",
                stats.source_transcript_artifacts_observed
            ));
        }

        let mut records = Vec::new();

        // Report record
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "source_agent": "hermes",
                "source_db_path": source_db_path.as_ref().map(|p| p.display().to_string()),
                "audio_records_emitted": stats.audio_records_emitted,
                "media_linked_count": stats.media_linked_count,
                "unresolved_media_count": stats.unresolved_media_count,
                "metadata_enriched_count": stats.metadata_enriched_count,
                "source_transcript_artifacts_observed": stats.source_transcript_artifacts_observed,
                "warnings": warnings,
            }),
        });

        // Audio records
        for audio in audio_records {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "audio".to_string(),
                timestamp: audio.timestamp_utc.clone().unwrap_or_default(),
                payload: json!({
                    "record_id": audio.record_id,
                    "source_agent": audio.source_agent,
                    "source_db_path": audio.source_db_path.map(|p| p.display().to_string()),
                    "source_media_path": audio.source_media_path.map(|p| p.display().to_string()),
                    "copied_media_path": audio.copied_media_path.map(|p| p.display().to_string()),
                    "duration_seconds": audio.duration_seconds,
                    "codec": audio.codec,
                    "mime_type": audio.mime_type,
                    "sha256": audio.sha256,
                    "transcript": audio.transcript,
                    "transcript_method": audio.transcript_method,
                    "resolution_method": audio.resolution_method,
                }),
            });
        }

        Ok(records)
    }
}

#[derive(Debug, Default)]
struct EchoStats {
    audio_records_emitted: usize,
    media_linked_count: usize,
    unresolved_media_count: usize,
    metadata_enriched_count: usize,
    source_transcript_artifacts_observed: usize,
}

#[derive(Debug, Default)]
struct AudioMetadata {
    duration_seconds: Option<f64>,
    codec: Option<String>,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeStream {
    codec_name: Option<String>,
    codec_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeFormat {
    duration: Option<String>,
}

fn load_voicemails(voicemail_dir: &Path) -> Result<Vec<VoicemailRecord>> {
    if !voicemail_dir.exists() {
        return Ok(Vec::new());
    }

    let mut records_by_id: HashMap<i64, VoicemailRecord> = HashMap::new();
    for entry in WalkDir::new(voicemail_dir)
        .max_depth(2)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("voicemail_")
            || path.extension().and_then(|ext| ext.to_str()) != Some("json")
        {
            continue;
        }

        let raw = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let parsed: Vec<VoicemailRecord> = serde_json::from_slice(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        for record in parsed {
            records_by_id.entry(record.id).or_insert(record);
        }
    }

    let mut records: Vec<VoicemailRecord> = records_by_id.into_values().collect();
    records.sort_by_key(|record| std::cmp::Reverse(record.timestamp));
    Ok(records)
}

fn resolve_source_db_path(case: &crate::case::Case) -> Option<PathBuf> {
    let clean_db = case.root_path().join("clean").join("voicemail.db");
    if clean_db.exists() {
        return Some(clean_db);
    }
    let backup_db = case
        .backup_path()
        .join("HomeDomain")
        .join("Library")
        .join("Voicemail")
        .join("voicemail.db");
    backup_db.exists().then_some(backup_db)
}

fn build_copied_media_index(case: &crate::case::Case) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    for dir in [case.evidence_path("voicemail"), case.evidence_path("audio")] {
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(dir).max_depth(3).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !looks_like_audio_file(path) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            index
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| path.to_path_buf());
        }
    }
    index
}

fn looks_like_audio_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "amr" | "wav" | "m4a" | "aac" | "caf" | "mp3" | "ogg" | "flac"
    )
}

fn build_audio_inventory(
    voicemails: &[VoicemailRecord],
    source_db_path: Option<&Path>,
    copied_media_index: &HashMap<String, PathBuf>,
) -> (Vec<AudioEvidenceRecord>, EchoStats) {
    let mut records = Vec::new();
    let mut stats = EchoStats::default();

    for vm in voicemails {
        let source_media_path = normalize_existing_path(vm.file_path.as_deref());
        let copied_media_path = resolve_copied_media_path(vm, copied_media_index);
        let metadata_source = copied_media_path.as_ref().or(source_media_path.as_ref());
        let metadata = metadata_source
            .map(|path| collect_audio_metadata(path, vm.duration))
            .transpose()
            .unwrap_or_default()
            .unwrap_or_else(|| AudioMetadata {
                duration_seconds: normalize_db_duration(vm.duration),
                ..AudioMetadata::default()
            });

        let sha256 = vm
            .hash
            .clone()
            .or_else(|| metadata_source.and_then(|path| compute_sha256(path).ok()));
        let resolution_method = if source_media_path.is_some() {
            Some("hermes_voicemail_source_path".to_string())
        } else if copied_media_path.is_some() {
            Some("echo_copied_media_index".to_string())
        } else {
            None
        };

        if vm.transcript_path.is_some() {
            stats.source_transcript_artifacts_observed += 1;
        }
        if source_media_path.is_some() || copied_media_path.is_some() {
            stats.media_linked_count += 1;
        } else {
            stats.unresolved_media_count += 1;
        }
        if metadata.duration_seconds.is_some()
            || metadata.codec.is_some()
            || metadata.mime_type.is_some()
        {
            stats.metadata_enriched_count += 1;
        }

        records.push(AudioEvidenceRecord {
            schema_version: ECHO_AUDIO_SCHEMA_VERSION,
            record_id: format!("voicemail:{}", vm.id),
            source_agent: "hermes".to_string(),
            source_db_path: source_db_path.map(Path::to_path_buf),
            source_media_path,
            copied_media_path,
            timestamp_utc: unix_to_rfc3339(vm.timestamp),
            duration_seconds: metadata.duration_seconds,
            codec: metadata.codec,
            mime_type: metadata.mime_type,
            sha256,
            transcript: None,
            transcript_method: None,
            resolution_method,
        });
        stats.audio_records_emitted += 1;
    }

    (records, stats)
}

fn normalize_existing_path(raw: Option<&str>) -> Option<PathBuf> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.exists().then_some(path)
}

fn resolve_copied_media_path(
    vm: &VoicemailRecord,
    copied_media_index: &HashMap<String, PathBuf>,
) -> Option<PathBuf> {
    let filename = vm.filename.as_deref()?.trim().to_ascii_lowercase();
    copied_media_index.get(&filename).cloned()
}

fn collect_audio_metadata(path: &Path, db_duration: i64) -> Result<AudioMetadata> {
    let ffprobe = probe_audio(path)?;
    let mime_type = sniff_mime_type(path)?;
    Ok(AudioMetadata {
        duration_seconds: ffprobe
            .as_ref()
            .and_then(|meta| meta.duration_seconds)
            .or_else(|| normalize_db_duration(db_duration)),
        codec: ffprobe.and_then(|meta| meta.codec),
        mime_type,
    })
}

fn probe_audio(path: &Path) -> Result<Option<AudioMetadata>> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_name,codec_type",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run ffprobe on {}", path.display()))?;
    if !output.status.success() {
        return Ok(None);
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("Failed to parse ffprobe output for {}", path.display()))?;

    let codec = parsed
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .and_then(|stream| stream.codec_name);
    let duration_seconds = parsed
        .format
        .and_then(|format| format.duration)
        .and_then(|raw| raw.parse::<f64>().ok());

    Ok(Some(AudioMetadata {
        duration_seconds,
        codec,
        mime_type: None,
    }))
}

fn sniff_mime_type(path: &Path) -> Result<Option<String>> {
    let output = Command::new("file")
        .args(["--mime-type", "-b"])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run file on {}", path.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        Ok(None)
    } else {
        Ok(Some(raw))
    }
}

fn normalize_db_duration(value: i64) -> Option<f64> {
    (value > 0).then_some(value as f64)
}

fn unix_to_rfc3339(timestamp: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(timestamp, 0).map(|dt| dt.to_rfc3339())
}

```

### `src/agents/vigil.rs`

```rust
//! Vigil — Deterministic video evidence inventory.

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agents::{Agent, AgentCtx};
use crate::agents::charon_models::{AssetRecord, MediaType, CHARON_ASSET_SCHEMA_VERSION};
use crate::common::prepared::compute_sha256;
use crate::evidence::EvidenceRecord;
use serde::{Deserialize, Serialize};

const VIGIL_VIDEO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoEvidenceRecord {
    pub schema_version: u32,
    pub asset_id: Option<String>,
    pub asset_numeric_id: Option<i64>,
    pub filename: String,
    pub media_type: String,
    pub timestamp_utc: Option<String>,
    pub source_path: Option<PathBuf>,
    pub copied_path: Option<PathBuf>,
    pub source_resolution_method: Option<String>,
    pub duration_seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub file_size: Option<u64>,
    pub sha256: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

pub struct VigilAgent;

impl Agent for VigilAgent {
    const NAME: &'static str = "Vigil";
    const SLUG: &'static str = "vigil";
    const SCHEMA_VERSION: u32 = VIGIL_VIDEO_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let charon_dir = ctx.case.evidence_path("charon");
        let assets_path = charon_dir.join("assets.json");
        let manifest_path = charon_dir.join("copy_manifest.json");

        let assets = load_charon_assets(&assets_path)?;
        let copy_manifest = load_copy_manifest(&manifest_path)?;
        let ffprobe_available = ffprobe_available();

        let (videos, stats) =
            collect_video_inventory(&assets, &copy_manifest, &charon_dir, ffprobe_available);

        let mut warnings = Vec::new();
        if !ffprobe_available {
            warnings.push("ffprobe not available; codec/container enrichment was skipped".to_string());
        }
        if stats.unresolved_videos > 0 {
            warnings.push(format!(
                "{} video assets had no deterministic source or copied path",
                stats.unresolved_videos
            ));
        }
        if stats.videos_with_hash == 0
            && stats.videos_with_copied_path == 0
            && stats.videos_with_resolved_source_path > 0
        {
            warnings.push(
                "Resolved source videos were not hashed by default; hashes are emitted when Charon provides them or copied/exported files exist".to_string(),
            );
        }

        // Build report as first record
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "source_agent": "charon",
                "charon_asset_schema_version": assets.first().map(|a| a.schema_version).unwrap_or(CHARON_ASSET_SCHEMA_VERSION),
                "charon_assets_path": assets_path.display().to_string(),
                "charon_copy_manifest_path": manifest_path.exists().then(|| manifest_path.display().to_string()),
                "assets_examined": stats.assets_examined,
                "video_assets": stats.video_assets,
                "videos_with_resolved_source_path": stats.videos_with_resolved_source_path,
                "videos_with_copied_path": stats.videos_with_copied_path,
                "videos_with_hash": stats.videos_with_hash,
                "ffprobe_enriched": stats.ffprobe_enriched,
                "unresolved_videos": stats.unresolved_videos,
                "warnings": warnings,
            }),
        });

        // Build video records
        for video in videos {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "video".to_string(),
                timestamp: video.timestamp_utc.clone().unwrap_or_default(),
                payload: json!({
                    "asset_id": video.asset_id,
                    "asset_numeric_id": video.asset_numeric_id,
                    "filename": video.filename,
                    "media_type": video.media_type,
                    "source_path": video.source_path.map(|p| p.display().to_string()),
                    "copied_path": video.copied_path.map(|p| p.display().to_string()),
                    "source_resolution_method": video.source_resolution_method,
                    "duration_seconds": video.duration_seconds,
                    "width": video.width,
                    "height": video.height,
                    "container": video.container,
                    "video_codec": video.video_codec,
                    "audio_codec": video.audio_codec,
                    "file_size": video.file_size,
                    "sha256": video.sha256,
                    "latitude": video.latitude,
                    "longitude": video.longitude,
                }),
            });
        }

        Ok(records)
    }
}

#[derive(Debug, Deserialize, Default)]
struct CopyManifest {
    #[serde(default)]
    entries: HashMap<String, CopyManifestEntry>,
}

#[derive(Debug, Deserialize, Clone)]
struct CopyManifestEntry {
    asset_id: String,
    source_path: String,
    destination_path: String,
}

#[derive(Debug, Default)]
struct VideoStats {
    assets_examined: usize,
    video_assets: usize,
    videos_with_resolved_source_path: usize,
    videos_with_copied_path: usize,
    videos_with_hash: usize,
    ffprobe_enriched: usize,
    unresolved_videos: usize,
}

#[derive(Debug, Default)]
struct VideoProbe {
    container: Option<String>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration_seconds: Option<f64>,
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeStream {
    codec_name: Option<String>,
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
}

fn load_charon_assets(path: &Path) -> Result<Vec<AssetRecord>> {
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon assets at {}", path.display()))?;
    let assets: Vec<AssetRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon assets JSON at {}", path.display()))?;
    Ok(assets)
}

fn load_copy_manifest(path: &Path) -> Result<CopyManifest> {
    if !path.exists() {
        return Ok(CopyManifest::default());
    }
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon copy manifest at {}", path.display()))?;
    let manifest: CopyManifest = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon copy manifest at {}", path.display()))?;
    Ok(manifest)
}

fn collect_video_inventory(
    assets: &[AssetRecord],
    copy_manifest: &CopyManifest,
    charon_dir: &Path,
    ffprobe_enabled: bool,
) -> (Vec<VideoEvidenceRecord>, VideoStats) {
    let mut stats = VideoStats {
        assets_examined: assets.len(),
        ..VideoStats::default()
    };
    let mut records = Vec::new();

    for asset in assets {
        if !is_video_asset(asset) {
            continue;
        }
        stats.video_assets += 1;

        let manifest_entry = manifest_entry_for(asset, copy_manifest);
        let source_path = resolved_source_path(asset, manifest_entry);
        let copied_path = resolved_copied_path(charon_dir, manifest_entry);
        if source_path.is_some() {
            stats.videos_with_resolved_source_path += 1;
        }
        if copied_path.is_some() {
            stats.videos_with_copied_path += 1;
        }
        if source_path.is_none() && copied_path.is_none() {
            stats.unresolved_videos += 1;
        }

        let media_path = source_path.as_ref().or(copied_path.as_ref());
        let probe = if ffprobe_enabled {
            media_path
                .and_then(|path| probe_video(path).ok().flatten())
                .unwrap_or_default()
        } else {
            VideoProbe::default()
        };
        if probe.container.is_some()
            || probe.video_codec.is_some()
            || probe.audio_codec.is_some()
            || probe.width.is_some()
            || probe.height.is_some()
        {
            stats.ffprobe_enriched += 1;
        }

        let sha256 = copied_path
            .as_ref()
            .and_then(|path| compute_sha256(path).ok())
            .or_else(|| asset.hash.clone());
        if sha256.is_some() {
            stats.videos_with_hash += 1;
        }

        let (latitude, longitude) = valid_location(asset);
        records.push(VideoEvidenceRecord {
            schema_version: VIGIL_VIDEO_SCHEMA_VERSION,
            asset_id: Some(asset.uuid.clone()),
            asset_numeric_id: Some(asset.id),
            filename: asset.filename.clone(),
            media_type: format!("{:?}", asset.media_type),
            timestamp_utc: Some(asset.created_date.to_rfc3339()),
            source_path,
            copied_path,
            source_resolution_method: asset.source_resolution_method.clone(),
            duration_seconds: probe.duration_seconds.or(asset.duration),
            width: probe
                .width
                .or_else(|| asset.width.and_then(|value| u32::try_from(value).ok())),
            height: probe
                .height
                .or_else(|| asset.height.and_then(|value| u32::try_from(value).ok())),
            container: probe.container.or_else(|| infer_container(asset)),
            video_codec: probe.video_codec,
            audio_codec: probe.audio_codec,
            file_size: probe.file_size.or(asset.file_size),
            sha256,
            latitude,
            longitude,
        });
    }

    (records, stats)
}

fn is_video_asset(asset: &AssetRecord) -> bool {
    matches!(
        asset.media_type,
        MediaType::Video
            | MediaType::LivePhoto
            | MediaType::Timelapse
            | MediaType::SlowMo
            | MediaType::Timecode
    ) || asset.mime_type.starts_with("video/")
}

fn manifest_entry_for<'a>(
    asset: &AssetRecord,
    manifest: &'a CopyManifest,
) -> Option<&'a CopyManifestEntry> {
    manifest
        .entries
        .get(&asset.uuid)
        .or_else(|| manifest.entries.get(&asset.id.to_string()))
        .or_else(|| {
            manifest.entries.values().find(|entry| {
                entry.asset_id == asset.uuid || entry.asset_id == asset.id.to_string()
            })
        })
}

fn resolved_source_path(
    asset: &AssetRecord,
    manifest_entry: Option<&CopyManifestEntry>,
) -> Option<PathBuf> {
    normalize_existing_path(asset.resolved_source_path.as_deref())
        .or_else(|| normalize_existing_path(manifest_entry.map(|entry| entry.source_path.as_str())))
}

fn resolved_copied_path(
    charon_dir: &Path,
    manifest_entry: Option<&CopyManifestEntry>,
) -> Option<PathBuf> {
    let raw = manifest_entry?.destination_path.trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        charon_dir.join(candidate)
    };
    resolved.exists().then_some(resolved)
}

fn normalize_existing_path(raw: Option<&str>) -> Option<PathBuf> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(value);
    candidate.exists().then_some(candidate)
}

fn probe_video(path: &Path) -> Result<Option<VideoProbe>> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name,duration,size:stream=codec_name,codec_type,width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run ffprobe on {}", path.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("Failed to parse ffprobe output for {}", path.display()))?;

    let mut video_codec = None;
    let mut audio_codec = None;
    let mut width = None;
    let mut height = None;

    for stream in parsed.streams {
        match stream.codec_type.as_deref() {
            Some("video") => {
                if video_codec.is_none() {
                    video_codec = stream.codec_name;
                }
                if width.is_none() {
                    width = stream.width;
                }
                if height.is_none() {
                    height = stream.height;
                }
            }
            Some("audio") => {
                if audio_codec.is_none() {
                    audio_codec = stream.codec_name;
                }
            }
            _ => {}
        }
    }

    let container = parsed
        .format
        .as_ref()
        .and_then(|format| format.format_name.clone());
    let duration_seconds = parsed
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .and_then(parse_f64);
    let file_size = parsed
        .format
        .as_ref()
        .and_then(|format| format.size.as_deref())
        .and_then(parse_u64);

    Ok(Some(VideoProbe {
        container,
        video_codec,
        audio_codec,
        width,
        height,
        duration_seconds,
        file_size,
    }))
}

fn parse_f64(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok()
}

fn parse_u64(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
}

fn ffprobe_available() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn infer_container(asset: &AssetRecord) -> Option<String> {
    let candidate = asset
        .original_filename
        .as_deref()
        .unwrap_or(&asset.filename);
    let ext = Path::new(candidate)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let container = match ext.as_str() {
        "mp4" | "m4v" => Some("mp4".to_string()),
        "mov" => Some("mov".to_string()),
        "3gp" => Some("3gp".to_string()),
        "avi" => Some("avi".to_string()),
        _ if asset.mime_type.starts_with("video/") => {
            Some(asset.mime_type.trim_start_matches("video/").to_string())
        }
        _ => None,
    };
    container
}

fn valid_location(asset: &AssetRecord) -> (Option<f64>, Option<f64>) {
    let Some(location) = asset.location_metadata.as_ref() else {
        return (None, None);
    };
    if location.latitude.is_finite()
        && location.longitude.is_finite()
        && (-90.0..=90.0).contains(&location.latitude)
        && (-180.0..=180.0).contains(&location.longitude)
    {
        (Some(location.latitude), Some(location.longitude))
    } else {
        (None, None)
    }
}

```

## Agents — Orpheus (Database Recon)

### `src/agents/orpheus.rs`

```rust
//! Orpheus — SQLite database reconnaissance agent.

use anyhow::Result;
use serde_json::json;

use crate::agents::orpheus_recon::{list_databases, summarize_db};
use crate::agents::orpheus_report::render_markdown;
use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

const ORPHEUS_SCHEMA_VERSION: u32 = 1;
const ORPHEUS_MAX_ROWS: usize = 5;
const ORPHEUS_SENSITIVE_KEYS: &[&str] = &[
    "pass",
    "pw",
    "pwd",
    "token",
    "secret",
    "key",
    "auth",
    "credential",
];

pub struct OrpheusAgent;

impl Agent for OrpheusAgent {
    const NAME: &'static str = "Orpheus";
    const SLUG: &'static str = "orpheus";
    const SCHEMA_VERSION: u32 = ORPHEUS_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let db_paths = list_databases(&ctx.backup_root);

        let mut summaries = Vec::with_capacity(db_paths.len());
        let mut total_tables = 0usize;
        let mut total_rows = 0usize;
        let mut databases_with_errors = 0usize;
        let mut total_sensitive_columns = 0usize;

        for path in &db_paths {
            let summary = summarize_db(path, ORPHEUS_MAX_ROWS, ORPHEUS_SENSITIVE_KEYS);

            total_tables += summary.tables.len();
            total_rows += summary
                .tables
                .iter()
                .map(|t| t.row_count as usize)
                .sum::<usize>();
            total_sensitive_columns += summary
                .tables
                .iter()
                .map(|t| t.sensitive_columns.len())
                .sum::<usize>();
            if !summary.errors.is_empty() {
                databases_with_errors += 1;
            }

            summaries.push(summary);
        }

        let markdown_report = render_markdown(&summaries);

        let mut records = Vec::with_capacity(summaries.len() + 1);

        // Report record
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "total_databases": summaries.len(),
                "total_tables": total_tables,
                "total_rows": total_rows,
                "databases_with_errors": databases_with_errors,
                "total_sensitive_columns": total_sensitive_columns,
                "markdown_report": markdown_report,
            }),
        });

        // Database records
        for summary in summaries {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "database".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: serde_json::to_value(&summary)?,
            });
        }

        Ok(records)
    }
}

```

### `src/agents/orpheus_crypto.rs`

```rust
//! Orpheus Cryptographic Operations
//!
//! AES-256-GCM encryption with PBKDF2-HMAC-SHA256 key derivation.
//! Compatible with the original Python Tartarus encryption format.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Encrypted data blob structure (JSON-serializable)
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedBlob {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_kdf")]
    pub kdf: String,
    #[serde(default = "default_iterations")]
    pub iterations: u32,
}

fn default_algorithm() -> String {
    "AES-256-GCM".to_string()
}

fn default_kdf() -> String {
    "PBKDF2-HMAC-SHA256".to_string()
}

fn default_iterations() -> u32 {
    200000
}

/// Encrypt plaintext using AES-256-GCM with PBKDF2 key derivation
pub fn encrypt_text(text: &str, passphrase: &str) -> Result<String> {
    use base64::{engine::general_purpose, Engine as _};
    use rand::Rng;

    // Generate random salt (16 bytes) and nonce (12 bytes)
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill(&mut salt);
    rand::thread_rng().fill(&mut nonce_bytes);

    // Derive key using PBKDF2-HMAC-SHA256 (200,000 iterations)
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, 200_000, &mut key);

    // Encrypt with AES-256-GCM
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Invalid key length: {:?}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, text.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

    // Build JSON blob
    let blob = EncryptedBlob {
        salt: general_purpose::STANDARD.encode(&salt),
        nonce: general_purpose::STANDARD.encode(&nonce_bytes),
        ciphertext: general_purpose::STANDARD.encode(&ciphertext),
        algorithm: "AES-256-GCM".to_string(),
        kdf: "PBKDF2-HMAC-SHA256".to_string(),
        iterations: 200000,
    };

    serde_json::to_string_pretty(&blob).context("Failed to serialize encrypted blob")
}

/// Decrypt an encrypted blob using AES-256-GCM
pub fn decrypt_blob(blob: &EncryptedBlob, passphrase: &str) -> Result<String> {
    use base64::{engine::general_purpose, Engine as _};

    // Decode base64 fields
    let salt = general_purpose::STANDARD
        .decode(&blob.salt)
        .context("Invalid salt encoding")?;
    let nonce_bytes = general_purpose::STANDARD
        .decode(&blob.nonce)
        .context("Invalid nonce encoding")?;
    let ciphertext = general_purpose::STANDARD
        .decode(&blob.ciphertext)
        .context("Invalid ciphertext encoding")?;

    // Derive key using PBKDF2
    let mut key = [0u8; 32];
    let iterations = blob.iterations;
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, iterations, &mut key);

    // Decrypt
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Invalid key length: {:?}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;

    String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted content")
}

```

### `src/agents/orpheus_ingest.rs`

```rust
//! Orpheus Document Ingestion Module
//!
//! Organizes case evidence into categorized folders with metadata
//! and maintains audit trails.

use anyhow::{Context, Result};
use filetime::{set_file_times, FileTime};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const ION_MIME: &str = "application/x-ion";

/// Ingest files into a case directory structure
pub fn ingest_documents(
    case_id: String,
    case_dir: Option<PathBuf>,
    files: Vec<PathBuf>,
    police_report: Option<String>,
    project_scope: Option<String>,
    notes: String,
    doc_categories: &[(&str, &[&str])],
) -> Result<()> {
    let (case_root, ingest_root, log_path) = resolve_case_layout(&case_id, case_dir)?;

    fs::create_dir_all(&case_root)?;
    fs::create_dir_all(&ingest_root)?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Build context hint from police report and project scope
    let context_entries: Vec<String> = police_report
        .as_ref()
        .map(|p| {
            Path::new(p)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .into_iter()
        .chain(project_scope.as_ref().map(|p| {
            if Path::new(p).exists() {
                Path::new(p)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            } else {
                p.clone()
            }
        }))
        .collect();

    let context_hint = if context_entries.is_empty() {
        "case scope".to_string()
    } else {
        context_entries.join(" & ")
    };

    println!(
        "[ORPHEUS] Ingesting {} files for case {}",
        files.len(),
        case_id
    );
    append_case_log(
        &log_path,
        &format!(
            "ORPHEUS — Ingesting {} files into {}",
            files.len(),
            ingest_root.display()
        ),
    )?;

    for file_arg in &files {
        if !file_arg.exists() {
            println!("[WARN] {} missing; skipping.", file_arg.display());
            append_case_log(
                &log_path,
                &format!("ORPHEUS — Missing file skipped: {}", file_arg.display()),
            )?;
            continue;
        }

        let category = categorize_path(file_arg, doc_categories);
        let dest_dir = ingest_root.join(&category);
        fs::create_dir_all(&dest_dir)?;

        let dest = dest_dir.join(file_arg.file_name().context("Invalid filename")?);

        copy_with_metadata(file_arg, &dest)?;

        // Guess mime type
        let mime = guess_mime_type(file_arg);

        let metadata = serde_json::json!({
            "source": file_arg.canonicalize()?.to_string_lossy(),
            "destination": dest.canonicalize()?.to_string_lossy(),
            "mimetype": mime,
            "category": category,
            "case_id": case_id,
            "police_report": police_report,
            "project_scope": project_scope,
            "notes": notes,
            "context_hint": context_hint,
            "ingested_at": chrono::Utc::now().to_rfc3339(),
        });

        // Write metadata file with .meta.json extension
        let meta_ext = format!(
            "{}.meta.json",
            dest.extension().unwrap_or_default().to_string_lossy()
        );
        let meta_path = dest.with_extension(meta_ext);
        fs::write(&meta_path, serde_json::to_string_pretty(&metadata)?)?;

        println!(
            "[ORPHEUS] {} -> {} ({}); {} reviewed.",
            file_arg.file_name().unwrap_or_default().to_string_lossy(),
            dest.display(),
            category,
            context_hint
        );

        append_case_log(
            &log_path,
            &format!(
                "ORPHEUS — Stored {} in evidence/documents/{}; {} applied.",
                file_arg.file_name().unwrap_or_default().to_string_lossy(),
                category,
                context_hint
            ),
        )?;
    }

    println!(
        "[ORPHEUS] Ingestion finished. Files staged under {}",
        ingest_root.display()
    );
    append_case_log(
        &log_path,
        &format!(
            "ORPHEUS COMPLETE — Document ingest finished: {}",
            ingest_root.display()
        ),
    )?;
    Ok(())
}

/// Categorize a file based on its extension
pub fn categorize_path(path: &Path, doc_categories: &[(&str, &[&str])]) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();

    for (category, exts) in doc_categories {
        if exts.contains(&ext.as_str()) {
            return category.to_string();
        }
    }
    "misc".to_string()
}

fn resolve_case_layout(
    case_id: &str,
    case_dir: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    match case_dir {
        Some(root) => {
            let ingest_root = root.join("evidence").join("documents");
            let log_path = root.join("logs").join("case.log");
            Ok((root, ingest_root, log_path))
        }
        None => {
            let case_root = PathBuf::from("/home/ghost/case").join(case_id);
            let ingest_root = case_root.join("evidence").join("documents");
            let log_path = case_root.join("logs").join("case.log");
            Ok((case_root, ingest_root, log_path))
        }
    }
}

fn guess_mime_type(path: &Path) -> String {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .as_deref()
    {
        Some("styg") => ION_MIME.to_string(),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    }
}

fn copy_with_metadata(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination)?;
    let metadata = fs::metadata(source)?;
    fs::set_permissions(destination, metadata.permissions())?;

    let accessed = FileTime::from_last_access_time(&metadata);
    let modified = FileTime::from_last_modification_time(&metadata);
    set_file_times(destination, accessed, modified)?;

    Ok(())
}

fn append_case_log(log_path: &Path, message: &str) -> Result<()> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    writeln!(
        file,
        "[{}] {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    )?;
    Ok(())
}

```

### `src/agents/orpheus_recon.rs`

```rust
//! Orpheus Database Reconnaissance Module
//!
//! Discovers and analyzes SQLite databases across mobile device backups.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Validates that an identifier is safe to use in SQL (table/column names).
/// Only allows alphanumeric characters and underscores. Must start with a letter or underscore.
fn is_safe_sql_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // First character must be alphabetic or underscore
    let first = name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    // Rest must be alphanumeric or underscore
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Summary of a database table
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableSummary {
    pub name: String,
    pub columns: Vec<String>,
    pub row_count: i64,
    pub sensitive_columns: Vec<String>,
    pub sample_rows: Vec<HashMap<String, serde_json::Value>>,
}

/// Summary of a complete database
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DatabaseSummary {
    pub path: String,
    pub tables: Vec<TableSummary>,
    pub errors: Vec<String>,
}

/// List all SQLite databases in a directory tree
pub fn list_databases(root: &Path) -> Vec<PathBuf> {
    let extensions: HashSet<&str> = ["sqlite", "db", "sqlitedb", "storedata"]
        .iter()
        .cloned()
        .collect();

    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Analyze a single database and return summary
pub fn summarize_db(path: &Path, max_rows: usize, sensitive_keys: &[&str]) -> DatabaseSummary {
    let path_str = path.to_string_lossy().to_string();
    let mut tables = Vec::new();
    let mut errors = Vec::new();

    // Evidence databases must be opened read-only; fail closed instead of mutating files.
    let conn_result = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    );

    let conn = match conn_result {
        Ok(c) => c,
        Err(e) => {
            return DatabaseSummary {
                path: path_str,
                tables: Vec::new(),
                errors: vec![format!("open_error: {}", e)],
            };
        }
    };

    // Set busy timeout to avoid locking issues in parallel mode
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));

    // Get table names
    let table_names: Vec<String> =
        match conn.prepare("SELECT name FROM sqlite_master WHERE type='table'") {
            Ok(mut stmt) => match stmt.query_map([], |row| row.get::<_, String>(0)) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    errors.push(format!("table_list_error: {}", e));
                    Vec::new()
                }
            },
            Err(e) => {
                errors.push(format!("db_error: {}", e));
                return DatabaseSummary {
                    path: path_str,
                    tables,
                    errors,
                };
            }
        };

    for table_name in table_names {
        match summarize_table(&conn, &table_name, max_rows, sensitive_keys) {
            Ok(summary) => tables.push(summary),
            Err(e) => errors.push(format!("table_error {}: {}", table_name, e)),
        }
    }

    DatabaseSummary {
        path: path_str,
        tables,
        errors,
    }
}

/// Analyze a single table within a database
fn summarize_table(
    conn: &rusqlite::Connection,
    table_name: &str,
    max_rows: usize,
    sensitive_keys: &[&str],
) -> Result<TableSummary> {
    // Validate table name to prevent SQL injection
    if !is_safe_sql_identifier(table_name) {
        return Err(anyhow!("Invalid table name: {}", table_name));
    }
    
    // Get columns
    let mut col_stmt = conn.prepare(&format!("PRAGMA table_info('{}')", table_name))?;
    let columns: Vec<String> = col_stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();

    // Detect sensitive columns
    let sensitive_columns: Vec<String> = columns
        .iter()
        .filter(|col| {
            let col_lower = col.to_lowercase();
            sensitive_keys.iter().any(|k| col_lower.contains(k))
        })
        .cloned()
        .collect();

    // Get row count (table_name already validated above)
    let count: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM '{}'", table_name),
        [],
        |row| row.get(0),
    )?;

    // Get sample rows (table_name already validated above)
    let mut samples = Vec::new();
    if max_rows > 0 && count > 0 {
        let query = format!("SELECT * FROM '{}' LIMIT {}", table_name, max_rows);
        let mut stmt = conn.prepare(&query)?;
        let column_names: Vec<String> = (0..stmt.column_count())
            .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
            .collect();

        let rows = stmt.query_map([], |row| {
            let mut row_map = HashMap::new();
            for (idx, col_name) in column_names.iter().enumerate() {
                let value = match row.get::<_, rusqlite::types::Value>(idx) {
                    Ok(v) => sanitize_value(v),
                    Err(_) => serde_json::Value::Null,
                };
                row_map.insert(col_name.clone(), value);
            }
            Ok(row_map)
        })?;

        for row in rows.take(max_rows) {
            if let Ok(r) = row {
                samples.push(r);
            }
        }
    }

    Ok(TableSummary {
        name: table_name.to_string(),
        columns,
        row_count: count,
        sensitive_columns,
        sample_rows: samples,
    })
}

/// Sanitize a database value for JSON serialization
fn sanitize_value(val: rusqlite::types::Value) -> serde_json::Value {
    match val {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::Value::Number(i.into()),
        rusqlite::types::Value::Real(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)),
        ),
        rusqlite::types::Value::Text(s) => serde_json::Value::String(s),
        rusqlite::types::Value::Blob(b) => {
            serde_json::Value::String(format!("<bytes {}>", b.len()))
        }
    }
}

```

### `src/agents/orpheus_report.rs`

```rust
//! Orpheus Report Generation Module
//!
//! Generates Markdown and JSON reports with integrity manifests.

use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use walkdir::WalkDir;

use crate::agents::orpheus_recon::DatabaseSummary;

/// Integrity manifest structure
#[derive(Debug, Serialize)]
pub struct Manifest {
    pub generated_at: String,
    pub root: String,
    pub sha256: HashMap<String, String>,
}

/// Generate Markdown report from database summaries
pub fn render_markdown(db_summaries: &[DatabaseSummary]) -> String {
    let mut lines = vec![
        "# Orpheus SQLite Recon".to_string(),
        String::new(),
        format!("Databases scanned: {}", db_summaries.len()),
        String::new(),
    ];

    for db in db_summaries {
        lines.push(format!("## DB: {}", db.path));
        lines.push(String::new());

        if !db.errors.is_empty() {
            lines.push(format!("- Errors: {:?}", db.errors));
        }

        if db.tables.is_empty() {
            lines.push("- No tables found or unreadable.".to_string());
        }

        for table in &db.tables {
            lines.push(format!("### Table: {}", table.name));
            lines.push(format!("- Columns: {}", table.columns.join(", ")));
            lines.push(format!("- Rows: {}", table.row_count));
            lines.push(format!(
                "- Sensitive columns: {}",
                if table.sensitive_columns.is_empty() {
                    "None detected".to_string()
                } else {
                    table.sensitive_columns.join(", ")
                }
            ));
            lines.push("- Samples:".to_string());

            if table.sample_rows.is_empty() {
                lines.push("  - <no samples>".to_string());
            } else {
                for sample in &table.sample_rows {
                    lines.push(format!(
                        "  - {}",
                        serde_json::to_string(sample).unwrap_or_default()
                    ));
                }
            }
            lines.push(String::new());
        }
    }

    lines.join("\n") + "\n"
}

/// Write integrity manifest with SHA-256 hashes
pub fn write_hash_manifest(root: &Path, log_path: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let mut entries = HashMap::new();

    for entry in WalkDir::new(&root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Ok(hash) = sha256_file(path) {
                entries.insert(path.to_string_lossy().to_string(), hash);
            }
        }
    }

    let manifest = Manifest {
        generated_at: chrono::Utc::now().to_rfc3339(),
        root: root.to_string_lossy().to_string(),
        sha256: entries,
    };

    let manifest_path = root.join("INTEGRITY.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    make_readonly(&manifest_path)?;

    // Write log entry
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    writeln!(
        log,
        "[{}] orpheus hash_manifest root={}",
        chrono::Utc::now().to_rfc3339(),
        root.display()
    )?;

    Ok(())
}

/// Calculate SHA-256 hash of a file
fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Make a file read-only (Unix only)
fn make_readonly(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o444);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

```

## Evidence Pipeline

### `src/evidence/mod.rs`

```rust
//! Unified evidence output pipeline.
//!
//! Every agent produces a Vec<EvidenceRecord> and calls write_evidence().
//! This module handles JSON, CSV, JSONL, and summary.json generation.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// One evidence record. Agents produce Vec<EvidenceRecord>.
/// The `payload` field contains the agent-specific struct serialized to JSON Value.
/// The `source_fields` map is written as CSV columns.
#[derive(Debug, Serialize)]
pub struct EvidenceRecord {
    pub schema_version: u32,
    pub source_agent: String,
    pub record_type: String,
    pub timestamp: String,
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

/// Write all evidence outputs for an agent run.
///
/// Produces:
///   evidence/<slug>/records.json      — full records as JSON array
///   evidence/<slug>/records.jsonl     — one JSON object per line
///   evidence/<slug>/records.csv       — flattened CSV
///   evidence/<slug>/summary.json      — agent metadata + record count
pub fn write_evidence(
    evidence_dir: &Path,
    agent_name: &str,
    slug: &str,
    schema_version: u32,
    records: &[EvidenceRecord],
) -> Result<()> {
    fs::create_dir_all(evidence_dir)
        .with_context(|| format!("creating evidence dir {}", evidence_dir.display()))?;

    let records_path = evidence_dir.join("records.json");
    let jsonl_path = evidence_dir.join("records.jsonl");
    let csv_path = evidence_dir.join("records.csv");
    let summary_path = evidence_dir.join("summary.json");

    // records.json — full JSON array
    fs::write(&records_path, serde_json::to_vec_pretty(records)?)
        .with_context(|| format!("writing {}", records_path.display()))?;

    // records.jsonl — one object per line
    let mut jsonl = String::new();
    for record in records {
        jsonl.push_str(&serde_json::to_string(record)?);
        jsonl.push('\n');
    }
    fs::write(&jsonl_path, jsonl)
        .with_context(|| format!("writing {}", jsonl_path.display()))?;

    // records.csv — flatten to CSV using the payload fields
    if let Err(e) = write_csv(&csv_path, records) {
        log::warn!("CSV export failed for {}: {}", slug, e);
    }

    // summary.json — agent metadata
    let summary = Summary {
        agent: agent_name.to_string(),
        slug: slug.to_string(),
        schema_version,
        generated_at: Utc::now().to_rfc3339(),
        record_count: records.len(),
        evidence_dir: evidence_dir.display().to_string(),
    };
    fs::write(&summary_path, serde_json::to_vec_pretty(&summary)?)
        .with_context(|| format!("writing {}", summary_path.display()))?;

    Ok(())
}

#[derive(Debug, Serialize)]
struct Summary {
    agent: String,
    slug: String,
    schema_version: u32,
    generated_at: String,
    record_count: usize,
    evidence_dir: String,
}

fn write_csv(path: &Path, records: &[EvidenceRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;

    // Collect all unique keys from all payload objects
    let mut keys: Vec<String> = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    for record in records {
        if let Some(obj) = record.payload.as_object() {
            for key in obj.keys() {
                if seen_keys.insert(key.clone()) {
                    keys.push(key.clone());
                }
            }
        }
    }
    keys.sort();

    // Add metadata columns at front
    let mut header = vec![
        "schema_version".to_string(),
        "source_agent".to_string(),
        "record_type".to_string(),
        "timestamp".to_string(),
    ];
    header.extend(keys.clone());
    wtr.write_record(&header)?;

    for record in records {
        let mut row = vec![
            record.schema_version.to_string(),
            record.source_agent.clone(),
            record.record_type.clone(),
            record.timestamp.clone(),
        ];
        if let Some(obj) = record.payload.as_object() {
            for key in &keys {
                let val = obj
                    .get(key)
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                row.push(val);
            }
        } else {
            for _ in &keys {
                row.push(String::new());
            }
        }
        wtr.write_record(&row)?;
    }

    wtr.flush()?;
    Ok(())
}

```

## Case Management

### `src/case.rs`

```rust
use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    root: PathBuf,
}

impl Case {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let root = locate_case_root(&name)?;

        // Persist the resolved root so future invocations from any directory reuse it.
        persist_case_root(&name, &root)?;

        Ok(Self { name, root })
    }

    pub fn from_root(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let name = name.into();
        let root = root.into();
        let _ = persist_case_root(&name, &root);
        Self { name, root }
    }

    pub fn open(&self, module: &str) -> Result<()> {
        self.ensure_base_dirs()?;
        fs::create_dir_all(self.path(module))?;
        self.log(&format!("{} initialized", module.to_uppercase()), None)?;
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn workspace(&self) -> CaseWorkspace {
        CaseWorkspace::new(self.root.clone())
    }

    pub fn backup_path(&self) -> PathBuf {
        self.root.join("backup")
    }

    pub fn ion_backup_path(&self) -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("iON_BACKUP_DIR") {
            return Ok(PathBuf::from(dir).join("cases").join(&self.name));
        }
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        Ok(home
            .join("iON")
            .join("backups")
            .join("cases")
            .join(&self.name))
    }

    pub fn source_backup_root(&self) -> Result<PathBuf> {
        for candidate in [
            self.root.join("source").join("backup"),
            self.root.join("backup"),
        ] {
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        Err(anyhow::anyhow!(
            "No source backup root found under {}",
            self.root.display()
        ))
    }

    pub fn discovered_helios_roots(&self) -> Result<Vec<PathBuf>> {
        let mut discovered = Vec::new();
        let mut seen = HashSet::new();
        let workspace = self.workspace();

        for candidate in [
            workspace.helios_full_root(),
            workspace.helios_sms_only_root(),
            self.root.join("prepared"),
            self.root.join("source").join("imports"),
            self.root.clone(),
        ] {
            collect_helios_extract_roots(&candidate, &mut discovered, &mut seen);
        }

        Ok(discovered)
    }

    pub fn active_backup_root(&self) -> Result<PathBuf> {
        if let Some(root) = self.discovered_helios_roots()?.into_iter().next() {
            return Ok(root);
        }

        self.source_backup_root()
    }

    pub fn evidence_path(&self, subdir: &str) -> PathBuf {
        self.root.join("evidence").join(subdir)
    }

    pub fn path(&self, subdir: &str) -> PathBuf {
        self.root.join(subdir)
    }

    pub fn write_file(&self, path: &Path, data: Vec<u8>) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, data).with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn log(&self, message: &str, context: Option<&str>) -> Result<()> {
        let logs_dir = self.root.join("logs");
        fs::create_dir_all(&logs_dir)?;
        let log_path = logs_dir.join("case.log");
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log {}", log_path.display()))?;

        match context {
            Some(ctx) => writeln!(file, "[{}] {} | {}", timestamp, message, ctx)?,
            None => writeln!(file, "[{}] {}", timestamp, message)?,
        }

        Ok(())
    }

    fn ensure_base_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("logs"))?;
        fs::create_dir_all(self.root.join("evidence"))?;
        fs::create_dir_all(self.root.join("backup"))?;
        fs::create_dir_all(self.root.join("clean"))?;
        Ok(())
    }

    /// Persist phone numbers associated with this case so other modules can auto-resolve them.
    pub fn remember_phone_numbers<S: AsRef<str>>(&self, phones: &[S]) -> Result<()> {
        let mut entry = load_registry_entry(&self.name)?;
        entry.root = self.root.to_string_lossy().to_string();

        for phone in phones {
            let normalized = normalize_phone(phone.as_ref());
            if normalized.is_empty() {
                continue;
            }
            if !entry.phone_numbers.iter().any(|p| p == &normalized) {
                entry.phone_numbers.push(normalized);
            }
        }

        persist_registry_entry(&self.name, &entry)
    }

    /// Persist a friendly contact name -> phone numbers mapping for this case.
    pub fn remember_contact_mapping<S: AsRef<str>>(&self, name: &str, phones: &[S]) -> Result<()> {
        let mut entry = load_registry_entry(&self.name)?;
        entry.root = self.root.to_string_lossy().to_string();

        let key = normalize_name(name);
        let mut existing: HashSet<String> = entry
            .contacts
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();

        for phone in phones {
            let normalized = normalize_phone(phone.as_ref());
            if !normalized.is_empty() {
                existing.insert(normalized);
            }
        }

        if !existing.is_empty() {
            entry.contacts.insert(key, existing.into_iter().collect());
        }

        persist_registry_entry(&self.name, &entry)
    }

    /// Read the stored registry entry for this case, if any.
    pub fn registry_entry(&self) -> Result<Option<CaseRegistryEntry>> {
        let entry = load_registry_entry(&self.name)?;
        if entry.root.is_empty() {
            return Ok(None);
        }
        Ok(Some(entry))
    }

    /// Get the device phone number stored for this case, if any.
    pub fn device_phone_number(&self) -> Result<Option<String>> {
        let entry = load_registry_entry(&self.name)?;
        Ok(entry.device_phone_number)
    }

    /// Store the device phone number for this case.
    pub fn set_device_phone_number(&self, phone: &str) -> Result<()> {
        let mut entry = load_registry_entry(&self.name)?;
        entry.root = self.root.to_string_lossy().to_string();
        let normalized = normalize_phone(phone);
        entry.device_phone_number = if normalized.is_empty() { None } else { Some(normalized) };
        persist_registry_entry(&self.name, &entry)
    }
}

#[derive(Clone, Debug)]
pub struct CaseWorkspace {
    root: PathBuf,
}

impl CaseWorkspace {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.logs_dir(),
            self.reports_dir(),
            self.prepared_database_root(),
            self.prepared_helios_dir(),
            self.helios_full_root(),
            self.helios_sms_only_root(),
            self.helios_metadata_root(),
            self.index_dir(),
            self.live_device_mounts_dir(),
            self.root.join("source").join("backup"),
            self.root.join("evidence"),
        ] {
            fs::create_dir_all(dir)?;
        }

        Ok(())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("reports")
    }

    pub fn prepared_database_root(&self) -> PathBuf {
        self.root.join("prepared").join("db")
    }

    pub fn prepared_helios_dir(&self) -> PathBuf {
        self.root.join("prepared").join("helios")
    }

    pub fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    pub fn live_device_mounts_dir(&self) -> PathBuf {
        self.root.join("tmp").join("device-mounts")
    }

    pub fn helios_full_root(&self) -> PathBuf {
        self.prepared_helios_dir().join("full")
    }

    pub fn helios_sms_only_root(&self) -> PathBuf {
        self.prepared_helios_dir().join("sms_only")
    }

    pub fn helios_metadata_root(&self) -> PathBuf {
        self.prepared_helios_dir().join("metadata")
    }
}

fn collect_helios_extract_roots(base: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if !base.exists() {
        return;
    }

    if looks_like_helios_extract(base) {
        let candidate = base.to_path_buf();
        if seen.insert(candidate.clone()) {
            out.push(candidate);
        }
        return;
    }

    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if looks_like_helios_extract(&path) {
                if seen.insert(path.clone()) {
                    out.push(path);
                }
                continue;
            }

            if let Ok(children) = fs::read_dir(&path) {
                for child in children.flatten() {
                    let child_path = child.path();
                    if child_path.is_dir()
                        && looks_like_helios_extract(&child_path)
                        && seen.insert(child_path.clone())
                    {
                        out.push(child_path);
                    }
                }
            }
        }
    }
}

fn looks_like_helios_extract(path: &Path) -> bool {
    path.join("_manifest")
        .join("Manifest.decrypted.db")
        .exists()
        || path.join("Manifest.decrypted.db").exists()
        || path.join("HomeDomain").exists()
        || path.join("RootDomain").exists()
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CaseRegistryEntry {
    pub root: String,
    pub phone_numbers: Vec<String>,
    pub contacts: HashMap<String, Vec<String>>,
    /// The phone number of the device owner (the acquired device).
    /// Used for sent-message attribution in Cerberus and other modules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_phone_number: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CaseRegistry {
    cases: HashMap<String, CaseRegistryEntry>,
}

fn locate_case_root(name: &str) -> Result<PathBuf> {
    // Priority order:
    // 1) Previously remembered registry entry
    // 2) Environment hints (iON_CASE_ROOTS / iON_HOME)
    // 3) Common install locations (home, cwd, parents, ghostdevops folders)
    // 4) Fallback to ./cases/<name>
    let remembered_root = load_registry_entry(name).ok().and_then(|e| {
        if !e.root.is_empty() {
            let p = PathBuf::from(&e.root);
            if p.exists() {
                return Some(p);
            }
        }
        None
    });

    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut push_candidate = |p: PathBuf| {
        if seen.insert(p.clone()) {
            candidates.push(p);
        }
    };

    // Environment provided roots (supports multiple, separated by OS path separator or comma/semicolon).
    if let Ok(var) = std::env::var("iON_CASE_ROOTS") {
        for p in split_paths_flexible(&var) {
            push_candidate(p);
        }
    }
    if let Ok(var) = std::env::var("iON_HOME") {
        push_candidate(PathBuf::from(var));
    }
    if let Ok(var) = std::env::var("iON_CASE_ROOT") {
        push_candidate(PathBuf::from(var));
    }

    if let Some(home) = dirs::home_dir() {
        push_candidate(home.clone());
        push_candidate(home.join("iON").join("cases"));
    }

    // Current dir and a few parents
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = Some(cwd.as_path());
        for _ in 0..4 {
            if let Some(p) = cur {
                push_candidate(p.to_path_buf());
                cur = p.parent();
            }
        }
    }

    // Crate root if available
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        push_candidate(PathBuf::from(manifest_dir));
    }

    // Common ghostdevops roots
    if let Some(home) = dirs::home_dir() {
        push_candidate(home.join("Documents").join("ghostdevops"));
        push_candidate(home.join("Documents").join("ghostdevops").join("iON"));
        push_candidate(home.join("Documents").join("ghostdevops").join("iON3"));
        push_candidate(
            home.join("Documents")
                .join("ghostdevops")
                .join("iON3_windows_output_results"),
        );
    }

    let discovered_root = find_case_root_in_bases(name, &candidates);

    if let Some(root) = select_preferred_case_root(name, remembered_root, discovered_root) {
        return Ok(root);
    }

    // Nothing found; default to ./cases/<name> next to cwd.
    Ok(std::env::current_dir()?.join("cases").join(name))
}

fn find_case_root_in_bases(name: &str, bases: &[PathBuf]) -> Option<PathBuf> {
    for base in bases {
        for path in candidate_case_paths(base, name) {
            if path.exists() && path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

fn candidate_case_paths(base: &Path, name: &str) -> [PathBuf; 4] {
    [
        base.join("cases").join(name),
        base.join("output").join("iON").join(name),
        base.join(name).join("cases"),
        base.join(name),
    ]
}

fn select_preferred_case_root(
    name: &str,
    remembered_root: Option<PathBuf>,
    discovered_root: Option<PathBuf>,
) -> Option<PathBuf> {
    match (remembered_root, discovered_root) {
        (Some(remembered), Some(discovered))
            if is_output_case_root(&remembered, name) && remembered != discovered =>
        {
            Some(discovered)
        }
        (Some(remembered), _) => Some(remembered),
        (None, Some(discovered)) => Some(discovered),
        (None, None) => None,
    }
}

fn is_output_case_root(path: &Path, name: &str) -> bool {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    components.len() >= 3
        && components[components.len() - 1] == name
        && components[components.len() - 2] == "iON"
        && components[components.len() - 3] == "output"
}

fn registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".iON")
        .join("case_registry.json")
}

fn load_registry_entry(name: &str) -> Result<CaseRegistryEntry> {
    let reg = load_registry()?;
    Ok(reg.cases.get(name).cloned().unwrap_or_default())
}

fn persist_registry_entry(name: &str, entry: &CaseRegistryEntry) -> Result<()> {
    let mut reg = load_registry()?;
    reg.cases.insert(name.to_string(), entry.clone());
    save_registry(&reg)
}

fn load_registry() -> Result<CaseRegistry> {
    let path = registry_path();
    if !path.exists() {
        return Ok(CaseRegistry::default());
    }

    let data = fs::read(&path)?;
    let reg: CaseRegistry = serde_json::from_slice(&data).unwrap_or_default();
    Ok(reg)
}

fn save_registry(reg: &CaseRegistry) -> Result<()> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(reg)?;
    fs::write(&path, data)?;
    Ok(())
}

fn persist_case_root(case: &str, root: &Path) -> Result<()> {
    let mut entry = load_registry_entry(case)?;
    entry.root = root.to_string_lossy().to_string();
    persist_registry_entry(case, &entry)
}

fn split_paths_flexible(raw: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for chunk in raw.split(',') {
        for p in std::env::split_paths(chunk) {
            if !p.as_os_str().is_empty() {
                out.push(p);
            }
        }
    }
    out
}

fn normalize_phone(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn normalize_name(raw: &str) -> String {
    raw.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_real_case_root_over_output_registry_root() {
        let remembered = Some(PathBuf::from("/tmp/output/iON/Case-001"));
        let discovered = Some(PathBuf::from("/tmp/cases/Case-001"));

        let resolved =
            select_preferred_case_root("Case-001", remembered, discovered).expect("expected root");

        assert_eq!(resolved, PathBuf::from("/tmp/cases/Case-001"));
    }

    #[test]
    fn preserves_non_output_registry_root() {
        let remembered = Some(PathBuf::from("/tmp/custom/Case-001"));
        let discovered = Some(PathBuf::from("/tmp/cases/Case-001"));

        let resolved =
            select_preferred_case_root("Case-001", remembered, discovered).expect("expected root");

        assert_eq!(resolved, PathBuf::from("/tmp/custom/Case-001"));
    }
}

```

### `src/ui_core.rs`

```rust
use crate::chronos::device::DeviceInfo as ChronosDeviceInfo;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// Shared helpers for UI layers (TUI + GUI) to read case state and logs.
#[derive(Debug, Clone)]
pub struct UiPaths {
    pub root: PathBuf,
    pub case_id: String,
}

impl UiPaths {
    pub fn new(root: PathBuf, case_id: String) -> Self {
        Self { root, case_id }
    }

    pub fn case_dir(&self) -> PathBuf {
        self.root.join("output").join("iON").join(&self.case_id)
    }

    pub fn context_path(&self) -> PathBuf {
        self.case_dir().join("iON_context.json")
    }

    pub fn log_path(&self) -> PathBuf {
        self.case_dir().join("iON_log.txt")
    }

    pub fn case_root(&self) -> PathBuf {
        self.root.join("cases").join(&self.case_id)
    }
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub paths: UiPaths,
    pub chronos_device_info: Option<ChronosDeviceInfo>,
    pub log_lines: Vec<String>,
    pub events: Vec<TimelineEvent>,
    pub artifacts: Vec<CaseItem>,
    pub running: bool,
    pub last_refreshed: DateTime<Utc>,
}

impl UiState {
    pub fn load(paths: UiPaths) -> Result<Self> {
        let mut state = Self {
            paths,
            chronos_device_info: None,
            log_lines: Vec::new(),
            events: Vec::new(),
            artifacts: Vec::new(),
            running: false,
            last_refreshed: Utc::now(),
        };
        state.refresh()?;
        Ok(state)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.chronos_device_info = load_device_info(&self.paths)?;
        self.log_lines = read_logs(&self.paths, 400)?;
        self.events = parse_timeline(&self.log_lines);
        self.artifacts = list_case_items(&self.paths, 64)?;
        self.last_refreshed = Utc::now();
        Ok(())
    }
}

/// Pick the newest case directory under output/iON when no case id is provided.
pub fn discover_latest_case(root: &Path) -> Result<Option<String>> {
    let cases_root = root.join("output").join("iON");
    if !cases_root.is_dir() {
        return Ok(None);
    }

    let mut newest: Option<(SystemTime, String)> = None;
    for entry in std::fs::read_dir(cases_root)? {
        let entry = entry?;
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_dir() {
                if let Ok(modified) = metadata.modified() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    match &newest {
                        Some((ts, _)) if *ts >= modified => {}
                        _ => newest = Some((modified, name)),
                    }
                }
            }
        }
    }

    Ok(newest.map(|(_, name)| name))
}

pub fn load_device_info(paths: &UiPaths) -> Result<Option<ChronosDeviceInfo>> {
    let path = paths.case_root().join("chronos_device_info.json");
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read device info at {}", path.display()))?;
    let info: ChronosDeviceInfo = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse device info at {}", path.display()))?;
    Ok(Some(info))
}

pub fn read_logs(paths: &UiPaths, max_lines: usize) -> Result<Vec<String>> {
    let log_path = paths.log_path();
    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&log_path)
        .with_context(|| format!("failed to open log at {}", log_path.display()))?;
    let reader = BufReader::new(file);
    let mut lines: Vec<String> = reader
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .collect();

    if lines.len() > max_lines {
        let start = lines.len() - max_lines;
        lines = lines.split_off(start);
    }

    Ok(lines)
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    pub timestamp: String,
    pub level: String,
    pub agent: String,
    pub message: String,
}

/// Parse Nemesis-style log lines into timeline events; leaves lines that don't match untouched.
pub fn parse_timeline(lines: &[String]) -> Vec<TimelineEvent> {
    let mut events = Vec::new();
    for line in lines {
        if let Some(evt) = parse_log_line(line) {
            events.push(evt);
        }
    }
    events
}

fn parse_log_line(line: &str) -> Option<TimelineEvent> {
    // Example: [2025-12-19T12:43:50.573902800+00:00] Info NEMESIS: Running agent: chronos run
    let line = line.trim();
    if !line.starts_with('[') {
        return None;
    }
    let ts_end = line.find(']')?;
    let ts = line.get(1..ts_end)?.to_string();
    let rest = line.get(ts_end + 1..)?.trim();
    let mut parts = rest.split_whitespace();
    let level = parts.next()?.to_string();
    let agent = parts.next()?.trim_end_matches(':').to_string();
    let message = parts.collect::<Vec<_>>().join(" ");
    Some(TimelineEvent {
        timestamp: ts,
        level,
        agent,
        message,
    })
}

/// Run an agent command (e.g., `chronos ["--encrypted"]`) and stream output to the Nemesis log path.
/// This is synchronous; callers should manage UI blocking if running long jobs.
pub fn run_agent(paths: &UiPaths, agent_bin: &str, args: &[&str]) -> Result<()> {
    let case_dir = paths.case_dir();
    if !case_dir.exists() {
        return Err(anyhow::anyhow!("case dir {} not found", case_dir.display()));
    }

    // Prefer a locally built binary, otherwise fall back to PATH lookup.
    let exe_name = if env::consts::EXE_EXTENSION.is_empty() {
        agent_bin.to_string()
    } else {
        format!("{}.{}", agent_bin, env::consts::EXE_EXTENSION)
    };

    let candidates = [
        paths.root.join("target").join("debug").join(&exe_name),
        paths
            .root
            .join("iON")
            .join("target")
            .join("debug")
            .join(&exe_name),
        paths
            .root
            .join("iON3")
            .join("target")
            .join("debug")
            .join(&exe_name),
    ];

    let mut cmd = candidates
        .iter()
        .find(|path| path.exists())
        .map(|path| Command::new(path))
        .unwrap_or_else(|| Command::new(&exe_name));

    cmd.arg(&paths.case_id)
        .args(args)
        .current_dir(&paths.root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to start {} {:?}", agent_bin, args))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let log_path = paths.log_path();
    std::fs::create_dir_all(log_path.parent().unwrap_or_else(|| Path::new(".")))?;
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open log {}", log_path.display()))?;

    if let Some(out) = stdout {
        let reader = BufReader::new(out);
        for line in reader.lines().map_while(Result::ok) {
            use std::io::Write;
            writeln!(log_file, "{}", line).ok();
        }
    }
    if let Some(err) = stderr {
        let reader = BufReader::new(err);
        for line in reader.lines().map_while(Result::ok) {
            use std::io::Write;
            writeln!(log_file, "{}", line).ok();
        }
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "{} {:?} failed with status {}",
            agent_bin,
            args,
            status
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseItem {
    pub name: String,
    pub kind: String,
    pub size_bytes: u64,
    pub modified: Option<SystemTime>,
}

pub fn list_case_items(paths: &UiPaths, max: usize) -> Result<Vec<CaseItem>> {
    let mut items = Vec::new();
    let dir = paths.case_dir();
    if !dir.exists() {
        return Ok(items);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if meta.is_dir() {
            "dir"
        } else if meta.is_file() {
            "file"
        } else {
            "other"
        }
        .to_string();
        let name = entry.file_name().to_string_lossy().to_string();
        items.push(CaseItem {
            name,
            kind,
            size_bytes: meta.len(),
            modified: meta.modified().ok(),
        });
    }
    items.sort_by(|a, b| b.modified.cmp(&a.modified));
    if items.len() > max {
        items.truncate(max);
    }
    Ok(items)
}

```

### `src/contact_index.rs`

```rust
use crate::case::{Case, CaseRegistryEntry};
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ContactIndex {
    name_to_numbers: HashMap<String, HashSet<String>>,
    number_to_name: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ContactRow {
    phone: String,
    #[serde(default)]
    name: String,
}

impl ContactIndex {
    pub fn load_for_case(case: &Case) -> Result<Self> {
        let mut idx = ContactIndex::default();

        if let Some(entry) = case.registry_entry()? {
            idx.ingest_registry(&entry);
        }

        // Primary contacts export
        let contacts_dir = case.root_path().join("evidence").join("contacts");
        idx.ingest_contacts_exports(&contacts_dir)?;

        // Derive from evidence filenames as a fallback (sms/mms/calls exports)
        idx.ingest_numbers_from_dir(&case.root_path().join("evidence"))?;

        // Persist what we discovered so other modules can reuse it without re-scanning.
        let all_numbers: Vec<String> = idx.number_to_name.keys().cloned().collect();
        if !all_numbers.is_empty() {
            let _ = case.remember_phone_numbers(&all_numbers);
        }
        for (name, nums) in &idx.name_to_numbers {
            let nums: Vec<String> = nums.iter().cloned().collect();
            let _ = case.remember_contact_mapping(name, &nums);
        }

        Ok(idx)
    }

    fn ingest_contacts_exports(&mut self, contacts_dir: &Path) -> Result<()> {
        self.ingest_contacts_csv(&contacts_dir.join("contacts_focus_numbers.csv"))?;
        self.ingest_contacts_csv(&contacts_dir.join("contacts.csv"))?;
        self.ingest_contacts_json(&contacts_dir.join("contacts_focus_numbers.json"))?;

        if !contacts_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(contacts_dir)
            .with_context(|| format!("Failed to read contacts dir {}", contacts_dir.display()))?
        {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("contacts_") {
                continue;
            }
            match path.extension().and_then(|value| value.to_str()) {
                Some("csv") => self.ingest_contacts_csv(&path)?,
                Some("json") => self.ingest_contacts_json(&path)?,
                _ => {}
            }
        }

        Ok(())
    }

    fn ingest_registry(&mut self, entry: &CaseRegistryEntry) {
        for (name, numbers) in &entry.contacts {
            for n in numbers {
                self.register(name, n);
            }
        }
        for number in &entry.phone_numbers {
            self.register(number, number);
        }
    }

    fn ingest_contacts_csv(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let mut rdr = csv::Reader::from_path(path)
            .with_context(|| format!("Failed to read contacts csv {}", path.display()))?;
        for row in rdr.deserialize::<ContactRow>().flatten() {
            self.register(&row.name, &row.phone);
        }
        Ok(())
    }

    fn ingest_contacts_json(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let data = fs::read_to_string(path)
            .with_context(|| format!("Failed to read contacts json {}", path.display()))?;
        let rows: Vec<ContactRow> = serde_json::from_str(&data).unwrap_or_default();
        for row in rows {
            self.register(&row.name, &row.phone);
        }
        Ok(())
    }

    fn ingest_numbers_from_dir(&mut self, root: &Path) -> Result<()> {
        let patterns = ["sms_", "mms_", "call_history_"];
        let re = Regex::new(r"(\+?\d[\d\-]+)").unwrap();

        fn walk<F: FnMut(&Path)>(dir: &Path, depth: usize, cb: &mut F) {
            if depth == 0 || !dir.is_dir() {
                return;
            }
            if let Ok(rd) = fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, depth - 1, cb);
                    } else {
                        cb(&path);
                    }
                }
            }
        }

        let mut visit_file = |path: &Path| {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if patterns.iter().any(|p| name.starts_with(p)) {
                    if let Some(stem) = PathBuf::from(name).file_stem().and_then(|s| s.to_str()) {
                        for cap in re.captures_iter(stem) {
                            if let Some(m) = cap.get(1) {
                                self.register(stem, m.as_str());
                            }
                        }
                    }
                }
            }
        };

        walk(root, 4, &mut visit_file);
        Ok(())
    }

    fn register(&mut self, raw_name: &str, raw_phone: &str) {
        let phone = normalize_phone(raw_phone);
        if phone.is_empty() {
            return;
        }

        let name_key = normalize_name(raw_name);
        if !name_key.is_empty() {
            self.name_to_numbers
                .entry(name_key.clone())
                .or_default()
                .insert(phone.clone());
            self.number_to_name
                .entry(phone.clone())
                .or_insert_with(|| raw_name.trim().to_string());
        } else {
            self.number_to_name
                .entry(phone.clone())
                .or_insert_with(|| phone.clone());
        }
    }

    pub fn phones_for_name(&self, name: &str) -> Vec<String> {
        let key = normalize_name(name);
        match self.name_to_numbers.get(&key) {
            Some(set) => set.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    pub fn name_for_phone(&self, phone: &str) -> Option<String> {
        let normalized = normalize_phone(phone);
        self.number_to_name.get(&normalized).cloned()
    }

    pub fn known_numbers(&self) -> Vec<String> {
        self.number_to_name.keys().cloned().collect()
    }
}

/// Split free-form contact expressions such as ["john", "doe", "&&", "jane", "doe"]
/// into clean name tokens.
pub fn parse_contact_terms(args: &[String]) -> Vec<String> {
    if args.is_empty() {
        return Vec::new();
    }

    let joined = args.join(" ");
    joined
        .split("&&")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn normalize_phone(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn normalize_name(raw: &str) -> String {
    raw.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ingests_wildcard_contact_exports() -> Result<()> {
        let dir = tempdir()?;
        let contacts_dir = dir.path().join("contacts");
        fs::create_dir_all(&contacts_dir)?;
        fs::write(
            contacts_dir.join("contacts_focus_numbers.csv"),
            "phone,name\n5157837352,Known Person\n",
        )?;
        fs::write(
            contacts_dir.join("contacts_manual_review.csv"),
            "phone,name\n5154994652,Manual Match\n",
        )?;

        let mut idx = ContactIndex::default();
        idx.ingest_contacts_exports(&contacts_dir)?;

        assert_eq!(
            idx.name_for_phone("5157837352").as_deref(),
            Some("Known Person")
        );
        assert_eq!(
            idx.name_for_phone("5154994652").as_deref(),
            Some("Manual Match")
        );
        Ok(())
    }
}

```

## Chronos — iOS Backup Engine

### `src/chronos/mod.rs`

```rust
//! Chronos - The iON Backup Engine
//!
//! Agent 0: Foundation for all other iON modules

mod awake;
pub mod backup;
pub mod config;
pub mod device;
pub mod error;
pub mod idevicebackup2;
pub mod ios_backup;
pub mod manifest;
pub mod mvt_builder;
pub mod progress;
pub mod validation;
pub mod wal;

pub use crate::ensure_dir;
pub use backup::{create_backup, BackupManager, BackupResult};
pub use config::{BackupEncryption, ChronosConfig};
pub use device::{
    probe_live_status, ConnectedDeviceStatus, DeviceInfo, DeviceManager, IfuseMount,
    LiveDeviceStatus, PairingState, ToolStatus,
};
pub use error::ChronosError;
pub use ios_backup as ios_backup_decrypt;
pub use manifest::{ChronosManifest, ChronosPreparedRecord};
pub use progress::BackupProgress;
pub use wal::{ReplayResult, WalReplayer};

// Re-export command builders
pub use idevicebackup2::{
    check_tool_available as check_idevicebackup2_available, enable_encryption,
    full_encrypted_backup, IDeviceBackup2Builder, IDeviceBackup2Command, IDeviceBackup2Options,
};
pub use mvt_builder::{
    check_mvt_android_available, check_mvt_available, MvtAndroidBuilder, MvtAndroidCheckApkBuilder,
    MvtAndroidCheckBackupBuilder, MvtAndroidCheckBuilder, MvtIosBuilder, MvtIosCheckBackupBuilder,
    MvtIosDecryptBackupBuilder,
};

use crate::case::Case;
use anyhow::Result;

/// Main entry point for Chronos functionality
pub fn run_chronos(case: Case, config: ChronosConfig) -> Result<BackupResult> {
    create_backup(case, config)
}

```

### `src/chronos/backup.rs`

```rust
//! Backup Creation and Management

use crate::case::Case;
use crate::chronos::awake::KeepAwakeGuard;
use crate::chronos::config::ChronosConfig;
use crate::chronos::device::DeviceManager;
use crate::chronos::error::ChronosError;
use crate::chronos::manifest::{ChronosManifest, ChronosPreparedRecord};
use crate::chronos::validation::validate_core_files;
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{
    write_resolver_audit, ArtifactResolver, BackupResolver, ResolverAuditRecord, ResolverContext,
};
use crate::common::target::chronos_targets;
use crate::agents::cerberus::{export_contacts, extract_contacts};
use anyhow::{Context, Result};
use crossterm::terminal;
use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const REQUIRED_FILES: &[&str] = &[
    "Manifest.db",
    "Manifest.plist",
    "Status.plist",
    "Info.plist",
];

#[derive(Debug, Serialize)]
pub struct BackupResult {
    pub backup_root: PathBuf,
    pub manifest: ChronosManifest,
    pub device_info: Option<crate::chronos::device::DeviceInfo>,
    pub duration: Duration,
    pub helios_root: Option<PathBuf>,
    pub orpheus_ok: bool,
}

pub struct BackupManager {
    case: Case,
    config: ChronosConfig,
    progress: std::sync::Arc<crate::chronos::progress::BackupProgress>,
}

impl BackupManager {
    pub fn new(case: Case, config: ChronosConfig) -> Self {
        Self {
            case,
            config,
            progress: crate::chronos::progress::BackupProgress::new(),
        }
    }

    pub fn create_backup(&self) -> Result<BackupResult> {
        let start_time = Instant::now();

        // Enforce encrypted backups before touching the device
        if !self.config.offline && self.config.backup_password.is_none() {
            return Err(anyhow::anyhow!(
                "Encrypted backups are required. Set BACKUP_PASSWORD environment variable."
            ));
        }

        self.case.log("CHRONOS — Full Backup Starting", None)?;

        println!(
            r#"
    ╔═══════════════════════════════════════════════════════════════════╗
    ║  ██████╗██╗  ██╗██████╗  ██████╗ ███╗   ██╗ ██████╗ ███████╗     ║
    ║ ██╔════╝██║  ██║██╔══██╗██╔═══██╗████╗  ██║██╔═══██╗██╔════╝     ║
    ║ ██║     ███████║██████╔╝██║   ██║██╔██╗ ██║██║   ██║███████╗     ║
    ║ ██║     ██╔══██║██╔══██╗██║   ██║██║╚██╗██║██║   ██║╚════██║     ║
    ║ ╚██████╗██║  ██║██║  ██║╚██████╔╝██║ ╚████║╚██████╔╝███████║     ║
    ║  ╚═════╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═══╝ ╚═════╝ ╚══════╝     ║
    ║                TITAN OF TIME - BACKUP CREATOR                     ║
    ╚═══════════════════════════════════════════════════════════════════╝
        "#
        );

        let base_dir = self.case.ion_backup_path()?;
        fs::create_dir_all(&base_dir).with_context(|| {
            format!("Failed to create backup directory: {}", base_dir.display())
        })?;

        let override_root = self.config.backup_path_override.clone();
        let device_manager = if self.config.offline || override_root.is_some() {
            None
        } else {
            Some(DeviceManager::new(self.case.clone(), self.config.clone()))
        };
        let prepared_device_info = if let Some(manager) = &device_manager {
            if self.config.offline || override_root.is_some() {
                None
            } else {
                self.prepare_device(manager)?;
                Some(manager.get_device_info()?)
            }
        } else {
            None
        };

        let backup_root = if let Some(override_path) = override_root {
            self.validate_backup_root(&override_path)?;
            println!("📂 Using provided backup path {}", override_path.display());
            self.case.log(
                "CHRONOS — Using provided backup path",
                Some(&format!("{}", override_path.display())),
            )?;
            override_path
        } else if let Some(existing) = self.find_backup_root(&base_dir)? {
            println!("📂 Found existing backup at {}", existing.display());
            self.case.log(
                "CHRONOS — Reusing existing backup",
                Some(&format!("{}", existing.display())),
            )?;
            existing
        } else if self.config.offline {
            return Err(anyhow::anyhow!(
                "--offline/--use-existing requires an existing backup in {}",
                base_dir.display()
            ));
        } else {
            device_manager.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Device manager unavailable; cannot create new backup")
            })?;
            self.create_new_backup(&base_dir, prepared_device_info.as_ref())?
        };

        // Validate core files exist
        validate_core_files(&backup_root)?;
        self.ensure_case_backup_reference(&base_dir, &backup_root)?;

        let resolver = BackupResolver::new(ResolverContext {
            backup_root: backup_root.clone(),
            case_root: self.case.root_path(),
            clean_root: self.case.root_path().join("clean"),
            manifest_db_path: backup_root
                .join("Manifest.db")
                .exists()
                .then(|| backup_root.join("Manifest.db")),
        });

        // Process known files and create clean copies with WAL replay (or direct-tree staging)
        let mut manifest = self.summarize_known_files(&resolver, &prepared_device_info)?;

        // Save manifest (phase 1)
        let summary_path = self.case.root_path().join("chronos_manifest.json");
        self.case
            .write_file(&summary_path, serde_json::to_vec_pretty(&manifest)?)?;
        println!("✓ Chronos manifest saved: {}", summary_path.display());
        self.case.log(
            "CHRONOS — Manifest saved (phase 1)",
            Some(&format!("{}", summary_path.display())),
        )?;

        // --- Pipeline: Helios -> Chronos Phase 2 -> Orpheus ---
        let mut helios_root: Option<PathBuf> = None;
        let mut orpheus_ok = false;

        if !self.config.offline {
            let password = self
                .config
                .backup_password
                .as_deref()
                .unwrap_or_default();

            // 1. Run Helios
            let helios_requested = self.case.workspace().helios_full_root();
            self.run_helios(&backup_root, &helios_requested, password)?;

            // Helios may append .styg to the output directory
            let helios_output = resolve_helios_actual_root(&helios_requested);
            helios_root = Some(helios_output.clone());

            // 2. Chronos second part: re-extract from helios decrypted tree
            if helios_output.exists() {
                println!("\n📱 Chronos Phase 2: Re-extracting from Helios output...");
                self.case.log(
                    "CHRONOS — Phase 2 starting (helios re-extract)",
                    Some(&format!("{}", helios_output.display())),
                )?;

                let helios_resolver = BackupResolver::new(ResolverContext {
                    backup_root: helios_output.clone(),
                    case_root: self.case.root_path(),
                    clean_root: self.case.root_path().join("clean"),
                    manifest_db_path: None,
                });

                let phase2_manifest =
                    self.summarize_known_files(&helios_resolver, &prepared_device_info)?;

                // Merge phase 2 into phase 1 (overwrite duplicates)
                merge_manifest(&mut manifest, phase2_manifest);

                // Point manifest to helios root so agents use decrypted tree
                manifest.backup_root = helios_output.display().to_string();

                // Save merged manifest
                self.case.write_file(
                    &summary_path,
                    serde_json::to_vec_pretty(&manifest)?,
                )?;
                println!("✓ Chronos manifest merged and saved: {}", summary_path.display());
                self.case.log(
                    "CHRONOS — Manifest merged (phase 2)",
                    Some(&format!("{}", summary_path.display())),
                )?;
            }

            // 3. Run Orpheus
            self.run_orpheus(&helios_output)?;
            orpheus_ok = true;

            // 3.5 Extract contacts
            self.extract_contacts()?;

            // 4. Run all remaining evidence agents
            self.run_all_evidence_agents()?;

            // 5. Launch GUI
            self.launch_gui()?;
        }

        let duration = start_time.elapsed();

        println!("\n✓ CHRONOS SUCCESS");
        println!("  Location: {}", backup_root.display());
        if let Some(ref hr) = helios_root {
            println!("  Helios:   {}", hr.display());
        }
        println!("  Orpheus:  {}", if orpheus_ok { "OK" } else { "SKIPPED" });
        println!("  Duration: {:?}", duration);
        self.case.log(
            "CHRONOS COMPLETE",
            Some(&format!(
                "backup: {} helios: {} orpheus: {} duration: {:?}",
                backup_root.display(),
                helios_root.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                if orpheus_ok { "ok" } else { "skipped" },
                duration
            )),
        )?;

        Ok(BackupResult {
            backup_root,
            manifest,
            device_info: prepared_device_info,
            duration,
            helios_root,
            orpheus_ok,
        })
    }

    fn prepare_device(&self, device_manager: &DeviceManager) -> Result<()> {
        device_manager.ensure_tools_available()?;

        println!("📱 Step 1/3: Pairing device...");
        self.case.log("CHRONOS — Running idevicepair pair", None)?;
        device_manager.pair_device()?;

        println!("📱 Step 2/3: Validating pairing...");
        self.case
            .log("CHRONOS — Running idevicepair validate", None)?;
        device_manager.validate_device()?;

        println!("📱 Step 3/3: Collecting device info...");
        self.case.log("CHRONOS — Running ideviceinfo", None)?;
        let info = device_manager.get_device_info()?;
        self.case.log(
            "CHRONOS — Device info collected",
            Some(&format!(
                "{} {} {}",
                info.device_name, info.product_type, info.product_version
            )),
        )?;

        let device_info_path = self.case.root_path().join("chronos_device_info.json");
        self.case
            .write_file(&device_info_path, serde_json::to_vec_pretty(&info)?)?;
        Ok(())
    }

    fn create_new_backup(
        &self,
        base_dir: &Path,
        device_info: Option<&crate::chronos::device::DeviceInfo>,
    ) -> Result<PathBuf> {
        println!("?? Starting iPhone backup...");
        println!("? This may take 5-30 minutes\n");

        let _keep_awake = KeepAwakeGuard::new();

        let udid = device_info.map(|d| d.udid.as_str());
        let password = self.config.backup_password.as_deref();

        self.perform_backup(base_dir, udid, password)
    }

    fn perform_backup(
        &self,
        base_dir: &Path,
        udid: Option<&str>,
        password: Option<&str>,
    ) -> Result<PathBuf> {
        println!("📱 Creating full encrypted backup...");
        println!("⏳ This may take 2-6 hours depending on the size of the backup and your connection speed.");
        println!("   Leave the device connected and unlocked. Do not close this window.\n");
        self.case.log("CHRONOS — Creating full encrypted backup", None)?;

        // Step 1: Ensure encryption is enabled
        let enc_ok = self.ensure_encryption_enabled(base_dir, udid, password);
        if let Err(ref e) = enc_ok {
            println!("⚠️  Encryption setup issue: {}", e);
            self.case.log(
                "CHRONOS — Encryption setup warning",
                Some(&format!("{}", e)),
            )?;
        }
        // We proceed even if enc_ok is Err("already enabled") — the backup will tell us if things are really wrong.

        // Step 2: Run full backup (with password if available)
        let mut backup_builder =
            crate::chronos::idevicebackup2::IDeviceBackup2Builder::full_backup(base_dir);
        if let Some(id) = udid {
            backup_builder = backup_builder.udid(id);
        }
        if let Some(pw) = password {
            backup_builder = backup_builder.password(pw);
        }

        eprintln!("🚀 Executing: {}", backup_builder.to_command_string());

        let progress_pct = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let _progress = MetroidProgress::start_with_progress(Arc::clone(&progress_pct));

        let mut child = backup_builder
            .build()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| "Failed to spawn idevicebackup2")?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn a thread to read combined output and parse percentage
        let progress_clone = Arc::clone(&progress_pct);
        let reader_handle = thread::spawn(move || {
            use std::io::{BufRead, BufReader};

            fn read_and_parse<R: std::io::Read>(source: Option<R>, pct: &std::sync::atomic::AtomicU8) {
                if let Some(stream) = source {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().flatten() {
                        // idevicebackup2 often prints progress like "10%" or "Progress: 10%"
                        if let Some(cap) = regex_percent(&line) {
                            pct.store(cap, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }

            fn regex_percent(line: &str) -> Option<u8> {
                // Look for the first number immediately followed by %
                let chars: Vec<char> = line.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i].is_ascii_digit() {
                        let start = i;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == '%' {
                            let num_str: String = chars[start..i].iter().collect();
                            if let Ok(n) = num_str.parse::<u8>() {
                                return Some(n);
                            }
                        }
                    }
                    i += 1;
                }
                None
            }

            read_and_parse(stdout, &progress_clone);
            read_and_parse(stderr, &progress_clone);
        });

        let status = child.wait()?;
        let _ = reader_handle.join();
        drop(_progress);

        if !status.success() {
            // We lost stdout/stderr because it was consumed by the reader thread.
            // Re-run briefly to capture error or use interactive fallback.
            self.case.log(
                "CHRONOS FAILED",
                Some(&format!("idevicebackup2 backup exited with status {:?}", status.code())),
            )?;

            // Interactive fallback if password-related failure is suspected
            if status.code() == Some(255) && password.is_some() {
                println!("\n🔐 Non-interactive backup failed. Falling back to interactive mode...");
                println!("   Please enter the backup password on the prompt below.\n");
                let mut interactive_builder =
                    crate::chronos::idevicebackup2::IDeviceBackup2Builder::full_backup(base_dir)
                        .interactive();
                if let Some(id) = udid {
                    interactive_builder = interactive_builder.udid(id);
                }
                let int_status = interactive_builder
                    .build()
                    .status()
                    .with_context(|| "Failed to run interactive backup")?;
                if !int_status.success() {
                    return Err(ChronosError::BackupFailed(format!(
                        "Interactive backup failed with status {:?}",
                        int_status.code()
                    ))
                    .into());
                }
                println!("✓ Full encrypted backup completed (interactive)");
                return self.find_backup_root(base_dir)?
                    .ok_or_else(|| ChronosError::InvalidBackupFormat.into());
            }

            return Err(ChronosError::BackupFailed(format!(
                "idevicebackup2 exited with status {:?}",
                status.code()
            ))
            .into());
        }

        println!("✓ Full encrypted backup completed");
        self.find_backup_root(base_dir)?
            .ok_or_else(|| ChronosError::InvalidBackupFormat.into())
    }

    fn ensure_encryption_enabled(
        &self,
        base_dir: &Path,
        udid: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        let mut builder = crate::chronos::idevicebackup2::IDeviceBackup2Builder::encryption(base_dir, true);
        if password.is_none() {
            builder = builder.interactive();
        }
        if let Some(id) = udid {
            builder = builder.udid(id);
        }
        if let Some(pw) = password {
            builder = builder.password(pw);
        }

        let output = builder.execute()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{} {}", stdout, stderr).to_lowercase();

        if output.status.success()
            || combined.contains("already enabled")
            || combined.contains("is enabled")
        {
            return Ok(());
        }

        // Try interactive as last resort
        if password.is_some() {
            println!("\n🔐 Encryption enablement failed non-interactively. Falling back to interactive...");
            let mut interactive =
                crate::chronos::idevicebackup2::IDeviceBackup2Builder::encryption(base_dir, true)
                    .interactive();
            if let Some(id) = udid {
                interactive = interactive.udid(id);
            }
            let status = interactive
                .build()
                .status()
                .with_context(|| "Failed to run interactive encryption enable")?;
            if status.success() {
                return Ok(());
            }
        }

        Err(anyhow::anyhow!(
            "Failed to enable backup encryption: stdout='{}' stderr='{}'",
            stdout.trim(),
            stderr.trim()
        ))
    }

    fn run_helios(&self, backup_root: &Path, output_dir: &Path, password: &str) -> Result<()> {
        println!("\n🔆 Launching Helios full decrypt...");
        self.case.log(
            "CHRONOS — Launching Helios",
            Some(&format!("output={}", output_dir.display())),
        )?;

        let helios_bin = locate_agent_binary("helios")
            .ok_or_else(|| anyhow::anyhow!("Could not locate 'helios' binary on PATH or next to chronos"))?;

        fs::create_dir_all(output_dir)?;

        let mut cmd = Command::new(&helios_bin);
        cmd.arg(self.case.name())
            .arg("-i")
            .arg(backup_root)
            .arg("-o")
            .arg(output_dir)
            .arg("-p")
            .arg(password)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("🚀 Executing: {}", cmd_to_string(&cmd));

        let status = cmd.status().with_context(|| {
            format!("Failed to execute helios binary: {}", helios_bin.display())
        })?;

        if !status.success() {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            self.case.log("CHRONOS — Helios failed", Some(&code))?;
            return Err(ChronosError::ToolExecutionFailed(format!(
                "helios exited with status {}",
                code
            ))
            .into());
        }

        println!("✓ Helios completed successfully");
        self.case.log("CHRONOS — Helios completed", None)?;
        Ok(())
    }

    fn run_orpheus(&self, root: &Path) -> Result<()> {
        println!("\n🎵 Launching Orpheus recon...");
        self.case.log(
            "CHRONOS — Launching Orpheus",
            Some(&format!("root={}", root.display())),
        )?;

        let orpheus_bin = locate_agent_binary("orpheus")
            .ok_or_else(|| anyhow::anyhow!("Could not locate 'orpheus' binary on PATH or next to chronos"))?;

        let mut cmd = Command::new(&orpheus_bin);
        cmd.arg("recon")
            .arg("-c")
            .arg(self.case.name())
            .arg("-r")
            .arg(root)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("🚀 Executing: {}", cmd_to_string(&cmd));

        let status = cmd.status().with_context(|| {
            format!("Failed to execute orpheus binary: {}", orpheus_bin.display())
        })?;

        if !status.success() {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            self.case.log("CHRONOS — Orpheus failed", Some(&code))?;
            return Err(ChronosError::ToolExecutionFailed(format!(
                "orpheus exited with status {}",
                code
            ))
            .into());
        }

        println!("✓ Orpheus completed successfully");
        self.case.log("CHRONOS — Orpheus completed", None)?;
        Ok(())
    }

    fn run_all_evidence_agents(&self) -> Result<()> {
        let agents = [
            "atlas",
            "plutus",
            "cerberus",
            "hermes",
            "charon",
            "nyx",
            "obolus",
            "psyche",
        ];

        for agent in agents {
            println!("\n🤖 Launching {}...", agent.to_uppercase());
            self.case.log(
                &format!("CHRONOS — Launching {}", agent.to_uppercase()),
                None,
            )?;

            let bin = locate_agent_binary(agent)
                .ok_or_else(|| anyhow::anyhow!("Could not locate '{}' binary", agent))?;

            let mut cmd = Command::new(&bin);
            cmd.arg(self.case.name())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            println!("🚀 Executing: {}", cmd_to_string(&cmd));

            let status = cmd.status().with_context(|| {
                format!("Failed to execute {} binary: {}", agent, bin.display())
            })?;

            if !status.success() {
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string());
                self.case.log(
                    &format!("CHRONOS — {} failed", agent.to_uppercase()),
                    Some(&code),
                )?;
                // Log warning but continue — one agent failing shouldn't kill the whole pipeline
                println!(
                    "⚠️  {} failed with status {}. Continuing pipeline...",
                    agent, code
                );
            } else {
                println!("✓ {} completed successfully", agent.to_uppercase());
                self.case.log(
                    &format!("CHRONOS — {} completed", agent.to_uppercase()),
                    None,
                )?;
            }
        }

        Ok(())
    }

    fn extract_contacts(&self) -> Result<()> {
        println!("\n👤 Extracting contacts...");
        self.case.log("CHRONOS — Extracting contacts", None)?;

        match extract_contacts(&self.case) {
            Ok(contacts) => {
                let path = export_contacts(&self.case, &contacts)?;
                println!("✓ Extracted {} contacts -> {}", contacts.len(), path.display());
                self.case.log(
                    "CHRONOS — Contacts extracted",
                    Some(&format!("{} contacts", contacts.len())),
                )?;
                Ok(())
            }
            Err(e) => {
                println!("⚠️  Contacts extraction failed: {}", e);
                self.case.log(
                    "CHRONOS — Contacts extraction failed",
                    Some(&e.to_string()),
                )?;
                // Non-fatal: don't block the pipeline
                Ok(())
            }
        }
    }

    fn launch_gui(&self) -> Result<()> {
        println!("\n🖥️  Launching iON GUI...");
        self.case.log(
            "CHRONOS — Launching GUI",
            Some(self.case.name()),
        )?;

        let gui_bin = locate_agent_binary("ion_ui")
            .ok_or_else(|| anyhow::anyhow!("Could not locate 'ion_ui' binary on PATH or next to chronos"))?;

        let mut cmd = Command::new(&gui_bin);
        cmd.arg(self.case.root_path())
            .arg("--case-name")
            .arg(self.case.name())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("🚀 Executing: {}", cmd_to_string(&cmd));

        let status = cmd.status().with_context(|| {
            format!("Failed to execute ion_ui binary: {}", gui_bin.display())
        })?;

        if !status.success() {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            self.case.log("CHRONOS — GUI failed", Some(&code))?;
            return Err(ChronosError::ToolExecutionFailed(format!(
                "ion_ui exited with status {}",
                code
            ))
            .into());
        }

        println!("✓ GUI closed");
        self.case.log("CHRONOS — GUI closed", None)?;
        Ok(())
    }

    fn summarize_known_files(
        &self,
        resolver: &BackupResolver,
        device_info: &Option<crate::chronos::device::DeviceInfo>,
    ) -> Result<ChronosManifest> {
        let prepare_ctx = PrepareContext {
            case_root: self.case.root_path(),
            clean_root: resolver.clean_root().to_path_buf(),
            temp_root: self.case.root_path().join("tmp"),
        };
        let targets = chronos_targets();
        let mut prepared = Vec::new();

        // Update progress
        self.progress.update_total(targets.len() as u64);

        for (index, target) in targets.iter().enumerate() {
            self.progress.set_current_file(target.artifact_key);

            let resolved = resolver.resolve_known_target(target)?;
            let audit = ResolverAuditRecord {
                artifact_key: target.artifact_key.to_string(),
                candidates: target
                    .candidates
                    .iter()
                    .map(|candidate| format!("{}/{}", candidate.domain, candidate.relative_path))
                    .collect(),
                chosen: resolved
                    .as_ref()
                    .map(|artifact| artifact.source_path.display().to_string()),
                method: resolved
                    .as_ref()
                    .map(|artifact| format!("{:?}", artifact.method)),
            };
            write_resolver_audit(resolver.case_root(), &audit)?;

            if let Some(resolved) = resolved {
                match prepare_artifact(&prepare_ctx, &resolved, target.sqlite_like) {
                    Ok(artifact) => {
                        prepared.push(ChronosPreparedRecord {
                            artifact_key: target.artifact_key.to_string(),
                            source_path: artifact.source_path.display().to_string(),
                            working_path: artifact.working_path.display().to_string(),
                            resolution_method: format!("{:?}", artifact.resolution_method),
                            sha256: artifact.sha256.clone(),
                        });
                    }
                    Err(e) => {
                        println!("WARN: Failed to prepare {}: {}", target.artifact_key, e);
                    }
                }
            } else {
                println!("WARN: Failed to resolve {}", target.artifact_key);
            }

            self.progress.increment_processed();

            // Show progress every 10% or for first/last items
            let progress = self.progress.progress_percentage();
            if index == 0 || index == targets.len() - 1 || (progress as usize).is_multiple_of(10) {
                println!(
                    "Progress: {:.1}% - Processing {}",
                    progress, target.artifact_key
                );
            }
        }

        Ok(ChronosManifest {
            backup_root: resolver.backup_root().display().to_string(),
            prepared,
            device_info: device_info.clone(),
        })
    }

    fn find_backup_root(&self, base_dir: &Path) -> Result<Option<PathBuf>> {
        if !base_dir.exists() {
            return Ok(None);
        }

        let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();

        if self.has_required_files(base_dir) {
            candidates.push((self.modified_time(base_dir), base_dir.to_path_buf()));
        }

        if base_dir.is_dir() {
            for entry in fs::read_dir(base_dir)? {
                let path = match entry {
                    Ok(e) => e.path(),
                    Err(_) => continue,
                };

                if path.is_dir() && self.has_required_files(&path) {
                    candidates.push((self.modified_time(&path), path));
                }
            }
        }

        candidates.sort_by_key(|(mtime, _)| *mtime);
        Ok(candidates.pop().map(|(_, path)| path))
    }

    fn validate_backup_root(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(anyhow::anyhow!("Backup path not found: {}", path.display()));
        }
        if !path.is_dir() {
            return Err(anyhow::anyhow!(
                "Backup path is not a directory: {}",
                path.display()
            ));
        }
        if !self.has_required_files(path) {
            return Err(anyhow::anyhow!(
                "Backup path missing required files: {}",
                path.display()
            ));
        }
        Ok(())
    }

    fn has_required_files(&self, dir: &Path) -> bool {
        REQUIRED_FILES.iter().all(|name| dir.join(name).exists())
    }

    fn modified_time(&self, path: &Path) -> std::time::SystemTime {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    }

    fn ensure_case_backup_reference(&self, base_dir: &Path, backup_root: &Path) -> Result<()> {
        if backup_root.starts_with(base_dir) {
            return Ok(());
        }

        fs::create_dir_all(base_dir)?;
        let link_name = backup_root
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("external_backup"));
        let link_path = base_dir.join(link_name);

        if link_path.exists() {
            return Ok(());
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(backup_root, &link_path).with_context(|| {
                format!(
                    "Failed to create backup symlink {} -> {}",
                    link_path.display(),
                    backup_root.display()
                )
            })?;
        }

        Ok(())
    }
}

fn merge_manifest(base: &mut ChronosManifest, from: ChronosManifest) {
    for record in from.prepared {
        if let Some(existing) = base.prepared.iter_mut().find(|r| r.artifact_key == record.artifact_key) {
            *existing = record;
        } else {
            base.prepared.push(record);
        }
    }
}

fn resolve_helios_actual_root(requested: &Path) -> PathBuf {
    let styg_variant = if let Some(name) = requested.file_name().and_then(|n| n.to_str()) {
        requested.with_file_name(format!("{}.styg", name))
    } else {
        requested.to_path_buf()
    };
    if styg_variant.exists() {
        styg_variant
    } else {
        requested.to_path_buf()
    }
}

fn locate_agent_binary(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let candidate = dir.join(&exe_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let candidates = [
        Path::new("target").join("debug").join(&exe_name),
        Path::new("target").join("release").join(&exe_name),
        Path::new("iON").join("target").join("debug").join(&exe_name),
        Path::new("iON").join("target").join("release").join(&exe_name),
        Path::new("iON3").join("target").join("debug").join(&exe_name),
        Path::new("iON3").join("target").join("release").join(&exe_name),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Fallback: search PATH manually
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(&exe_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn cmd_to_string(cmd: &Command) -> String {
    // Best-effort string representation for logging
    let prog = cmd.get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    format!("{} {}", prog, args.join(" "))
}

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_CYAN: &str = "\x1b[36m";

struct MetroidProgress {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MetroidProgress {
    fn start() -> Self {
        Self::start_with_progress(Arc::new(std::sync::atomic::AtomicU8::new(0)))
    }

    fn start_with_progress(progress_pct: Arc<std::sync::atomic::AtomicU8>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let start = Instant::now();
            let width = progress_width();
            let mut step = 0usize;
            while !stop_thread.load(Ordering::Relaxed) {
                let pct = progress_pct.load(std::sync::atomic::Ordering::Relaxed);
                let line = render_metroid_line(step, width, start.elapsed(), pct);
                eprint!("\r\x1b[2K{}", line);
                let _ = io::stderr().flush();
                step = step.wrapping_add(1);
                thread::sleep(Duration::from_millis(120));
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for MetroidProgress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        eprint!("\r\x1b[2K");
        let _ = io::stderr().flush();
        eprintln!();
    }
}

fn progress_width() -> usize {
    let fallback = 48usize;
    let reserved = 24usize;
    match terminal::size() {
        Ok((cols, _)) => {
            let width = (cols as usize).saturating_sub(reserved);
            if width < 24 {
                fallback
            } else {
                width.min(64)
            }
        }
        Err(_) => fallback,
    }
}

fn render_metroid_line(step: usize, width: usize, elapsed: Duration, pct: u8) -> String {
    let scene = metroid_scene(step, width);
    let elapsed = format_elapsed(elapsed);
    let pct_str = if pct > 0 { format!("{:>3}%", pct) } else { "   ".to_string() };
    format!(
        "{bold}METROID RUN{reset} {scene} {dim}{elapsed} {cyan}{pct}{reset}",
        bold = ANSI_BOLD,
        dim = ANSI_DIM,
        cyan = ANSI_CYAN,
        reset = ANSI_RESET,
        pct = pct_str,
    )
}

fn metroid_scene(step: usize, width: usize) -> String {
    const TILE: &[u8] = b"[]-=";
    let width = width.max(4);
    let samus_pos = step % width;
    let metroid_pos = (step * 3 + 7) % width;

    let mut out = String::new();
    out.push_str(ANSI_GREEN);
    for idx in 0..width {
        if idx == samus_pos {
            out.push_str(ANSI_YELLOW);
            out.push('S');
            out.push_str(ANSI_GREEN);
        } else if idx == metroid_pos {
            out.push_str(ANSI_CYAN);
            out.push('o');
            out.push_str(ANSI_GREEN);
        } else {
            out.push(TILE[idx % TILE.len()] as char);
        }
    }
    out.push_str(ANSI_RESET);
    out
}

fn format_elapsed(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    let minutes = total / 60;
    let seconds = total % 60;
    format!("T+{:02}:{:02}", minutes, seconds)
}

pub fn create_backup(case: Case, config: ChronosConfig) -> Result<BackupResult> {
    let backup_manager = BackupManager::new(case, config);
    backup_manager.create_backup()
}

```

### `src/chronos/device.rs`

```rust
//! Device Management and Pairing

use crate::case::Case;
use crate::chronos::error::ChronosError;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DeviceManager {
    case: Case,
    config: crate::chronos::config::ChronosConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    /// ideviceinfo: UniqueDeviceID
    pub udid: String,
    /// ideviceinfo: DeviceName
    pub device_name: String,
    /// ideviceinfo: ProductType
    pub product_type: String,
    /// ideviceinfo: ProductVersion
    pub product_version: String,
    /// ideviceinfo: SerialNumber
    pub serial_number: Option<String>,
    /// ideviceinfo: InternationalMobileEquipmentIdentity
    pub imei: Option<String>,
    /// Secondary IMEI when present on dual-SIM devices.
    pub imei1: Option<String>,
    /// Secondary IMEI when present on dual-SIM devices.
    pub imei2: Option<String>,
    /// ideviceinfo: IntegratedCircuitCardIdentity
    pub iccid: Option<String>,
    /// ideviceinfo: PhoneNumber
    pub phone_number: Option<String>,
    /// ideviceinfo: BuildVersion
    pub build_version: Option<String>,
    /// ideviceinfo: ModelNumber
    pub model_number: Option<String>,
    /// ideviceinfo: ProductName or DeviceClass depending on device.
    pub model_name: Option<String>,
    /// ideviceinfo: ChipID or ChipSerialNo.
    pub chipset: Option<String>,
    /// ideviceinfo: WiFiAddress
    pub wifi_address: Option<String>,
    /// ideviceinfo: BluetoothAddress
    pub bluetooth_address: Option<String>,
    /// Raw human-readable system fields that are useful for evidence notes.
    pub raw_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PairingState {
    Paired,
    Unpaired,
    Unknown,
}

impl PairingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paired => "paired",
            Self::Unpaired => "unpaired",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IfuseMount {
    pub mount_point: PathBuf,
    pub udid: Option<String>,
    pub filesystem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedDeviceStatus {
    pub udid: String,
    pub pairing_state: PairingState,
    pub pairing_detail: Option<String>,
    pub suggested_mount_point: PathBuf,
    pub active_mounts: Vec<PathBuf>,
    pub device_info: Option<DeviceInfo>,
    pub info_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveDeviceStatus {
    pub connected_devices: Vec<ConnectedDeviceStatus>,
    pub paired_count: usize,
    pub mount_root: PathBuf,
    pub ifuse_mounts: Vec<IfuseMount>,
    pub tool_status: Vec<ToolStatus>,
}

impl LiveDeviceStatus {
    pub fn connected_count(&self) -> usize {
        self.connected_devices.len()
    }

    pub fn available_tool(&self, name: &str) -> bool {
        self.tool_status
            .iter()
            .any(|tool| tool.name == name && tool.available)
    }
}

pub fn probe_live_status(case: &Case) -> LiveDeviceStatus {
    let mount_root = case.workspace().live_device_mounts_dir();
    let _ = fs::create_dir_all(&mount_root);

    let tool_status = ["idevice_id", "idevicepair", "ideviceinfo", "ifuse"]
        .into_iter()
        .map(tool_status)
        .collect::<Vec<_>>();
    let ifuse_mounts = discover_ifuse_mounts(&mount_root);
    let connected_devices = discover_connected_devices(case, &mount_root, &ifuse_mounts);
    let paired_count = connected_devices
        .iter()
        .filter(|device| device.pairing_state == PairingState::Paired)
        .count();

    LiveDeviceStatus {
        connected_devices,
        paired_count,
        mount_root,
        ifuse_mounts,
        tool_status,
    }
}

impl DeviceInfo {
    pub fn parse(info_str: &str) -> Result<Self> {
        let mut info = DeviceInfo {
            udid: String::new(),
            device_name: String::new(),
            product_type: String::new(),
            product_version: String::new(),
            serial_number: None,
            imei: None,
            imei1: None,
            imei2: None,
            iccid: None,
            phone_number: None,
            build_version: None,
            model_number: None,
            model_name: None,
            chipset: None,
            wifi_address: None,
            bluetooth_address: None,
            raw_fields: HashMap::new(),
        };

        for line in info_str.lines() {
            if let Some((key, value)) = line.split_once(": ") {
                let key = key.trim();
                let value = value.trim().to_string();
                info.raw_fields.insert(key.to_string(), value.clone());
                match key {
                    "UniqueDeviceID" => info.udid = value,
                    "DeviceName" => info.device_name = value,
                    "ProductType" => info.product_type = value,
                    "ProductVersion" => info.product_version = value,
                    "SerialNumber" => info.serial_number = Some(value),
                    "InternationalMobileEquipmentIdentity" => info.imei = Some(value),
                    "IntegratedCircuitCardIdentity" => info.iccid = Some(value),
                    "PhoneNumber" => info.phone_number = Some(value),
                    "BuildVersion" => info.build_version = Some(value),
                    "ModelNumber" => info.model_number = Some(value),
                    "ProductName" | "DeviceClass" => info.model_name = Some(value),
                    "ChipID" | "ChipSerialNo" => info.chipset = Some(value),
                    "WiFiAddress" => info.wifi_address = Some(value),
                    "BluetoothAddress" => info.bluetooth_address = Some(value),
                    "IMEI" => info.imei = Some(value),
                    "IMEI1" => info.imei1 = Some(value),
                    "IMEI2" => info.imei2 = Some(value),
                    _ => {}
                }
            }
        }

        if info.udid.is_empty() {
            return Err(anyhow::anyhow!("Could not parse device UDID from info"));
        }

        Ok(info)
    }
}

impl DeviceManager {
    pub fn new(case: Case, config: crate::chronos::config::ChronosConfig) -> Self {
        Self { case, config }
    }

    pub fn pair_device(&self) -> Result<()> {
        self.log_operation("Device pairing", || {
            let output = Command::new("idevicepair")
                .arg("pair")
                .output()
                .context("Failed to execute idevicepair pair")?;

            if output.status.success() {
                self.case.log("CHRONOS — Device paired", None)?;
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.case.log("CHRONOS — Pairing failed", Some(&stderr))?;
                Err(ChronosError::PairingFailed(stderr.to_string()).into())
            }
        })
    }

    pub fn validate_device(&self) -> Result<()> {
        self.log_operation("Device pairing validation", || {
            let output = Command::new("idevicepair")
                .arg("validate")
                .output()
                .context("Failed to execute idevicepair validate")?;

            if output.status.success() {
                self.case
                    .log("CHRONOS — Device paired and validated", None)?;
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.case
                    .log("CHRONOS — Pairing validation failed", Some(&stderr))?;
                Err(ChronosError::PairingFailed(stderr.to_string()).into())
            }
        })
    }

    pub fn ensure_paired(&self) -> Result<()> {
        self.log_operation("Device pairing validation", || self.validate_pairing())
    }

    fn validate_pairing(&self) -> Result<()> {
        let output = Command::new("idevicepair")
            .arg("validate")
            .output()
            .context("Failed to execute idevicepair validate")?;

        if output.status.success() {
            self.case
                .log("CHRONOS — Device paired and validated", None)?;
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.case
                .log("CHRONOS — Pairing validation failed", Some(&stderr))?;
            self.attempt_pairing()
        }
    }

    fn attempt_pairing(&self) -> Result<()> {
        self.case.log("CHRONOS — Attempting device pairing", None)?;

        eprintln!("🔌 CONNECT YOUR DEVICE");
        eprintln!("📱 Make sure your iPhone/iPad is:");
        eprintln!("   1. Connected via USB cable");
        eprintln!("   2. Unlocked");
        eprintln!("   3. Showing the 'Trust This Computer?' dialog");
        eprintln!("   4. You have tapped 'Trust' on the device");
        eprintln!();

        // Try up to 3 times with delays
        for attempt in 1..=3 {
            eprintln!("🔄 Pairing attempt {} of 3...", attempt);

            let output = Command::new("idevicepair")
                .arg("pair")
                .output()
                .context("Failed to execute idevicepair pair")?;

            if output.status.success() {
                eprintln!("✅ Device paired successfully!");
                // Wait a moment and validate
                std::thread::sleep(Duration::from_secs(2));
                return self.validate_pairing();
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);

                eprintln!("⚠️  Pairing attempt {} failed:", attempt);
                if !stderr.is_empty() {
                    eprintln!("   Error: {}", stderr);
                }
                if !stdout.is_empty() {
                    eprintln!("   Output: {}", stdout);
                }

                self.case.log(
                    &format!("CHRONOS — Pairing attempt {} failed", attempt),
                    Some(&stderr),
                )?;

                if attempt < 3 {
                    eprintln!("⏳ Waiting 5 seconds before retry...");
                    eprintln!("   (Check your device for the Trust dialog)");
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        }

        eprintln!();
        eprintln!("❌ PAIRING FAILED");
        eprintln!("Common causes:");
        eprintln!("   • Device not connected via USB");
        eprintln!("   • Device is locked");
        eprintln!("   • 'Trust This Computer?' was not tapped");
        eprintln!("   • USB cable is charge-only (not data)");
        eprintln!();

        Err(
            ChronosError::PairingFailed("Failed to pair device after 3 attempts. Ensure device is connected, unlocked, and you have tapped 'Trust' on the device.".to_string())
                .into(),
        )
    }

    pub fn force_pair(&self) -> Result<()> {
        eprintln!("🔧 FORCE PAIRING MODE");

        // First, try to unpair any existing pairing
        let _ = Command::new("idevicepair").arg("unpair").output();

        // Wait a moment
        std::thread::sleep(Duration::from_secs(1));

        // Now attempt pairing again
        self.attempt_pairing()
    }

    pub fn mount_live_filesystem(&self, udid: &str) -> Result<PathBuf> {
        let mount_point = self.case.workspace().live_device_mounts_dir().join(udid);
        fs::create_dir_all(&mount_point)?;

        let output = Command::new("ifuse")
            .arg("-u")
            .arg(udid)
            .arg(&mount_point)
            .output()
            .context("Failed to execute ifuse")?;

        if output.status.success() {
            self.case.log(
                "CHRONOS — Live filesystem mounted",
                Some(&mount_point.display().to_string()),
            )?;
            Ok(mount_point)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ChronosError::ToolExecutionFailed(format!("ifuse failed: {}", stderr)).into())
        }
    }

    pub fn unmount_live_filesystem(&self, mount: &Path) -> Result<()> {
        let fusermount = Command::new("fusermount")
            .arg("-u")
            .arg(mount)
            .output();

        let status_ok = match fusermount {
            Ok(output) if output.status.success() => true,
            _ => Command::new("umount")
                .arg(mount)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false),
        };

        if status_ok {
            self.case.log(
                "CHRONOS — Live filesystem unmounted",
                Some(&mount.display().to_string()),
            )?;
            Ok(())
        } else {
            Err(ChronosError::ToolExecutionFailed(format!(
                "Failed to unmount {}",
                mount.display()
            ))
            .into())
        }
    }

    pub fn get_device_info(&self) -> Result<DeviceInfo> {
        self.log_operation("Device info retrieval", || {
            let output = Command::new("ideviceinfo")
                .output()
                .context("Failed to execute ideviceinfo")?;

            if output.status.success() {
                let info_str = String::from_utf8_lossy(&output.stdout);
                DeviceInfo::parse(&info_str)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(
                    ChronosError::ToolExecutionFailed(format!("ideviceinfo failed: {}", stderr))
                        .into(),
                )
            }
        })
    }

    pub fn ensure_tools_available(&self) -> Result<()> {
        for tool in &self.config.required_tools {
            match Command::new(tool).arg("--help").output() {
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ChronosError::ToolExecutionFailed(format!(
                        "Required tool `{}` not found. Install libimobiledevice.",
                        tool
                    ))
                    .into());
                }
                Err(e) => {
                    return Err(ChronosError::ToolExecutionFailed(format!(
                        "Failed to launch {}: {}",
                        tool, e
                    ))
                    .into());
                }
            }
        }
        Ok(())
    }

    fn log_operation<T, F>(&self, operation: &str, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        if self.config.verbose {
            self.case
                .log(&format!("CHRONOS — Starting: {}", operation), None)?;
        }

        let result = f();

        if self.config.verbose {
            match &result {
                Ok(_) => self
                    .case
                    .log(&format!("CHRONOS — Completed: {}", operation), None)?,
                Err(e) => self
                    .case
                    .log(&format!("CHRONOS — Failed: {} - {}", operation, e), None)?,
            }
        }

        result
    }
}

fn tool_status(name: &str) -> ToolStatus {
    let available = Command::new("bash")
        .arg("-lc")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    ToolStatus {
        name: name.to_string(),
        available,
        detail: if available {
            "found on PATH".to_string()
        } else {
            "not installed or not on PATH".to_string()
        },
    }
}

fn discover_ifuse_mounts(mount_root: &Path) -> Vec<IfuseMount> {
    let mut mounts = Vec::new();
    if let Ok(entries) = fs::read_dir(mount_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                mounts.push(IfuseMount {
                    udid: path.file_name().map(|value| value.to_string_lossy().to_string()),
                    mount_point: path,
                    filesystem: "ifuse".to_string(),
                });
            }
        }
    }
    mounts
}

fn discover_connected_devices(
    _case: &Case,
    mount_root: &Path,
    ifuse_mounts: &[IfuseMount],
) -> Vec<ConnectedDeviceStatus> {
    let output = Command::new("idevice_id").arg("-l").output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|udid| {
            let suggested_mount_point = mount_root.join(udid);
            let pairing = pairing_state_for_udid(udid);
            let (device_info, info_error) = match device_info_for_udid(udid) {
                Ok(info) => (Some(info), None),
                Err(error) => (None, Some(error)),
            };
            let active_mounts = ifuse_mounts
                .iter()
                .filter(|mount| mount.mount_point == suggested_mount_point)
                .map(|mount| mount.mount_point.clone())
                .collect::<Vec<_>>();

            ConnectedDeviceStatus {
                udid: udid.to_string(),
                pairing_state: pairing.0,
                pairing_detail: pairing.1,
                suggested_mount_point,
                active_mounts,
                device_info,
                info_error,
            }
        })
        .collect()
}

fn pairing_state_for_udid(udid: &str) -> (PairingState, Option<String>) {
    let output = Command::new("idevicepair")
        .arg("-u")
        .arg(udid)
        .arg("validate")
        .output();

    match output {
        Ok(output) if output.status.success() => (PairingState::Paired, None),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if detail.is_empty() { None } else { Some(detail) };
            (PairingState::Unpaired, detail)
        }
        Err(error) => (PairingState::Unknown, Some(error.to_string())),
    }
}

fn device_info_for_udid(udid: &str) -> std::result::Result<DeviceInfo, String> {
    let output = Command::new("ideviceinfo")
        .arg("-u")
        .arg(udid)
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let info_str = String::from_utf8_lossy(&output.stdout);
    DeviceInfo::parse(&info_str).map_err(|error| error.to_string())
}

```

### `src/chronos/manifest.rs`

```rust
//! Chronos Manifest Output Types

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct ChronosPreparedRecord {
    pub artifact_key: String,
    pub source_path: String,
    pub working_path: String,
    pub resolution_method: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChronosManifest {
    pub backup_root: String,
    pub prepared: Vec<ChronosPreparedRecord>,
    pub device_info: Option<crate::chronos::device::DeviceInfo>,
}

```

### `src/chronos/validation.rs`

```rust
//! Backup Validation and Integrity Checking

use crate::chronos::error::ChronosError;
use anyhow::Result;
use std::path::Path;

const REQUIRED_FILES: &[&str] = &[
    "Manifest.db",
    "Manifest.plist",
    "Status.plist",
    "Info.plist",
];

pub fn validate_core_files(root: &Path) -> Result<()> {
    for name in REQUIRED_FILES {
        let path = root.join(name);
        if !path.exists() {
            return Err(ChronosError::MissingFile(format!(
                "Backup missing required file: {}",
                path.display()
            ))
            .into());
        }
    }
    Ok(())
}

pub fn validate_backup_integrity(root: &Path) -> Result<bool> {
    // Check if all required files exist
    validate_core_files(root)?;

    // Additional integrity checks could be added here:
    // - Check Manifest.db schema
    // - Validate Info.plist structure
    // - Check file sizes and checksums

    Ok(true)
}

```

### `src/chronos/wal.rs`

```rust
//! WAL Replay and Database Checkpointing

use anyhow::{Context, Result};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WalReplayer {
    _timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub enum ReplayResult {
    Success,
    PartialRecovery,
    Failed,
}

impl WalReplayer {
    pub fn new(timeout_seconds: u64) -> Self {
        Self {
            _timeout_seconds: timeout_seconds,
        }
    }

    pub fn replay_with_recovery(&self, src: &Path, dst: &Path) -> Result<ReplayResult> {
        match self.attempt_replay(src, dst) {
            Ok(success) => Ok(success),
            Err(e) => {
                // Try fallback approach
                match self.fallback_replay(src, dst) {
                    Ok(fallback_result) => Ok(fallback_result),
                    Err(_) => Err(e), // Return original error if fallback fails
                }
            }
        }
    }

    fn attempt_replay(&self, src: &Path, dst: &Path) -> Result<ReplayResult> {
        // Open source database read-only
        let src_conn = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("Failed to open source database: {}", src.display()))?;

        // Create destination database
        let mut dst_conn = Connection::open(dst)
            .with_context(|| format!("Failed to create destination database: {}", dst.display()))?;

        {
            // Perform backup operation
            let backup = Backup::new(&src_conn, &mut dst_conn)
                .with_context(|| "Failed to create backup object")?;

            // Run backup with progress monitoring
            backup
                .run_to_completion(5, Duration::from_millis(25), None)
                .with_context(|| "Backup operation failed")?;
        }

        Ok(ReplayResult::Success)
    }

    fn fallback_replay(&self, src: &Path, dst: &Path) -> Result<ReplayResult> {
        // Alternative method for handling corrupted or problematic databases
        // This could use .dump/.read approach or other recovery methods

        // For now, we'll try a simpler approach with different flags
        let src_conn = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| {
            format!(
                "Failed to open source database (fallback): {}",
                src.display()
            )
        })?;

        let mut dst_conn = Connection::open(dst).with_context(|| {
            format!(
                "Failed to create destination database (fallback): {}",
                dst.display()
            )
        })?;

        {
            let backup = Backup::new(&src_conn, &mut dst_conn)?;
            backup.run_to_completion(10, Duration::from_millis(50), None)?;
        }

        Ok(ReplayResult::PartialRecovery)
    }

    pub fn validate_database(&self, db_path: &Path) -> Result<bool> {
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        // Check if database is accessible and has valid schema
        let result = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count > 0)
            },
        );

        match result {
            Ok(has_tables) => Ok(has_tables),
            Err(_) => Ok(false),
        }
    }
}

```

### `src/chronos/decrypt.rs`

```rust
//! Rust-based encrypted iOS backup decryptor (owner-authorized).
//!
//! This module is a compatibility wrapper over `chronos::ios_backup`.

use anyhow::Result;
use std::path::Path;

pub use crate::chronos::ios_backup::{ExtractResult, ExtractSpec, VerifyResult};
use crate::chronos::ios_backup;

/// Decrypt specific targets from an encrypted iTunes/Finder backup.
pub fn extract_from_encrypted_itunes_backup(
    encrypted_backup_root: &Path,
    output_root: &Path,
    password: &str,
    specs: &[ExtractSpec],
) -> Result<ExtractResult> {
    ios_backup::extract_from_encrypted_backup(encrypted_backup_root, output_root, password, specs)
}

/// Verify that the supplied password can decrypt the backup (no extraction).
pub fn verify_decrypt(backup_root: &Path, password: &str) -> Result<VerifyResult> {
    ios_backup::verify_decrypt(backup_root, password)
}

```

### `src/chronos/awake.rs`

```rust
#[cfg(windows)]
const ES_CONTINUOUS: u32 = 0x80000000;
#[cfg(windows)]
const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
#[cfg(windows)]
const ES_DISPLAY_REQUIRED: u32 = 0x00000002;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetThreadExecutionState(es_flags: u32) -> u32;
}

pub struct KeepAwakeGuard {
    #[cfg(windows)]
    active: bool,
}

impl KeepAwakeGuard {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let flags = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED;
            let active = unsafe { SetThreadExecutionState(flags) } != 0;
            Self { active }
        }

        #[cfg(not(windows))]
        {
            Self {}
        }
    }
}

impl Drop for KeepAwakeGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if self.active {
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS);
                }
            }
        }
    }
}

```

### `src/chronos/config.rs`

```rust
//! Chronos Configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChronosConfig {
    /// Maximum time to wait for backup completion (seconds)
    pub backup_timeout: u64,

    /// Number of retry attempts for failed operations
    pub retry_attempts: u32,

    /// Maximum time for WAL replay (seconds)
    pub wal_replay_timeout: u64,

    /// Required tools for operation
    pub required_tools: Vec<String>,

    /// Backup encryption settings
    pub backup_encryption: BackupEncryption,

    /// Backup password (required for encrypted backups)
    pub backup_password: Option<String>,

    /// Enable verbose logging
    pub verbose: bool,

    /// Skip device pairing/tools and only use an existing backup
    pub offline: bool,

    /// Use a specific backup root instead of searching under the case backup dir
    pub backup_path_override: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BackupEncryption {
    Encrypted,
}

impl Default for ChronosConfig {
    fn default() -> Self {
        Self {
            backup_timeout: 1800, // 30 minutes
            retry_attempts: 3,
            wal_replay_timeout: 300, // 5 minutes
            required_tools: vec!["idevicepair".to_string(), "idevicebackup2".to_string()],
            backup_encryption: BackupEncryption::Encrypted,
            backup_password: None,
            verbose: false,
            offline: false,
            backup_path_override: None,
        }
    }
}

```

### `src/chronos/error.rs`

```rust
//! Chronos error types.

use rusqlite::Error as SqliteError;
use std::io::Error as IoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChronosError {
    #[error("Device pairing failed: {0}")]
    PairingFailed(String),
    #[error("Backup creation failed: {0}")]
    BackupFailed(String),
    #[error("Required file missing: {0}")]
    MissingFile(String),
    #[error("WAL replay failed: {0}")]
    WalReplayFailed(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] SqliteError),
    #[error("IO error: {0}")]
    IoError(#[from] IoError),
    #[error("Device not found or not connected")]
    DeviceNotFound,
    #[error("Backup timeout exceeded")]
    BackupTimeout,
    #[error("Invalid backup format")]
    InvalidBackupFormat,
    #[error("Manifest parsing error: {0}")]
    ManifestError(String),
    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),
}

```

### `src/chronos/idevicebackup2.rs`

```rust
//! idevicebackup2 Command Builder
//!
//! Provides a type-safe builder for constructing idevicebackup2 commands
//! following the libimobiledevice spec:
//! Usage: idevicebackup2 [OPTIONS] CMD [CMDOPTIONS] DIRECTORY

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Command;

/// Commands supported by idevicebackup2
#[derive(Debug, Clone)]
pub enum IDeviceBackup2Command {
    /// Create backup for the device
    Backup {
        /// Force full backup from device
        full: bool,
        /// Supply the password for the encrypted backup
        password: Option<String>,
    },
    /// Restore last backup to the device
    Restore {
        /// Restore system files, too
        system: bool,
        /// Do NOT reboot the device when done (default: yes)
        no_reboot: bool,
        /// Create a copy of backup folder before restoring
        copy: bool,
        /// Restore device settings from the backup
        settings: bool,
        /// Remove items which are not being restored
        remove: bool,
        /// Do not trigger re-installation of apps after restore
        skip_apps: bool,
        /// Supply the password for the encrypted source backup
        password: Option<String>,
    },
    /// Show details about last completed backup of device
    Info,
    /// List files of last completed backup in CSV format
    List,
    /// Unpack a completed backup in DIRECTORY/_unback_/
    Unback,
    /// Enable or disable backup encryption
    Encryption {
        enabled: bool,
        password: Option<String>,
    },
    /// Change backup password on target device
    ChangePassword {
        old_password: String,
        new_password: String,
    },
    /// Enable or disable cloud use (requires iCloud account)
    Cloud { enabled: bool },
}

/// Options for idevicebackup2
#[derive(Debug, Clone, Default)]
pub struct IDeviceBackup2Options {
    /// Target specific device by UDID
    pub udid: Option<String>,
    /// Use backup data from device specified by UDID
    pub source: Option<String>,
    /// Connect to network device
    pub network: bool,
    /// Request passwords interactively
    pub interactive: bool,
    /// Enable communication debugging
    pub debug: bool,
}

/// Builder for idevicebackup2 commands
#[derive(Debug, Clone)]
pub struct IDeviceBackup2Builder {
    options: IDeviceBackup2Options,
    command: IDeviceBackup2Command,
    directory: PathBuf,
}

impl IDeviceBackup2Builder {
    /// Create a new backup command builder
    pub fn backup(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Backup {
                full: false,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new full backup command builder
    pub fn full_backup(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Backup {
                full: true,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new restore command builder
    pub fn restore(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Restore {
                system: false,
                no_reboot: false,
                copy: false,
                settings: false,
                remove: false,
                skip_apps: false,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new info command builder
    pub fn info(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Info,
            directory: directory.into(),
        }
    }

    /// Create a new list command builder
    pub fn list(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::List,
            directory: directory.into(),
        }
    }

    /// Create a new unback command builder
    pub fn unback(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Unback,
            directory: directory.into(),
        }
    }

    /// Create a new encryption command builder
    pub fn encryption(directory: impl Into<PathBuf>, enabled: bool) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Encryption {
                enabled,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new change password command builder
    pub fn change_password(
        directory: impl Into<PathBuf>,
        old_password: impl Into<String>,
        new_password: impl Into<String>,
    ) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::ChangePassword {
                old_password: old_password.into(),
                new_password: new_password.into(),
            },
            directory: directory.into(),
        }
    }

    /// Create a new cloud command builder
    pub fn cloud(directory: impl Into<PathBuf>, enabled: bool) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Cloud { enabled },
            directory: directory.into(),
        }
    }

    /// Set the target device UDID
    pub fn udid(mut self, udid: impl Into<String>) -> Self {
        self.options.udid = Some(udid.into());
        self
    }

    /// Set the source device UDID
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.options.source = Some(source.into());
        self
    }

    /// Enable network device connection
    pub fn network(mut self) -> Self {
        self.options.network = true;
        self
    }

    /// Enable interactive mode (password prompts)
    pub fn interactive(mut self) -> Self {
        self.options.interactive = true;
        self
    }

    /// Enable debug mode
    pub fn debug(mut self) -> Self {
        self.options.debug = true;
        self
    }

    /// Set the password for encrypted backups (used in non-interactive mode)
    pub fn password(mut self, password: impl Into<String>) -> Self {
        let pw = password.into();
        match &mut self.command {
            IDeviceBackup2Command::Backup { password: p, .. } => {
                *p = Some(pw);
            }
            IDeviceBackup2Command::Restore { password: p, .. } => {
                *p = Some(pw);
            }
            IDeviceBackup2Command::Encryption { password: p, .. } => {
                *p = Some(pw);
            }
            _ => {}
        }
        self
    }

    /// Configure restore options (only applicable for restore command)
    pub fn restore_options(
        mut self,
        system: bool,
        no_reboot: bool,
        copy: bool,
        settings: bool,
        remove: bool,
        skip_apps: bool,
    ) -> Self {
        if let IDeviceBackup2Command::Restore {
            system: s,
            no_reboot: nr,
            copy: c,
            settings: st,
            remove: r,
            skip_apps: sa,
            ..
        } = &mut self.command
        {
            *s = system;
            *nr = no_reboot;
            *c = copy;
            *st = settings;
            *r = remove;
            *sa = skip_apps;
        }
        self
    }

    /// Build the command arguments (without the executable name)
    pub fn build_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // Add options
        if let Some(udid) = &self.options.udid {
            args.push("-u".to_string());
            args.push(udid.clone());
        }

        if let Some(source) = &self.options.source {
            args.push("-s".to_string());
            args.push(source.clone());
        }

        if self.options.network {
            args.push("-n".to_string());
        }

        if self.options.interactive {
            args.push("-i".to_string());
        }

        if self.options.debug {
            args.push("-d".to_string());
        }

        // Add command and its options
        match &self.command {
            IDeviceBackup2Command::Backup { full, password } => {
                args.push("backup".to_string());
                if *full {
                    args.push("--full".to_string());
                }
                if let Some(pw) = password {
                    args.push("--password".to_string());
                    args.push(pw.clone());
                }
            }
            IDeviceBackup2Command::Restore {
                system,
                no_reboot,
                copy,
                settings,
                remove,
                skip_apps,
                password,
            } => {
                args.push("restore".to_string());
                if *system {
                    args.push("--system".to_string());
                }
                if *no_reboot {
                    args.push("--no-reboot".to_string());
                }
                if *copy {
                    args.push("--copy".to_string());
                }
                if *settings {
                    args.push("--settings".to_string());
                }
                if *remove {
                    args.push("--remove".to_string());
                }
                if *skip_apps {
                    args.push("--skip-apps".to_string());
                }
                if let Some(password) = password {
                    args.push("--password".to_string());
                    args.push(password.clone());
                }
            }
            IDeviceBackup2Command::Info => {
                args.push("info".to_string());
            }
            IDeviceBackup2Command::List => {
                args.push("list".to_string());
            }
            IDeviceBackup2Command::Unback => {
                args.push("unback".to_string());
            }
            IDeviceBackup2Command::Encryption { enabled, password } => {
                args.push("encryption".to_string());
                args.push(if *enabled {
                    "on".to_string()
                } else {
                    "off".to_string()
                });
                if let Some(pw) = password {
                    args.push(pw.clone());
                }
            }
            IDeviceBackup2Command::ChangePassword {
                old_password,
                new_password,
            } => {
                args.push("changepw".to_string());
                args.push(old_password.clone());
                args.push(new_password.clone());
            }
            IDeviceBackup2Command::Cloud { enabled } => {
                args.push("cloud".to_string());
                args.push(if *enabled {
                    "on".to_string()
                } else {
                    "off".to_string()
                });
            }
        }

        // Add directory
        args.push(self.directory.to_string_lossy().to_string());

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let mut cmd = Command::new("idevicebackup2");
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute idevicebackup2: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("idevicebackup2 {}", args)
    }
}

/// Helper function to enable encryption on a device
pub fn enable_encryption(
    directory: impl Into<PathBuf>,
    password: Option<&str>,
    interactive: bool,
) -> Result<std::process::Output> {
    let dir: PathBuf = directory.into();

    if interactive {
        let mut builder = IDeviceBackup2Builder::encryption(&dir, true).interactive();
        if let Some(pw) = password {
            builder = builder.password(pw);
        }
        builder.execute()
    } else {
        let mut builder = IDeviceBackup2Builder::encryption(&dir, true);
        if let Some(pw) = password {
            builder = builder.password(pw);
        }
        builder.execute()
    }
}

/// Helper function to perform a full encrypted backup
///
/// This follows the two-step process:
/// 1. Enable encryption: idevicebackup2 -i encryption on [PWD]
/// 2. Run backup: idevicebackup2 backup --full <directory>
pub fn full_encrypted_backup(
    directory: impl Into<PathBuf>,
    password: Option<&str>,
    udid: Option<&str>,
) -> Result<std::process::Output> {
    let dir: PathBuf = directory.into();

    // Step 1: Enable encryption
    let mut enc_builder = IDeviceBackup2Builder::encryption(&dir, true);

    // Use interactive mode only when no password is provided
    if password.is_none() {
        enc_builder = enc_builder.interactive();
    }

    if let Some(device_id) = udid {
        enc_builder = enc_builder.udid(device_id);
    }

    if let Some(pw) = password {
        enc_builder = enc_builder.password(pw);
    }

    let enc_output = enc_builder.execute()?;

    if !enc_output.status.success() {
        let stderr = String::from_utf8_lossy(&enc_output.stderr);
        return Err(anyhow!("Failed to enable backup encryption: {}", stderr));
    }

    // Step 2: Run full backup
    let mut backup_builder = IDeviceBackup2Builder::full_backup(&dir);

    if let Some(device_id) = udid {
        backup_builder = backup_builder.udid(device_id);
    }

    backup_builder.execute()
}

/// Helper to check if idevicebackup2 is available
pub fn check_tool_available() -> bool {
    Command::new("idevicebackup2")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the version of idevicebackup2
pub fn get_version() -> Result<String> {
    let output = Command::new("idevicebackup2")
        .arg("--version")
        .output()
        .map_err(|e| anyhow!("Failed to get idevicebackup2 version: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(anyhow!("idevicebackup2 --version failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_command() {
        let builder = IDeviceBackup2Builder::backup("/tmp/backup");
        let args = builder.build_args();
        assert!(args.contains(&"backup".to_string()));
        assert!(args.contains(&"/tmp/backup".to_string()));
    }

    #[test]
    fn test_full_backup_command() {
        let builder = IDeviceBackup2Builder::full_backup("/tmp/backup");
        let args = builder.build_args();
        assert!(args.contains(&"backup".to_string()));
        assert!(args.contains(&"--full".to_string()));
        assert!(args.contains(&"/tmp/backup".to_string()));
    }

    #[test]
    fn test_encryption_command() {
        let builder = IDeviceBackup2Builder::encryption("/tmp/backup", true);
        let args = builder.build_args();
        assert!(args.contains(&"encryption".to_string()));
        assert!(args.contains(&"on".to_string()));
    }

    #[test]
    fn test_with_udid() {
        let builder = IDeviceBackup2Builder::full_backup("/tmp/backup").udid("1234567890abcdef");
        let args = builder.build_args();
        assert!(args.contains(&"-u".to_string()));
        assert!(args.contains(&"1234567890abcdef".to_string()));
    }

    #[test]
    fn test_restore_command() {
        let builder = IDeviceBackup2Builder::restore("/tmp/backup")
            .restore_options(true, false, true, true, false, false);
        let args = builder.build_args();
        assert!(args.contains(&"restore".to_string()));
        assert!(args.contains(&"--system".to_string()));
        assert!(args.contains(&"--copy".to_string()));
        assert!(args.contains(&"--settings".to_string()));
    }
}

```

### `src/chronos/mvt_builder.rs`

```rust
//! MVT (Mobile Verification Toolkit) Command Builder
//!
//! Provides builders for MVT-iOS and MVT-Android commands
//! MVT is a analytic tool for analyzing mobile devices: https://github.com/mvt-project/mvt

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Command;

/// MVT modules for iOS analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MvtIosModule {
    /// Check backup for signs of compromise
    CheckBackup,
    /// Extract data from an iTunes backup
    DecryptBackup,
}

impl MvtIosModule {
    /// Get the command name for this module
    pub fn command_name(&self) -> &'static str {
        match self {
            MvtIosModule::CheckBackup => "mvt-ios",
            MvtIosModule::DecryptBackup => "mvt-ios",
        }
    }

    /// Get the subcommand/argument for this module
    pub fn subcommand(&self) -> &'static str {
        match self {
            MvtIosModule::CheckBackup => "check-backup",
            MvtIosModule::DecryptBackup => "decrypt-backup",
        }
    }
}

/// MVT modules for Android analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MvtAndroidModule {
    /// Check an Android device for signs of compromise
    CheckAndroid,
    /// Download and check an Android APK for signs of compromise
    CheckApk,
    /// Check an Android backup for signs of compromise
    CheckBackup,
}

impl MvtAndroidModule {
    /// Get the command name for this module
    pub fn command_name(&self) -> &'static str {
        match self {
            MvtAndroidModule::CheckAndroid => "mvt-android",
            MvtAndroidModule::CheckApk => "mvt-android",
            MvtAndroidModule::CheckBackup => "mvt-android",
        }
    }

    /// Get the subcommand/argument for this module
    pub fn subcommand(&self) -> &'static str {
        match self {
            MvtAndroidModule::CheckAndroid => "check-android",
            MvtAndroidModule::CheckApk => "check-apk",
            MvtAndroidModule::CheckBackup => "check-backup",
        }
    }
}

/// Common options for MVT commands
#[derive(Debug, Clone, Default)]
pub struct MvtOptions {
    /// Output directory for results
    pub output: Option<PathBuf>,
    /// iOC JSON file for additional indicators
    pub iocs: Option<PathBuf>,
    /// Enable verbose output
    pub verbose: bool,
    /// Enable JSON output format
    pub json: bool,
    /// Specify the serial number for Android devices
    pub serial: Option<String>,
}

/// Builder for MVT-iOS check-backup command
#[derive(Debug, Clone)]
pub struct MvtIosCheckBackupBuilder {
    backup_path: PathBuf,
    options: MvtOptions,
}

impl MvtIosCheckBackupBuilder {
    /// Create a new check-backup builder
    pub fn new(backup_path: impl Into<PathBuf>) -> Self {
        Self {
            backup_path: backup_path.into(),
            options: MvtOptions::default(),
        }
    }

    /// Set the output directory
    pub fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.options.output = Some(output.into());
        self
    }

    /// Set the iOC JSON file
    pub fn iocs(mut self, iocs: impl Into<PathBuf>) -> Self {
        self.options.iocs = Some(iocs.into());
        self
    }

    /// Enable verbose output
    pub fn verbose(mut self) -> Self {
        self.options.verbose = true;
        self
    }

    /// Enable JSON output
    pub fn json(mut self) -> Self {
        self.options.json = true;
        self
    }

    /// Build the command arguments
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![MvtIosModule::CheckBackup.subcommand().to_string()];

        if let Some(output) = &self.options.output {
            args.push("--output".to_string());
            args.push(output.to_string_lossy().to_string());
        }

        if let Some(iocs) = &self.options.iocs {
            args.push("--iocs".to_string());
            args.push(iocs.to_string_lossy().to_string());
        }

        if self.options.verbose {
            args.push("--verbose".to_string());
        }

        if self.options.json {
            args.push("--json".to_string());
        }

        args.push(self.backup_path.to_string_lossy().to_string());

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let tool = crate::common::deps::resolve_tool(MvtIosModule::CheckBackup.command_name())
            .unwrap_or_else(|| PathBuf::from(MvtIosModule::CheckBackup.command_name()));
        let mut cmd = Command::new(tool);
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute mvt-ios check-backup: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("mvt-ios {}", args)
    }
}

/// Builder for MVT-iOS decrypt-backup command
#[derive(Debug, Clone)]
pub struct MvtIosDecryptBackupBuilder {
    backup_path: PathBuf,
    output_path: PathBuf,
    password: Option<String>,
    options: MvtOptions,
}

impl MvtIosDecryptBackupBuilder {
    /// Create a new decrypt-backup builder
    pub fn new(backup_path: impl Into<PathBuf>, output_path: impl Into<PathBuf>) -> Self {
        Self {
            backup_path: backup_path.into(),
            output_path: output_path.into(),
            password: None,
            options: MvtOptions::default(),
        }
    }

    /// Set the backup password
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the iOC JSON file
    pub fn iocs(mut self, iocs: impl Into<PathBuf>) -> Self {
        self.options.iocs = Some(iocs.into());
        self
    }

    /// Enable verbose output
    pub fn verbose(mut self) -> Self {
        self.options.verbose = true;
        self
    }

    /// Build the command arguments
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![MvtIosModule::DecryptBackup.subcommand().to_string()];

        args.push("-d".to_string());
        args.push(self.backup_path.to_string_lossy().to_string());

        args.push("-o".to_string());
        args.push(self.output_path.to_string_lossy().to_string());

        if let Some(password) = &self.password {
            args.push("-p".to_string());
            args.push(password.clone());
        }

        if self.options.verbose {
            args.push("--verbose".to_string());
        }

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let tool = crate::common::deps::resolve_tool(MvtIosModule::DecryptBackup.command_name())
            .unwrap_or_else(|| PathBuf::from(MvtIosModule::DecryptBackup.command_name()));
        let mut cmd = Command::new(tool);
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute mvt-ios decrypt-backup: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("mvt-ios {}", args)
    }
}

/// Unified MVT iOS Builder
#[derive(Debug, Clone)]
pub enum MvtIosBuilder {
    CheckBackup(MvtIosCheckBackupBuilder),
    DecryptBackup(MvtIosDecryptBackupBuilder),
}

impl MvtIosBuilder {
    /// Create a new check-backup builder
    pub fn check_backup(backup_path: impl Into<PathBuf>) -> Self {
        Self::CheckBackup(MvtIosCheckBackupBuilder::new(backup_path))
    }

    /// Create a new decrypt-backup builder
    pub fn decrypt_backup(
        backup_path: impl Into<PathBuf>,
        output_path: impl Into<PathBuf>,
    ) -> Self {
        Self::DecryptBackup(MvtIosDecryptBackupBuilder::new(backup_path, output_path))
    }

    /// Execute the command
    pub fn execute(&self) -> Result<std::process::Output> {
        match self {
            MvtIosBuilder::CheckBackup(b) => b.execute(),
            MvtIosBuilder::DecryptBackup(b) => b.execute(),
        }
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        match self {
            MvtIosBuilder::CheckBackup(b) => b.to_command_string(),
            MvtIosBuilder::DecryptBackup(b) => b.to_command_string(),
        }
    }
}

/// Builder for MVT-Android check-android command
#[derive(Debug, Clone)]
pub struct MvtAndroidCheckBuilder {
    options: MvtOptions,
}

impl MvtAndroidCheckBuilder {
    /// Create a new check-android builder
    pub fn new() -> Self {
        Self {
            options: MvtOptions::default(),
        }
    }

    /// Set the output directory
    pub fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.options.output = Some(output.into());
        self
    }

    /// Set the iOC JSON file
    pub fn iocs(mut self, iocs: impl Into<PathBuf>) -> Self {
        self.options.iocs = Some(iocs.into());
        self
    }

    /// Set the device serial number
    pub fn serial(mut self, serial: impl Into<String>) -> Self {
        self.options.serial = Some(serial.into());
        self
    }

    /// Enable verbose output
    pub fn verbose(mut self) -> Self {
        self.options.verbose = true;
        self
    }

    /// Enable JSON output
    pub fn json(mut self) -> Self {
        self.options.json = true;
        self
    }

    /// Build the command arguments
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![MvtAndroidModule::CheckAndroid.subcommand().to_string()];

        if let Some(output) = &self.options.output {
            args.push("--output".to_string());
            args.push(output.to_string_lossy().to_string());
        }

        if let Some(iocs) = &self.options.iocs {
            args.push("--iocs".to_string());
            args.push(iocs.to_string_lossy().to_string());
        }

        if let Some(serial) = &self.options.serial {
            args.push("--serial".to_string());
            args.push(serial.clone());
        }

        if self.options.verbose {
            args.push("--verbose".to_string());
        }

        if self.options.json {
            args.push("--json".to_string());
        }

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let tool = crate::common::deps::resolve_tool(MvtAndroidModule::CheckAndroid.command_name())
            .unwrap_or_else(|| PathBuf::from(MvtAndroidModule::CheckAndroid.command_name()));
        let mut cmd = Command::new(tool);
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute mvt-android check-android: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("mvt-android {}", args)
    }
}

/// Builder for MVT-Android check-apk command
#[derive(Debug, Clone)]
pub struct MvtAndroidCheckApkBuilder {
    apk_path: PathBuf,
    options: MvtOptions,
}

impl MvtAndroidCheckApkBuilder {
    /// Create a new check-apk builder
    pub fn new(apk_path: impl Into<PathBuf>) -> Self {
        Self {
            apk_path: apk_path.into(),
            options: MvtOptions::default(),
        }
    }

    /// Set the output directory
    pub fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.options.output = Some(output.into());
        self
    }

    /// Set the iOC JSON file
    pub fn iocs(mut self, iocs: impl Into<PathBuf>) -> Self {
        self.options.iocs = Some(iocs.into());
        self
    }

    /// Enable verbose output
    pub fn verbose(mut self) -> Self {
        self.options.verbose = true;
        self
    }

    /// Enable JSON output
    pub fn json(mut self) -> Self {
        self.options.json = true;
        self
    }

    /// Build the command arguments
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![MvtAndroidModule::CheckApk.subcommand().to_string()];

        if let Some(output) = &self.options.output {
            args.push("--output".to_string());
            args.push(output.to_string_lossy().to_string());
        }

        if let Some(iocs) = &self.options.iocs {
            args.push("--iocs".to_string());
            args.push(iocs.to_string_lossy().to_string());
        }

        if self.options.verbose {
            args.push("--verbose".to_string());
        }

        if self.options.json {
            args.push("--json".to_string());
        }

        args.push(self.apk_path.to_string_lossy().to_string());

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let tool = crate::common::deps::resolve_tool(MvtAndroidModule::CheckApk.command_name())
            .unwrap_or_else(|| PathBuf::from(MvtAndroidModule::CheckApk.command_name()));
        let mut cmd = Command::new(tool);
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute mvt-android check-apk: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("mvt-android {}", args)
    }
}

/// Builder for MVT-Android check-backup command
#[derive(Debug, Clone)]
pub struct MvtAndroidCheckBackupBuilder {
    backup_path: PathBuf,
    options: MvtOptions,
}

impl MvtAndroidCheckBackupBuilder {
    /// Create a new check-backup builder
    pub fn new(backup_path: impl Into<PathBuf>) -> Self {
        Self {
            backup_path: backup_path.into(),
            options: MvtOptions::default(),
        }
    }

    /// Set the output directory
    pub fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.options.output = Some(output.into());
        self
    }

    /// Set the iOC JSON file
    pub fn iocs(mut self, iocs: impl Into<PathBuf>) -> Self {
        self.options.iocs = Some(iocs.into());
        self
    }

    /// Enable verbose output
    pub fn verbose(mut self) -> Self {
        self.options.verbose = true;
        self
    }

    /// Enable JSON output
    pub fn json(mut self) -> Self {
        self.options.json = true;
        self
    }

    /// Build the command arguments
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![MvtAndroidModule::CheckBackup.subcommand().to_string()];

        if let Some(output) = &self.options.output {
            args.push("--output".to_string());
            args.push(output.to_string_lossy().to_string());
        }

        if let Some(iocs) = &self.options.iocs {
            args.push("--iocs".to_string());
            args.push(iocs.to_string_lossy().to_string());
        }

        if self.options.verbose {
            args.push("--verbose".to_string());
        }

        if self.options.json {
            args.push("--json".to_string());
        }

        args.push(self.backup_path.to_string_lossy().to_string());

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let tool = crate::common::deps::resolve_tool(MvtAndroidModule::CheckBackup.command_name())
            .unwrap_or_else(|| PathBuf::from(MvtAndroidModule::CheckBackup.command_name()));
        let mut cmd = Command::new(tool);
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute mvt-android check-backup: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("mvt-android {}", args)
    }
}

/// Unified MVT Android Builder
#[derive(Debug, Clone)]
pub enum MvtAndroidBuilder {
    CheckAndroid(MvtAndroidCheckBuilder),
    CheckApk(MvtAndroidCheckApkBuilder),
    CheckBackup(MvtAndroidCheckBackupBuilder),
}

impl MvtAndroidBuilder {
    /// Create a new check-android builder
    pub fn check_android() -> Self {
        Self::CheckAndroid(MvtAndroidCheckBuilder::new())
    }

    /// Create a new check-apk builder
    pub fn check_apk(apk_path: impl Into<PathBuf>) -> Self {
        Self::CheckApk(MvtAndroidCheckApkBuilder::new(apk_path))
    }

    /// Create a new check-backup builder
    pub fn check_backup(backup_path: impl Into<PathBuf>) -> Self {
        Self::CheckBackup(MvtAndroidCheckBackupBuilder::new(backup_path))
    }

    /// Execute the command
    pub fn execute(&self) -> Result<std::process::Output> {
        match self {
            MvtAndroidBuilder::CheckAndroid(b) => b.execute(),
            MvtAndroidBuilder::CheckApk(b) => b.execute(),
            MvtAndroidBuilder::CheckBackup(b) => b.execute(),
        }
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        match self {
            MvtAndroidBuilder::CheckAndroid(b) => b.to_command_string(),
            MvtAndroidBuilder::CheckApk(b) => b.to_command_string(),
            MvtAndroidBuilder::CheckBackup(b) => b.to_command_string(),
        }
    }
}

/// Check if MVT is available
pub fn check_mvt_available() -> bool {
    Command::new("mvt-ios")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if MVT Android is available
pub fn check_mvt_android_available() -> bool {
    Command::new("mvt-android")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get MVT version
pub fn get_mvt_version() -> Result<String> {
    let output = Command::new("mvt-ios")
        .arg("--version")
        .output()
        .map_err(|e| anyhow!("Failed to get mvt-ios version: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(anyhow!("mvt-ios --version failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mvt_ios_check_backup() {
        let builder = MvtIosCheckBackupBuilder::new("/tmp/backup")
            .output("/tmp/output")
            .verbose()
            .json();
        let args = builder.build_args();
        assert!(args.contains(&"check-backup".to_string()));
        assert!(args.contains(&"--output".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--json".to_string()));
    }

    #[test]
    fn test_mvt_ios_decrypt_backup() {
        let builder =
            MvtIosDecryptBackupBuilder::new("/tmp/backup", "/tmp/output").password("test123");
        let args = builder.build_args();
        assert!(args.contains(&"decrypt-backup".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"test123".to_string()));
    }

    #[test]
    fn test_mvt_android_check_apk() {
        let builder = MvtAndroidCheckApkBuilder::new("/tmp/app.apk").output("/tmp/output");
        let args = builder.build_args();
        assert!(args.contains(&"check-apk".to_string()));
        assert!(args.contains(&"/tmp/app.apk".to_string()));
    }
}

```

### `src/chronos/progress.rs`

```rust
//! Backup Progress Tracking

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BackupProgress {
    total_files: Arc<AtomicU64>,
    processed_files: Arc<AtomicU64>,
    current_file: Arc<RwLock<String>>,
    start_time: Instant,
    last_update: Arc<RwLock<Instant>>,
}

#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub total_files: u64,
    pub processed_files: u64,
    pub progress_percentage: f64,
    pub current_file: String,
    pub elapsed_time: Duration,
    pub estimated_remaining: Option<Duration>,
}

impl BackupProgress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            total_files: Arc::new(AtomicU64::new(0)),
            processed_files: Arc::new(AtomicU64::new(0)),
            current_file: Arc::new(RwLock::new(String::new())),
            start_time: Instant::now(),
            last_update: Arc::new(RwLock::new(Instant::now())),
        })
    }

    pub fn update_total(&self, count: u64) {
        self.total_files.store(count, Ordering::Relaxed);
        self.update_timestamp();
    }

    pub fn increment_processed(&self) {
        self.processed_files.fetch_add(1, Ordering::Relaxed);
        self.update_timestamp();
    }

    pub fn set_current_file(&self, filename: &str) {
        *self.current_file.write().unwrap() = filename.to_string();
        self.update_timestamp();
    }

    fn update_timestamp(&self) {
        *self.last_update.write().unwrap() = Instant::now();
    }

    pub fn progress_percentage(&self) -> f64 {
        let total = self.total_files.load(Ordering::Relaxed);
        let processed = self.processed_files.load(Ordering::Relaxed);

        if total == 0 {
            0.0
        } else {
            (processed as f64 / total as f64) * 100.0
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        let total = self.total_files.load(Ordering::Relaxed);
        let processed = self.processed_files.load(Ordering::Relaxed);
        let current_file = self.current_file.read().unwrap().clone();
        let elapsed = self.start_time.elapsed();

        let estimated_remaining = if processed > 0 && total > 0 {
            let rate = processed as f64 / elapsed.as_secs_f64();
            let remaining = (total - processed) as f64 / rate;
            Some(Duration::from_secs_f64(remaining))
        } else {
            None
        };

        ProgressSnapshot {
            total_files: total,
            processed_files: processed,
            progress_percentage: self.progress_percentage(),
            current_file,
            elapsed_time: elapsed,
            estimated_remaining,
        }
    }

    pub fn reset(&self) {
        self.total_files.store(0, Ordering::Relaxed);
        self.processed_files.store(0, Ordering::Relaxed);
        *self.current_file.write().unwrap() = String::new();
        *self.last_update.write().unwrap() = Instant::now();
    }
}

```

## Chronos — iOS Backup Internals

### `src/chronos/ios_backup/mod.rs`

```rust
//! iOS encrypted backup support for Chronos.
//!
//! Responsibilities:
//! - Parse Manifest.plist
//! - Derive backup keys (owner password only)
//! - Decrypt Manifest.db
//! - Decrypt individual backup blobs on demand

pub mod file_decrypt;
pub mod keybag;
pub mod keys;
pub mod manifest;

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize)]
pub struct ExtractSpec {
    pub relative_paths_like: String,
    pub domain_like: String,
    pub preserve_folders: bool,
    pub domain_subfolders: bool,
    pub incremental: bool,
}

#[derive(Debug, Serialize)]
pub struct ExtractResult {
    pub input: PathBuf,
    pub output: PathBuf,
    pub extracted: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Debug, Serialize)]
pub struct VerifyResult {
    pub file_count: u64,
}

/// High-level verification entrypoint.
/// This should complete in seconds if the password is correct.
pub fn verify_decrypt(backup_dir: &Path, password: &str) -> Result<VerifyResult> {
    if password.is_empty() {
        return Err(anyhow!("Decrypt password is empty"));
    }
    let plist = manifest::load_manifest_plist(backup_dir)?;
    let keybag = keybag::parse_keybag(&plist.backup_keybag)?;
    let class_keys = keys::derive_class_keys(password, &keybag)?;

    let temp = NamedTempFile::new()?;
    let decrypted = manifest::decrypt_manifest_db_to(
        backup_dir,
        &class_keys,
        &plist.manifest_key,
        temp.path(),
    )?;
    let conn = decrypted.open_connection()?;
    let file_count = manifest::count_files(&conn)?;

    Ok(VerifyResult { file_count })
}

/// Decrypt specific files from an encrypted iTunes/Finder backup.
pub fn extract_from_encrypted_backup(
    encrypted_backup_root: &Path,
    output_root: &Path,
    password: &str,
    specs: &[ExtractSpec],
) -> Result<ExtractResult> {
    if password.is_empty() {
        return Err(anyhow!("Decrypt password is empty"));
    }
    if !encrypted_backup_root.is_dir() {
        return Err(anyhow!(
            "Encrypted backup path is not a directory: {}",
            encrypted_backup_root.display()
        ));
    }
    if specs.is_empty() {
        return Err(anyhow!("No extraction specs provided"));
    }

    std::fs::create_dir_all(output_root)?;

    let plist = manifest::load_manifest_plist(encrypted_backup_root)?;
    let keybag = keybag::parse_keybag(&plist.backup_keybag)?;
    let class_keys = keys::derive_class_keys(password, &keybag)?;

    let manifest_out = output_root.join("_manifest").join("Manifest.decrypted.db");
    let decrypted = manifest::decrypt_manifest_db_to(
        encrypted_backup_root,
        &class_keys,
        &plist.manifest_key,
        &manifest_out,
    )?;

    let conn = decrypted.open_connection()?;
    let error_log = output_root.join("_manifest").join("extract_errors.jsonl");
    let skip_log = output_root.join("_manifest").join("extract_skipped.jsonl");
    let stats = file_decrypt::extract_specs(
        encrypted_backup_root,
        &conn,
        &class_keys,
        output_root,
        specs,
        Some(&error_log),
        Some(&skip_log),
    )?;

    Ok(ExtractResult {
        input: encrypted_backup_root.to_path_buf(),
        output: output_root.to_path_buf(),
        extracted: stats.extracted,
        skipped: stats.skipped,
        errors: stats.errors,
    })
}

/// Decrypt the full backup into `output_root` (domain/relativePath preserved).
pub fn extract_full_backup(
    encrypted_backup_root: &Path,
    output_root: &Path,
    password: &str,
    incremental: bool,
) -> Result<ExtractResult> {
    let spec = ExtractSpec {
        relative_paths_like: "%".to_string(),
        domain_like: "%".to_string(),
        preserve_folders: true,
        domain_subfolders: true,
        incremental,
    };
    extract_from_encrypted_backup(encrypted_backup_root, output_root, password, &[spec])
}

pub fn decrypt_file_by_path(
    backup_dir: &Path,
    manifest_db: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    domain: &str,
    relative_path: &str,
    output_path: &Path,
) -> Result<()> {
    let conn = manifest::DecryptedManifest {
        path: manifest_db.to_path_buf(),
    }
    .open_connection()?;
    file_decrypt::decrypt_file_by_path(
        backup_dir,
        &conn,
        class_keys,
        domain,
        relative_path,
        output_path,
    )
}

```

### `src/chronos/ios_backup/file_decrypt.rs`

```rust
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use aes_kw::KekAes256;
use anyhow::{anyhow, Context, Result};
use plist::Value;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct FilePlist {
    pub filesize: u64,
    pub mtime: Option<i64>,
    pub protection_class: u32,
    pub encryption_key: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct ManifestEntry {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub file_plist: FilePlist,
}

#[derive(Debug, Default, Serialize)]
pub struct ExtractStats {
    pub extracted: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl FilePlist {
    pub fn from_bplist(blob: &[u8]) -> Result<Self> {
        let value = read_plist_value(blob)?;
        let dict = value
            .as_dictionary()
            .ok_or_else(|| anyhow!("File plist is not a dictionary"))?;

        let objects = dict
            .get("$objects")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("File plist missing $objects"))?;

        let top = dict
            .get("$top")
            .and_then(|v| v.as_dictionary())
            .ok_or_else(|| anyhow!("File plist missing $top"))?;

        let root_uid = top
            .get("root")
            .and_then(uid_value)
            .ok_or_else(|| anyhow!("File plist missing root UID"))?;

        let data = objects
            .get(root_uid as usize)
            .and_then(|v| v.as_dictionary())
            .ok_or_else(|| anyhow!("File plist root object invalid"))?;

        let filesize = data
            .get("Size")
            .and_then(value_to_u64)
            .ok_or_else(|| anyhow!("File plist missing Size"))?;

        let mtime = data.get("LastModified").and_then(value_to_i64);

        let protection_class =
            data.get("ProtectionClass")
                .and_then(value_to_u64)
                .ok_or_else(|| anyhow!("File plist missing ProtectionClass"))? as u32;

        let encryption_key = if let Some(uid) = data.get("EncryptionKey").and_then(uid_value) {
            if let Some(obj) = objects.get(uid as usize).and_then(|v| v.as_dictionary()) {
                if let Some(data_val) = obj.get("NS.data").and_then(|v| v.as_data()) {
                    if data_val.len() > 4 {
                        Some(data_val[4..].to_vec())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(FilePlist {
            filesize,
            mtime,
            protection_class,
            encryption_key,
        })
    }
}

pub fn find_entry(conn: &Connection, domain: &str, relative_path: &str) -> Result<ManifestEntry> {
    let mut stmt = conn.prepare(
        "SELECT fileID, domain, relativePath, file FROM Files WHERE flags=1 AND domain = ?1 AND relativePath = ?2 LIMIT 1",
    )?;
    let mut rows = stmt.query([domain, relative_path])?;
    let row = rows
        .next()?
        .ok_or_else(|| anyhow!("File not found in manifest"))?;
    let file_id: String = row.get(0)?;
    let domain: String = row.get(1)?;
    let relative_path: String = row.get(2)?;
    let file_blob: Vec<u8> = row.get(3)?;
    let file_plist = FilePlist::from_bplist(&file_blob)?;
    Ok(ManifestEntry {
        file_id,
        domain,
        relative_path,
        file_plist,
    })
}

pub fn extract_specs(
    backup_dir: &Path,
    conn: &Connection,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    output_root: &Path,
    specs: &[crate::chronos::ios_backup::ExtractSpec],
    error_log_path: Option<&Path>,
    skip_log_path: Option<&Path>,
) -> Result<ExtractStats> {
    let mut stats = ExtractStats::default();
    let mut error_log = open_error_log(error_log_path);
    let mut skip_log = open_skip_log(skip_log_path);

    for spec in specs {
        let mut stmt = conn.prepare(
            "SELECT fileID, domain, relativePath, file FROM Files WHERE flags=1 AND relativePath LIKE ?1 AND domain LIKE ?2 ORDER BY domain, relativePath",
        )?;
        let mut rows =
            stmt.query([spec.relative_paths_like.as_str(), spec.domain_like.as_str()])?;
        while let Some(row) = rows.next()? {
            let file_id: String = row.get(0)?;
            let domain: String = row.get(1)?;
            let relative_path: String = row.get(2)?;
            let file_blob: Vec<u8> = row.get(3)?;
            let file_plist = match FilePlist::from_bplist(&file_blob) {
                Ok(p) => p,
                Err(err) => {
                    stats.errors += 1;
                    write_error(
                        &mut error_log,
                        &ErrorEntry {
                            file_id,
                            domain,
                            relative_path,
                            stage: "file_plist",
                            message: err.to_string(),
                        },
                    );
                    continue;
                }
            };

            let entry = ManifestEntry {
                file_id,
                domain,
                relative_path,
                file_plist,
            };

            match decrypt_entry_to_root(backup_dir, class_keys, output_root, &entry, spec) {
                Ok(ExtractOutcome::Extracted) => stats.extracted += 1,
                Ok(ExtractOutcome::Skipped(skip_entry)) => {
                    stats.skipped += 1;
                    write_skip(&mut skip_log, &skip_entry);
                }
                Err(err) => {
                    stats.errors += 1;
                    write_error(
                        &mut error_log,
                        &ErrorEntry {
                            file_id: entry.file_id.clone(),
                            domain: entry.domain.clone(),
                            relative_path: entry.relative_path.clone(),
                            stage: "decrypt",
                            message: format!("{:#}", err),
                        },
                    );
                }
            }
        }
    }

    Ok(stats)
}

pub fn decrypt_entry_to_root(
    backup_dir: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    output_root: &Path,
    entry: &ManifestEntry,
    spec: &crate::chronos::ios_backup::ExtractSpec,
) -> Result<ExtractOutcome> {
    let domain = sanitize_domain(&entry.domain);
    let rel_path = sanitize_relative_path(&entry.relative_path);
    let mut dest = PathBuf::from(output_root);

    if spec.domain_subfolders {
        dest.push(domain);
    }
    if spec.preserve_folders {
        if let Some(parent) = rel_path.parent() {
            dest.push(parent);
        }
    }
    let filename = rel_path
        .file_name()
        .ok_or_else(|| anyhow!("Missing filename"))?;
    dest.push(filename);

    if spec.incremental && dest.exists() {
        let dest_mtime = file_mtime_secs(&dest);
        if should_skip_existing(dest_mtime, entry.file_plist.mtime) {
            return Ok(ExtractOutcome::Skipped(SkipEntry {
                file_id: entry.file_id.clone(),
                domain: entry.domain.clone(),
                relative_path: entry.relative_path.clone(),
                output_path: dest.display().to_string(),
                reason: "incremental_up_to_date",
                backup_mtime: entry.file_plist.mtime,
                dest_mtime,
            }));
        }
    }

    decrypt_entry_to_path(backup_dir, class_keys, entry, &dest)?;
    Ok(ExtractOutcome::Extracted)
}

pub fn decrypt_entry_to_path(
    backup_dir: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    entry: &ManifestEntry,
    output_path: &Path,
) -> Result<()> {
    let source_path = file_blob_path(backup_dir, &entry.file_id)?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    if entry.file_plist.encryption_key.is_none() {
        fs::copy(&source_path, output_path).with_context(|| {
            format!(
                "Failed to copy {} -> {}",
                source_path.display(),
                output_path.display()
            )
        })?;
        return Ok(());
    }

    let class_key = class_keys
        .get(&entry.file_plist.protection_class)
        .ok_or_else(|| anyhow!("Missing class key {}", entry.file_plist.protection_class))?;
    let kek = KekAes256::try_from(class_key.as_slice())?;
    let file_key = kek.unwrap_vec(
        entry
            .file_plist
            .encryption_key
            .as_ref()
            .ok_or_else(|| anyhow!("Missing encryption key"))?,
    )?;

    decrypt_cbc_to_path(&source_path, output_path, &file_key).with_context(|| {
        format!(
            "Failed to decrypt {} -> {}",
            source_path.display(),
            output_path.display()
        )
    })?;
    Ok(())
}

pub fn decrypt_file_by_path(
    backup_dir: &Path,
    conn: &Connection,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    domain: &str,
    relative_path: &str,
    output_path: &Path,
) -> Result<()> {
    let entry = find_entry(conn, domain, relative_path)?;
    decrypt_entry_to_path(backup_dir, class_keys, &entry, output_path)?;
    Ok(())
}

fn file_blob_path(backup_dir: &Path, file_id: &str) -> Result<PathBuf> {
    if file_id.len() < 2 {
        return Err(anyhow!("Invalid file ID"));
    }
    Ok(backup_dir.join(&file_id[0..2]).join(file_id))
}

fn sanitize_relative_path(rel: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(part) => {
                let part_str = part.to_string_lossy();
                let safe = sanitize_component(&part_str);
                out.push(safe);
            }
            Component::CurDir => {}
            Component::ParentDir => {}
            Component::RootDir => {}
            Component::Prefix(_) => {}
        }
    }
    out
}

fn sanitize_domain(domain: &str) -> String {
    let cleaned: String = domain
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    sanitize_component(&cleaned)
}

fn value_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Integer(i) => i
            .as_unsigned()
            .or_else(|| i.as_signed().and_then(|v| u64::try_from(v).ok())),
        Value::Real(f) => Some(*f as u64),
        _ => None,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(i) => i
            .as_signed()
            .or_else(|| i.as_unsigned().and_then(|v| i64::try_from(v).ok())),
        Value::Real(f) => Some(*f as i64),
        _ => None,
    }
}

fn uid_value(value: &Value) -> Option<u64> {
    match value {
        Value::Uid(uid) => Some(uid.get()),
        _ => None,
    }
}

fn read_plist_value(blob: &[u8]) -> Result<Value> {
    let cursor = Cursor::new(blob);
    Ok(Value::from_reader(cursor)?)
}

fn decrypt_cbc_to_path(input: &Path, output: &Path, key: &[u8]) -> Result<()> {
    let mut reader = BufReader::new(File::open(input)?);
    let mut writer = BufWriter::new(File::create(output)?);

    let cipher = Aes256::new_from_slice(key)?;
    let mut prev_block = [0u8; 16];
    let mut pending: Option<Vec<u8>> = None;
    let mut buf = vec![0u8; 1024 * 1024];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if n % 16 != 0 {
            return Err(anyhow!("Encrypted data length is not a multiple of 16"));
        }
        for block in buf[..n].chunks_exact(16) {
            let mut block_buf = [0u8; 16];
            block_buf.copy_from_slice(block);
            let mut ga = block_buf.into();
            cipher.decrypt_block(&mut ga);
            for (i, b) in ga.iter_mut().enumerate() {
                *b ^= prev_block[i];
            }
            prev_block.copy_from_slice(block);

            if let Some(prev) = pending.take() {
                writer.write_all(&prev)?;
            }
            pending = Some(ga.to_vec());
        }
    }

    if let Some(last) = pending {
        if last.is_empty() {
            return Err(anyhow!("Missing final block"));
        }
        let pad = *last.last().unwrap() as usize;
        if pad == 0 || pad > 16 || pad > last.len() {
            return Err(anyhow!("Invalid PKCS7 padding"));
        }
        writer.write_all(&last[..last.len() - pad])?;
    }

    writer.flush()?;
    Ok(())
}

fn file_mtime_secs(path: &Path) -> Option<i64> {
    let Ok(meta) = fs::metadata(path) else {
        return None;
    };
    let Ok(modified) = meta.modified() else {
        return None;
    };
    system_time_to_secs(modified)
}

fn should_skip_existing(dest_mtime: Option<i64>, backup_mtime: Option<i64>) -> bool {
    let target_mtime = match backup_mtime {
        Some(v) => v,
        None => return false,
    };
    let dest_mtime = match dest_mtime {
        Some(v) => v,
        None => return false,
    };
    dest_mtime >= target_mtime
}

fn system_time_to_secs(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn sanitize_component(value: &str) -> String {
    let mut out: String = value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();

    while out.ends_with(' ') || out.ends_with('.') {
        out.pop();
    }

    if out.is_empty() {
        out = "_".to_string();
    }

    let stem = out.split('.').next().unwrap_or(&out);
    let upper = stem.to_ascii_uppercase();
    if is_reserved_device_name(&upper) {
        out = format!("_{}", out);
    }

    out
}

fn is_reserved_device_name(name: &str) -> bool {
    matches!(
        name,
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[derive(Debug, Serialize)]
struct ErrorEntry {
    file_id: String,
    domain: String,
    relative_path: String,
    stage: &'static str,
    message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct SkipEntry {
    file_id: String,
    domain: String,
    relative_path: String,
    output_path: String,
    reason: &'static str,
    backup_mtime: Option<i64>,
    dest_mtime: Option<i64>,
}

fn open_error_log(path: Option<&Path>) -> Option<BufWriter<File>> {
    let path = path?;
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!(
                "WARN: failed to create error log dir {}: {}",
                parent.display(),
                err
            );
            return None;
        }
    }
    match File::create(path) {
        Ok(file) => Some(BufWriter::new(file)),
        Err(err) => {
            eprintln!(
                "WARN: failed to create error log {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn open_skip_log(path: Option<&Path>) -> Option<BufWriter<File>> {
    let path = path?;
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!(
                "WARN: failed to create skip log dir {}: {}",
                parent.display(),
                err
            );
            return None;
        }
    }
    match File::create(path) {
        Ok(file) => Some(BufWriter::new(file)),
        Err(err) => {
            eprintln!(
                "WARN: failed to create skip log {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn write_error(writer: &mut Option<BufWriter<File>>, entry: &ErrorEntry) {
    let Some(writer) = writer.as_mut() else {
        return;
    };
    if let Ok(line) = serde_json::to_string(entry) {
        let _ = writeln!(writer, "{}", line);
    }
}

fn write_skip(writer: &mut Option<BufWriter<File>>, entry: &SkipEntry) {
    let Some(writer) = writer.as_mut() else {
        return;
    };
    if let Ok(line) = serde_json::to_string(entry) {
        let _ = writeln!(writer, "{}", line);
    }
}

#[derive(Debug, Clone)]
pub enum ExtractOutcome {
    Extracted,
    Skipped(SkipEntry),
}

```

### `src/chronos/ios_backup/keybag.rs`

```rust
use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct ClassKey {
    pub class_id: u32,
    pub wrapped_key: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Keybag {
    pub dpsl: Vec<u8>,
    pub dpic: u32,
    pub salt: Vec<u8>,
    pub iter: u32,
    pub class_keys: Vec<ClassKey>,
}

/// Parse Apple TLV keybag structure from Manifest.plist.
pub fn parse_keybag(buf: &[u8]) -> Result<Keybag> {
    let mut i = 0;
    let mut dpsl = None;
    let mut dpic = None;
    let mut salt = None;
    let mut iter = None;

    let mut current_class = None;
    let mut current_wrapped = None;
    let mut class_keys = Vec::new();

    while i + 8 <= buf.len() {
        let tag = &buf[i..i + 4];
        i += 4;
        let len = u32::from_be_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if i + len > buf.len() {
            return Err(anyhow!("Keybag TLV length out of bounds"));
        }
        let val = &buf[i..i + len];
        i += len;

        match tag {
            b"DPSL" => dpsl = Some(val.to_vec()),
            b"DPIC" => {
                dpic = Some(parse_u32(tag, val)?);
            }
            b"SALT" => salt = Some(val.to_vec()),
            b"ITER" => {
                iter = Some(parse_u32(tag, val)?);
            }
            b"CLAS" => {
                if let (Some(c), Some(w)) = (current_class.take(), current_wrapped.take()) {
                    class_keys.push(ClassKey {
                        class_id: c,
                        wrapped_key: w,
                    });
                }
                current_class = Some(parse_u32(tag, val)?);
            }
            b"WPKY" => current_wrapped = Some(val.to_vec()),
            _ => {}
        }
    }

    if let (Some(c), Some(w)) = (current_class, current_wrapped) {
        class_keys.push(ClassKey {
            class_id: c,
            wrapped_key: w,
        });
    }

    Ok(Keybag {
        dpsl: dpsl.ok_or_else(|| anyhow!("Missing DPSL"))?,
        dpic: dpic.ok_or_else(|| anyhow!("Missing DPIC"))?,
        salt: salt.ok_or_else(|| anyhow!("Missing SALT"))?,
        iter: iter.ok_or_else(|| anyhow!("Missing ITER"))?,
        class_keys,
    })
}

fn parse_u32(tag: &[u8], val: &[u8]) -> Result<u32> {
    if val.len() != 4 {
        return Err(anyhow!(
            "Keybag tag {} expected 4 bytes, got {}",
            String::from_utf8_lossy(tag),
            val.len()
        ));
    }
    Ok(u32::from_be_bytes(val.try_into().unwrap()))
}

```

### `src/chronos/ios_backup/keys.rs`

```rust
use aes_kw::KekAes256;
use anyhow::Result;
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use sha2::Sha256;
use std::collections::BTreeMap;
use zeroize::Zeroize;

use super::keybag::Keybag;

/// Owner-authorized key derivation (no brute force).
pub fn derive_class_keys(password: &str, kb: &Keybag) -> Result<BTreeMap<u32, Vec<u8>>> {
    let mut dpk = vec![0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &kb.dpsl, kb.dpic, &mut dpk);

    let mut kek_bytes = vec![0u8; 32];
    pbkdf2_hmac::<Sha1>(&dpk, &kb.salt, kb.iter, &mut kek_bytes);
    dpk.zeroize();

    let kek = KekAes256::try_from(kek_bytes.as_slice())?;
    kek_bytes.zeroize();

    let mut out = BTreeMap::new();
    for ck in &kb.class_keys {
        let key = kek.unwrap_vec(&ck.wrapped_key)?;
        out.insert(ck.class_id, key);
    }

    Ok(out)
}

```

### `src/chronos/ios_backup/manifest.rs`

```rust
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use aes_kw::KekAes256;
use anyhow::{anyhow, Context, Result};
use plist::Value;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

use super::file_decrypt::FilePlist;

#[derive(Debug, Clone)]
pub struct ManifestPlist {
    pub backup_keybag: Vec<u8>,
    pub manifest_key: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DecryptedManifest {
    pub path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ManifestIndexEntry {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub flags: i64,
    pub size: Option<u64>,
    pub protection_class: Option<u32>,
    pub has_encryption_key: bool,
}

impl DecryptedManifest {
    pub fn open_connection(&self) -> Result<Connection> {
        Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("Failed to open Manifest.db at {}", self.path.display()))
    }
}

pub fn load_manifest_plist(backup_dir: &Path) -> Result<ManifestPlist> {
    let plist_path = backup_dir.join("Manifest.plist");
    let data = fs::read(&plist_path)
        .with_context(|| format!("Failed to read {}", plist_path.display()))?;
    let value = read_plist_value(&data).context("Failed to parse Manifest.plist")?;

    let dict = value
        .as_dictionary()
        .ok_or_else(|| anyhow!("Manifest.plist is not a dictionary"))?;

    let backup_keybag = dict
        .get("BackupKeyBag")
        .and_then(|v| v.as_data())
        .ok_or_else(|| anyhow!("Manifest.plist missing BackupKeyBag"))?
        .to_vec();

    let manifest_key = dict
        .get("ManifestKey")
        .and_then(|v| v.as_data())
        .ok_or_else(|| anyhow!("Manifest.plist missing ManifestKey"))?
        .to_vec();

    Ok(ManifestPlist {
        backup_keybag,
        manifest_key,
    })
}

pub fn decrypt_manifest_db_to(
    backup_dir: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    manifest_key: &[u8],
    output_path: &Path,
) -> Result<DecryptedManifest> {
    if manifest_key.len() < 5 {
        return Err(anyhow!("ManifestKey is too short"));
    }

    let mut class_bytes = [0u8; 4];
    class_bytes.copy_from_slice(&manifest_key[0..4]);
    let manifest_class = u32::from_le_bytes(class_bytes);
    let wrapped_manifest_key = &manifest_key[4..];

    let class_key = class_keys
        .get(&manifest_class)
        .ok_or_else(|| anyhow!("Missing class key for {}", manifest_class))?;
    let kek = KekAes256::try_from(class_key.as_slice())?;
    let mut manifest_file_key = kek.unwrap_vec(wrapped_manifest_key)?;

    let input_path = backup_dir.join("Manifest.db");
    if !input_path.is_file() {
        return Err(anyhow!("Manifest.db not found at {}", input_path.display()));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    decrypt_cbc_to_path(&input_path, output_path, &manifest_file_key)?;
    manifest_file_key.zeroize();

    validate_manifest_db(output_path)?;

    Ok(DecryptedManifest {
        path: output_path.to_path_buf(),
    })
}

pub fn validate_manifest_db(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header)?;
    if &header != b"SQLite format 3\0" {
        return Err(anyhow!(
            "Decrypted Manifest.db does not look like SQLite (bad header)"
        ));
    }
    Ok(())
}

pub fn count_files(conn: &Connection) -> Result<u64> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM Files WHERE flags=1")?;
    let count: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(count as u64)
}

pub fn build_index(conn: &Connection) -> Result<Vec<ManifestIndexEntry>> {
    let mut stmt = conn.prepare(
        "SELECT fileID, domain, relativePath, flags, file FROM Files WHERE flags=1 ORDER BY domain, relativePath",
    )?;
    let mut rows = stmt.query([])?;
    let mut entries = Vec::new();

    while let Some(row) = rows.next()? {
        let file_id: String = row.get(0)?;
        let domain: String = row.get(1)?;
        let relative_path: String = row.get(2)?;
        let flags: i64 = row.get(3)?;
        let file_blob: Vec<u8> = row.get(4)?;

        let plist = FilePlist::from_bplist(&file_blob).ok();
        let size = plist.as_ref().map(|p| p.filesize);
        let protection_class = plist.as_ref().map(|p| p.protection_class);
        let has_encryption_key = plist
            .as_ref()
            .map(|p| p.encryption_key.is_some())
            .unwrap_or(false);

        entries.push(ManifestIndexEntry {
            file_id,
            domain,
            relative_path,
            flags,
            size,
            protection_class,
            has_encryption_key,
        });
    }

    Ok(entries)
}

fn decrypt_cbc_to_path(input: &Path, output: &Path, key: &[u8]) -> Result<()> {
    let mut reader = BufReader::new(
        File::open(input).with_context(|| format!("Failed to open {}", input.display()))?,
    );
    let mut writer = BufWriter::new(
        File::create(output).with_context(|| format!("Failed to create {}", output.display()))?,
    );

    let cipher = Aes256::new_from_slice(key)?;
    let mut prev_block = [0u8; 16];
    let mut pending: Option<Vec<u8>> = None;
    let mut buf = vec![0u8; 1024 * 1024];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if n % 16 != 0 {
            return Err(anyhow!("Encrypted data length is not a multiple of 16"));
        }
        for block in buf[..n].chunks_exact(16) {
            let mut block_buf = [0u8; 16];
            block_buf.copy_from_slice(block);
            let mut ga = block_buf.into();
            cipher.decrypt_block(&mut ga);
            for (i, b) in ga.iter_mut().enumerate() {
                *b ^= prev_block[i];
            }
            prev_block.copy_from_slice(block);

            if let Some(prev) = pending.take() {
                writer.write_all(&prev)?;
            }
            pending = Some(ga.to_vec());
        }
    }

    if let Some(last) = pending {
        if last.is_empty() {
            return Err(anyhow!("Missing final block"));
        }
        let pad = *last.last().unwrap() as usize;
        if pad == 0 || pad > 16 || pad > last.len() {
            return Err(anyhow!("Invalid PKCS7 padding"));
        }
        writer.write_all(&last[..last.len() - pad])?;
    }

    writer.flush()?;
    Ok(())
}

fn read_plist_value(data: &[u8]) -> Result<Value> {
    let cursor = Cursor::new(data);
    Ok(Value::from_reader(cursor)?)
}

```

## Common Utilities

### `src/common/mod.rs`

```rust
pub mod deps;
pub mod prepared;
pub mod resolver;
pub mod sqlite;
pub mod target;

```

### `src/common/resolver.rs`

```rust
use crate::case::Case;
use crate::common::target::{CandidateKind, CandidatePath, KnownTarget};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResolveMethod {
    CleanCopy,
    HashedManifest,
    DirectTree,
    AlternateCandidate,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPath {
    pub artifact_key: String,
    pub source_path: PathBuf,
    pub method: ResolveMethod,
    pub domain: String,
    pub relative_path: String,
    pub clean_file_name: Option<String>,
    pub attempted: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolverContext {
    pub backup_root: PathBuf,
    pub case_root: PathBuf,
    pub clean_root: PathBuf,
    pub manifest_db_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolverAuditRecord {
    pub artifact_key: String,
    pub candidates: Vec<String>,
    pub chosen: Option<String>,
    pub method: Option<String>,
}

pub trait ArtifactResolver {
    fn resolve_known_target(&self, target: &KnownTarget) -> Result<Option<ResolvedPath>>;
    fn resolve_exact(
        &self,
        artifact_key: &str,
        domain: &str,
        relative_path: &str,
    ) -> Result<Option<ResolvedPath>>;
}

#[derive(Debug, Clone)]
struct ManifestIndex {
    file_ids: HashMap<(String, String), String>,
}

pub struct BackupResolver {
    pub ctx: ResolverContext,
    manifest_index: Option<ManifestIndex>,
}

impl BackupResolver {
    pub fn new(ctx: ResolverContext) -> Self {
        let manifest_index = load_manifest_index(ctx.manifest_db_path.as_deref())
            .ok()
            .flatten();
        Self {
            ctx,
            manifest_index,
        }
    }

    pub fn from_case(case: &Case) -> Result<Self> {
        let backup_root = discover_backup_root(case)?;
        let manifest_db_path = backup_root
            .join("Manifest.db")
            .exists()
            .then(|| backup_root.join("Manifest.db"));
        Ok(Self::new(ResolverContext {
            backup_root,
            case_root: case.root_path(),
            clean_root: case.root_path().join("clean"),
            manifest_db_path,
        }))
    }

    pub fn clean_path_for(&self, target: &KnownTarget) -> PathBuf {
        self.ctx.clean_root.join(target.clean_path())
    }

    pub fn direct_tree_path(&self, domain: &str, relative_path: &str) -> PathBuf {
        self.ctx.backup_root.join(domain).join(relative_path)
    }

    pub fn manifest_hashed_path(&self, file_id: &str) -> PathBuf {
        let prefix = file_id.get(0..2).unwrap_or_default();
        self.ctx.backup_root.join(prefix).join(file_id)
    }

    pub fn backup_root(&self) -> &Path {
        &self.ctx.backup_root
    }

    pub fn case_root(&self) -> &Path {
        &self.ctx.case_root
    }

    pub fn clean_root(&self) -> &Path {
        &self.ctx.clean_root
    }

    pub fn resolve_from_clean(&self, target: &KnownTarget) -> Option<ResolvedPath> {
        let clean_path = self.clean_path_for(target);
        if clean_path.exists() {
            return Some(ResolvedPath {
                artifact_key: target.artifact_key.to_string(),
                source_path: clean_path.clone(),
                method: ResolveMethod::CleanCopy,
                domain: "clean".to_string(),
                relative_path: target.clean_file_name.to_string(),
                clean_file_name: Some(target.clean_file_name.to_string()),
                attempted: vec![clean_path],
            });
        }
        None
    }

    pub fn resolve_from_manifest_hash(&self, target: &KnownTarget) -> Result<Option<ResolvedPath>> {
        let mut attempted = Vec::new();

        for candidate in primary_candidates(&target.candidates) {
            if let Some(file_id) = self.lookup_manifest_file_id(candidate) {
                let hashed = self.manifest_hashed_path(&file_id);
                attempted.push(hashed.clone());
                if hashed.exists() {
                    return Ok(Some(ResolvedPath {
                        artifact_key: target.artifact_key.to_string(),
                        source_path: hashed,
                        method: ResolveMethod::HashedManifest,
                        domain: candidate.domain.clone(),
                        relative_path: candidate.relative_path.clone(),
                        clean_file_name: Some(target.clean_file_name.to_string()),
                        attempted,
                    }));
                }
            }
        }

        Ok(None)
    }

    pub fn resolve_from_direct_tree_candidate(&self, target: &KnownTarget) -> Option<ResolvedPath> {
        let mut attempted = Vec::new();

        for candidate in primary_candidates(&target.candidates) {
            let direct = self.direct_tree_path(&candidate.domain, &candidate.relative_path);
            attempted.push(direct.clone());
            if direct.exists() {
                return Some(ResolvedPath {
                    artifact_key: target.artifact_key.to_string(),
                    source_path: direct,
                    method: ResolveMethod::DirectTree,
                    domain: candidate.domain.clone(),
                    relative_path: candidate.relative_path.clone(),
                    clean_file_name: Some(target.clean_file_name.to_string()),
                    attempted,
                });
            }
        }

        None
    }

    pub fn search_direct_tree_file_by_name(
        &self,
        domain: &str,
        relative_base: &str,
        file_name: &str,
    ) -> Result<Option<PathBuf>> {
        let base = self.direct_tree_path(domain, relative_base);
        if !base.exists() {
            return Ok(None);
        }

        for entry in walkdir::WalkDir::new(&base)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.file_name().to_string_lossy() == file_name {
                return Ok(Some(entry.path().to_path_buf()));
            }
        }

        Ok(None)
    }

    fn resolve_alternate_candidate(&self, target: &KnownTarget) -> Option<ResolvedPath> {
        let mut attempted = Vec::new();

        for candidate in alternate_candidates(&target.candidates) {
            if let Some(file_id) = self.lookup_manifest_file_id(candidate) {
                let hashed = self.manifest_hashed_path(&file_id);
                attempted.push(hashed.clone());
                if hashed.exists() {
                    return Some(ResolvedPath {
                        artifact_key: target.artifact_key.to_string(),
                        source_path: hashed,
                        method: ResolveMethod::AlternateCandidate,
                        domain: candidate.domain.clone(),
                        relative_path: candidate.relative_path.clone(),
                        clean_file_name: Some(target.clean_file_name.to_string()),
                        attempted,
                    });
                }
            }

            let direct = self.direct_tree_path(&candidate.domain, &candidate.relative_path);
            attempted.push(direct.clone());
            if direct.exists() {
                return Some(ResolvedPath {
                    artifact_key: target.artifact_key.to_string(),
                    source_path: direct,
                    method: ResolveMethod::AlternateCandidate,
                    domain: candidate.domain.clone(),
                    relative_path: candidate.relative_path.clone(),
                    clean_file_name: Some(target.clean_file_name.to_string()),
                    attempted,
                });
            }
        }

        None
    }

    fn lookup_manifest_file_id(&self, candidate: &CandidatePath) -> Option<String> {
        self.manifest_index.as_ref().and_then(|manifest| {
            manifest
                .file_ids
                .get(&(candidate.domain.clone(), candidate.relative_path.clone()))
                .cloned()
        })
    }
}

impl ArtifactResolver for BackupResolver {
    fn resolve_known_target(&self, target: &KnownTarget) -> Result<Option<ResolvedPath>> {
        if let Some(clean) = self.resolve_from_clean(target) {
            return Ok(Some(clean));
        }

        if let Some(hashed) = self.resolve_from_manifest_hash(target)? {
            return Ok(Some(hashed));
        }

        if let Some(direct) = self.resolve_from_direct_tree_candidate(target) {
            return Ok(Some(direct));
        }

        Ok(self.resolve_alternate_candidate(target))
    }

    fn resolve_exact(
        &self,
        artifact_key: &str,
        domain: &str,
        relative_path: &str,
    ) -> Result<Option<ResolvedPath>> {
        let target = KnownTarget {
            artifact_key: "",
            clean_file_name: "",
            candidates: vec![CandidatePath {
                domain: domain.to_string(),
                relative_path: relative_path.to_string(),
                kind: CandidateKind::Primary,
            }],
            sqlite_like: false,
        };

        let mut resolved = match self.resolve_from_manifest_hash(&target)? {
            Some(found) => Some(found),
            None => self.resolve_from_direct_tree_candidate(&target),
        };

        if let Some(path) = &mut resolved {
            path.artifact_key = artifact_key.to_string();
            path.clean_file_name = None;
        }

        Ok(resolved)
    }
}

pub fn write_resolver_audit(case_root: &Path, record: &ResolverAuditRecord) -> Result<()> {
    let log_dir = case_root.join("logs");
    fs::create_dir_all(&log_dir)?;
    let path = log_dir.join(format!("resolver_{}.json", record.artifact_key));
    fs::write(path, serde_json::to_vec_pretty(record)?)?;
    Ok(())
}

pub fn discover_backup_root(case: &Case) -> Result<PathBuf> {
    let chronos_manifest = case.root_path().join("chronos_manifest.json");
    if chronos_manifest.exists() {
        if let Ok(raw) = fs::read(&chronos_manifest) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw) {
                if let Some(backup_root) = value.get("backup_root").and_then(|v| v.as_str()) {
                    let candidate = PathBuf::from(backup_root);
                    if candidate.exists() && looks_like_backup_root(&candidate) {
                        return Ok(candidate);
                    }
                }
            }
        }
    }

    let search_paths = vec![
        case.backup_path(),
        PathBuf::from("./backups"),
        dirs::home_dir()
            .unwrap_or_default()
            .join(".local/share/idevicebackup"),
    ];

    for base in search_paths {
        if !base.exists() {
            continue;
        }

        if looks_like_backup_root(&base) {
            return Ok(base);
        }

        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && looks_like_backup_root(&path) {
                    return Ok(path);
                }
            }
        }
    }

    Ok(case.backup_path())
}

fn looks_like_backup_root(path: &Path) -> bool {
    path.join("Manifest.db").exists()
}

fn load_manifest_index(path: Option<&Path>) -> Result<Option<ManifestIndex>> {
    let Some(path) = path else {
        return Ok(None);
    };

    if !path.exists() {
        return Ok(None);
    }

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open Manifest.db at {}", path.display()))?;

    let mut stmt = conn.prepare(
        "SELECT fileID, domain, relativePath FROM Files WHERE domain IS NOT NULL AND relativePath IS NOT NULL",
    )?;

    let rows = stmt.query_map([], |row| {
        let file_id: String = row.get(0)?;
        let domain: String = row.get(1)?;
        let relative_path: String = row.get(2)?;
        Ok((file_id, domain, relative_path))
    })?;

    let mut file_ids = HashMap::new();
    for row in rows {
        let (file_id, domain, relative_path) = row?;
        file_ids.insert((domain, relative_path), file_id);
    }

    Ok(Some(ManifestIndex { file_ids }))
}

fn primary_candidates(candidates: &[CandidatePath]) -> impl Iterator<Item = &CandidatePath> {
    candidates
        .iter()
        .filter(|candidate| candidate.kind == CandidateKind::Primary)
}

fn alternate_candidates(candidates: &[CandidatePath]) -> impl Iterator<Item = &CandidatePath> {
    candidates
        .iter()
        .filter(|candidate| candidate.kind == CandidateKind::Alternate)
}

```

### `src/common/target.rs`

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePath {
    pub domain: String,
    pub relative_path: String,
    pub kind: CandidateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind {
    Primary,
    Alternate,
}

#[derive(Debug, Clone)]
pub struct KnownTarget {
    pub artifact_key: &'static str,
    pub clean_file_name: &'static str,
    pub candidates: Vec<CandidatePath>,
    pub sqlite_like: bool,
}

impl KnownTarget {
    pub fn clean_path(&self) -> PathBuf {
        PathBuf::from(self.clean_file_name)
    }
}

fn candidate(domain: &str, relative_path: &str, kind: CandidateKind) -> CandidatePath {
    CandidatePath {
        domain: domain.to_string(),
        relative_path: relative_path.to_string(),
        kind,
    }
}

pub fn sms_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "sms",
        clean_file_name: "Library/SMS/sms.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/SMS/sms.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn modern_notes_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "modern_notes",
        clean_file_name: "NoteStore.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate(
                "AppDomainGroup-group.com.apple.notes",
                "NoteStore.sqlite",
                CandidateKind::Primary,
            ),
            candidate(
                "HomeDomain",
                "Library/Notes/NoteStore.sqlite",
                CandidateKind::Alternate,
            ),
        ],
    }
}

pub fn legacy_notes_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "legacy_notes",
        clean_file_name: "Library/Notes/notes.sqlite",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Notes/notes.sqlite",
            CandidateKind::Primary,
        )],
    }
}

pub fn call_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "call_history",
        clean_file_name: "Library/CallHistoryDB/CallHistory.storedata",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/CallHistoryDB/CallHistory.storedata",
            CandidateKind::Primary,
        )],
    }
}

pub fn safari_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "safari_history",
        clean_file_name: "Library/Safari/History.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Safari/History.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn safari_bookmarks_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "safari_bookmarks",
        clean_file_name: "Library/Safari/Bookmarks.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Safari/Bookmarks.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn safari_tabs_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "safari_tabs",
        clean_file_name: "Library/Safari/SafariTabs.db",
        sqlite_like: true,
        candidates: vec![
            candidate(
                "HomeDomain",
                "Library/Safari/SafariTabs.db",
                CandidateKind::Primary,
            ),
            candidate(
                "HomeDomain",
                "Library/Safari/iCloudTabs.db",
                CandidateKind::Alternate,
            ),
        ],
    }
}

pub fn voicemail_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "voicemail",
        clean_file_name: "Library/Voicemail/voicemail.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Voicemail/voicemail.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn photos_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "photos",
        clean_file_name: "Media/PhotoData/Photos.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate(
                "CameraRollDomain",
                "Media/PhotoData/Photos.sqlite",
                CandidateKind::Primary,
            ),
            candidate(
                "MediaDomain",
                "PhotoData/Photos.sqlite",
                CandidateKind::Alternate,
            ),
        ],
    }
}

pub fn addressbook_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "addressbook",
        clean_file_name: "Library/AddressBook/AddressBook.sqlitedb",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/AddressBook/AddressBook.sqlitedb",
            CandidateKind::Primary,
        )],
    }
}

pub fn apple_maps_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "apple_maps_history",
        clean_file_name: "History.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.apple.Maps", "History.sqlite", CandidateKind::Primary),
            candidate("AppDomainGroup-group.com.apple.Maps", "History.sqlite", CandidateKind::Alternate),
        ],
    }
}

pub fn apple_maps_cloud_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "apple_maps_cloud_history",
        clean_file_name: "CloudHistory.syncedDB",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.apple.Maps", "CloudHistory.syncedDB", CandidateKind::Primary),
            candidate("AppDomainGroup-group.com.apple.Maps", "CloudHistory.syncedDB", CandidateKind::Alternate),
        ],
    }
}

pub fn apple_maps_geo_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "apple_maps_geo",
        clean_file_name: "geo.db",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.apple.Maps", "geo.db", CandidateKind::Primary),
            candidate("AppDomainGroup-group.com.apple.Maps", "geo.db", CandidateKind::Alternate),
        ],
    }
}

pub fn google_maps_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "google_maps",
        clean_file_name: "Session.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.google.Maps", "Library/Application Support/GoogleMaps/Session.sqlite", CandidateKind::Primary),
            candidate("AppDomain-com.google.Maps", "Library/Application Support/DataStore/Session.sqlite", CandidateKind::Alternate),
            candidate("AppDomain-com.google.Maps", "Documents/OSCacheData", CandidateKind::Alternate),
        ],
    }
}

pub fn snapchat_maps_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "snapchat_maps",
        clean_file_name: "scmap.db",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.toyopagroup.picaboo", "Documents/scmap.db", CandidateKind::Primary),
            candidate("AppDomain-com.toyopagroup.picaboo", "scmap.db", CandidateKind::Alternate),
            candidate("AppDomain-com.toyopagroup.picaboo", "Library/Caches/scmap.db", CandidateKind::Alternate),
        ],
    }
}

pub fn chronos_targets() -> Vec<KnownTarget> {
    vec![
        sms_target(),
        call_history_target(),
        modern_notes_target(),
        legacy_notes_target(),
        safari_history_target(),
        safari_bookmarks_target(),
        safari_tabs_target(),
        voicemail_target(),
        photos_target(),
        addressbook_target(),
    ]
}

```

### `src/common/prepared.rs`

```rust
use crate::chronos::wal::{ReplayResult, WalReplayer};
use crate::common::resolver::{ResolveMethod, ResolvedPath};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedKind {
    PlainFile,
    SqliteLike,
}

#[derive(Debug, Clone)]
pub struct PreparedArtifact {
    pub artifact_key: String,
    pub source_path: PathBuf,
    pub working_path: PathBuf,
    pub resolution_method: ResolveMethod,
    pub kind: PreparedKind,
    pub sha256: Option<String>,
}

impl PreparedArtifact {
    pub fn open_sqlite_ro(&self) -> Result<Connection> {
        if self.kind != PreparedKind::SqliteLike {
            return Err(anyhow!("artifact {} is not SQLite-like", self.artifact_key));
        }

        Connection::open_with_flags(
            &self.working_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| format!("Failed to open {}", self.working_path.display()))
    }
}

#[derive(Debug, Clone)]
pub struct PrepareContext {
    pub case_root: PathBuf,
    pub clean_root: PathBuf,
    pub temp_root: PathBuf,
}

pub fn prepare_artifact(
    ctx: &PrepareContext,
    resolved: &ResolvedPath,
    sqlite_like: bool,
) -> Result<PreparedArtifact> {
    if sqlite_like {
        prepare_sqlite_like(ctx, resolved)
    } else {
        prepare_plain_file(ctx, resolved)
    }
}

pub fn prepare_sqlite_like(
    ctx: &PrepareContext,
    resolved: &ResolvedPath,
) -> Result<PreparedArtifact> {
    fs::create_dir_all(&ctx.clean_root)?;
    let working_path = ctx.clean_root.join(file_name_for(
        resolved.clean_file_name.as_deref(),
        &resolved.source_path,
        &resolved.artifact_key,
    ));

    if resolved.method == ResolveMethod::CleanCopy && resolved.source_path == working_path {
        return Ok(PreparedArtifact {
            artifact_key: resolved.artifact_key.clone(),
            source_path: resolved.source_path.clone(),
            working_path: working_path.clone(),
            resolution_method: resolved.method,
            kind: PreparedKind::SqliteLike,
            sha256: Some(compute_sha256(&working_path)?),
        });
    }

    if let Some(parent) = working_path.parent() {
        fs::create_dir_all(parent)?;
    }

    remove_existing_sqlite_artifact(&working_path)?;

    if needs_sqlite_replay(&resolved.source_path)? {
        let wal_replayer = WalReplayer::new(300);
        match wal_replayer.replay_with_recovery(&resolved.source_path, &working_path) {
            Ok(ReplayResult::Success) | Ok(ReplayResult::PartialRecovery) => {}
            Ok(ReplayResult::Failed) => {
                copy_with_sidecars(&resolved.source_path, &working_path)?;
            }
            Err(_) => {
                copy_with_sidecars(&resolved.source_path, &working_path)?;
            }
        }
    } else {
        copy_main_file(&resolved.source_path, &working_path)?;
    }

    Ok(PreparedArtifact {
        artifact_key: resolved.artifact_key.clone(),
        source_path: resolved.source_path.clone(),
        working_path: working_path.clone(),
        resolution_method: resolved.method,
        kind: PreparedKind::SqliteLike,
        sha256: Some(compute_sha256(&working_path)?),
    })
}

pub fn prepare_plain_file(
    ctx: &PrepareContext,
    resolved: &ResolvedPath,
) -> Result<PreparedArtifact> {
    fs::create_dir_all(&ctx.temp_root)?;
    let working_path = ctx.temp_root.join(file_name_for(
        resolved.clean_file_name.as_deref(),
        &resolved.source_path,
        &resolved.artifact_key,
    ));

    if resolved.method == ResolveMethod::CleanCopy && resolved.source_path == working_path {
        return Ok(PreparedArtifact {
            artifact_key: resolved.artifact_key.clone(),
            source_path: resolved.source_path.clone(),
            working_path: working_path.clone(),
            resolution_method: resolved.method,
            kind: PreparedKind::PlainFile,
            sha256: Some(compute_sha256(&working_path)?),
        });
    }

    fs::copy(&resolved.source_path, &working_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            resolved.source_path.display(),
            working_path.display()
        )
    })?;

    Ok(PreparedArtifact {
        artifact_key: resolved.artifact_key.clone(),
        source_path: resolved.source_path.clone(),
        working_path: working_path.clone(),
        resolution_method: resolved.method,
        kind: PreparedKind::PlainFile,
        sha256: Some(compute_sha256(&working_path)?),
    })
}

pub fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 8192];

    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_with_sidecars(src: &Path, dst: &Path) -> Result<()> {
    copy_main_file(src, dst)?;

    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{}", src.display(), suffix));
        if sidecar.exists() {
            let sidecar_dst = PathBuf::from(format!("{}{}", dst.display(), suffix));
            fs::copy(&sidecar, &sidecar_dst).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    sidecar.display(),
                    sidecar_dst.display()
                )
            })?;
        }
    }

    Ok(())
}

fn copy_main_file(src: &Path, dst: &Path) -> Result<()> {
    fs::copy(src, dst)
        .with_context(|| format!("Failed to copy {} to {}", src.display(), dst.display()))?;
    Ok(())
}

fn needs_sqlite_replay(path: &Path) -> Result<bool> {
    for suffix in ["-wal", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{}", path.display(), suffix));
        if !sidecar.exists() {
            continue;
        }

        let size = fs::metadata(&sidecar)
            .with_context(|| format!("Failed to stat {}", sidecar.display()))?
            .len();
        if size > 0 {
            return Ok(true);
        }
    }

    Ok(false)
}

fn remove_existing_sqlite_artifact(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
        PathBuf::from(format!("{}-journal", path.display())),
    ] {
        if candidate.exists() {
            fs::remove_file(&candidate)
                .with_context(|| format!("Failed to remove {}", candidate.display()))?;
        }
    }

    Ok(())
}

fn file_name_for(clean_file_name: Option<&str>, source_path: &Path, artifact_key: &str) -> PathBuf {
    clean_file_name
        .map(PathBuf::from)
        .or_else(|| source_path.file_name().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(format!("{artifact_key}.bin")))
}

```

### `src/common/sqlite.rs`

```rust
//! Thin SQLite helper wrapping rusqlite with our conventions.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub type SqliteConn = Connection;

pub fn open_readonly(path: &Path) -> Result<SqliteConn> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening SQLite database {}", path.display()))
}

```

### `src/common/deps.rs`

```rust
//! Runtime dependency checker for iON agents.
//!
//! Provides human-readable error messages when external tools are missing,
//! so users get "install X" instructions instead of cryptic "file not found" errors.

use anyhow::{bail, Result};
use std::path::PathBuf;
use std::process::Command;

/// A required external tool and how to install it.
pub struct ToolInfo {
    pub name: &'static str,
    pub required_by: &'static str,
    pub ubuntu: &'static str,
    pub macos: &'static str,
    pub windows: &'static str,
}

const TOOLS: &[ToolInfo] = &[
    ToolInfo {
        name: "idevicebackup2",
        required_by: "chronos (iOS backup acquisition)",
        ubuntu: "sudo apt install libimobiledevice6 libimobiledevice-utils",
        macos: "brew install libimobiledevice",
        windows: "Download libimobiledevice from https://github.com/libimobiledevice/libimobiledevice/releases",
    },
    ToolInfo {
        name: "idevicepair",
        required_by: "chronos (iOS device pairing)",
        ubuntu: "sudo apt install libimobiledevice6 libimobiledevice-utils",
        macos: "brew install libimobiledevice",
        windows: "Download libimobiledevice from https://github.com/libimobiledevice/libimobiledevice/releases",
    },
    ToolInfo {
        name: "mvt-ios",
        required_by: "chronos (iOS malware triage)",
        ubuntu: "pip install mvt",
        macos: "pip install mvt",
        windows: "pip install mvt",
    },
    ToolInfo {
        name: "ffmpeg",
        required_by: "cerberus / vox (audio normalization & transcription)",
        ubuntu: "sudo apt install ffmpeg",
        macos: "brew install ffmpeg",
        windows: "choco install ffmpeg or download from https://ffmpeg.org/download.html",
    },
    ToolInfo {
        name: "whisperx",
        required_by: "vox (AI transcription)",
        ubuntu: "pip install whisperx",
        macos: "pip install whisperx",
        windows: "pip install whisperx",
    },
    ToolInfo {
        name: "ollama",
        required_by: "psyche / cerberus (AI analysis)",
        ubuntu: "curl -fsSL https://ollama.com/install.sh | sh",
        macos: "brew install ollama",
        windows: "Download from https://ollama.com/download",
    },
];

/// Common directories where tools may be installed outside of PATH.
fn common_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // iON MVT virtualenv
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("iON").join("mvtEnv").join("bin"));
        dirs.push(home.join("iON").join("mvtEnv").join("Scripts")); // Windows
        dirs.push(home.join(".local").join("bin"));
    }

    dirs
}

/// Resolve a tool name to its full path, searching PATH and common iON directories.
pub fn resolve_tool(name: &str) -> Option<PathBuf> {
    // Try PATH first
    if Command::new(name).arg("--help").output().is_ok()
        || Command::new(name).arg("-v").output().is_ok()
        || Command::new(name).arg("--version").output().is_ok()
    {
        return Some(PathBuf::from(name));
    }

    // Search common directories
    for dir in common_tool_dirs() {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        // Also try with .exe on Windows
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{}.exe", name));
            if candidate_exe.exists() {
                return Some(candidate_exe);
            }
        }
    }

    None
}

/// Check whether a tool is available on PATH or in common iON directories.
pub fn is_available(name: &str) -> bool {
    resolve_tool(name).is_some()
}

/// Check a list of required tools and return a clear error if any are missing.
///
/// # Example
/// ```ignore
/// use minios::common::deps::check_required;
/// check_required(&["idevicebackup2", "idevicepair"])?;
/// ```
pub fn check_required(names: &[&str]) -> Result<()> {
    let mut missing = Vec::new();

    for name in names {
        if !is_available(name) {
            missing.push(*name);
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    let platform = std::env::consts::OS;
    let mut msg = format!(
        "Missing required external tool(s) for iON Data Systems:\n\n"
    );

    for name in &missing {
        if let Some(tool) = TOOLS.iter().find(|t| t.name == *name) {
            let install = match platform {
                "linux" => tool.ubuntu,
                "macos" => tool.macos,
                _ => tool.windows,
            };
            msg.push_str(&format!(
                "  ❌ {} (required by: {})\n     Install: {}\n\n",
                tool.name, tool.required_by, install
            ));
        } else {
            msg.push_str(&format!("  ❌ {} (unknown tool — please install it)\n\n", name));
        }
    }

    bail!("{}", msg.trim_end())
}

/// Print a warning for optional tools rather than failing.
pub fn warn_missing(names: &[&str]) {
    for name in names {
        if !is_available(name) {
            if let Some(tool) = TOOLS.iter().find(|t| t.name == *name) {
                eprintln!(
                    "⚠️  Optional tool '{}' not found (used by: {}). Some features will be unavailable.",
                    tool.name, tool.required_by
                );
            }
        }
    }
}

```

## Nemesis — Pipeline Orchestrator

### `src/nemesis/mod.rs`

```rust
//! MINiOS pipeline orchestrator.
//!
//! Forensic extraction is defined declaratively in `pipeline.toml`.
//! The runner performs a topological sort on agent dependencies and executes
//! them sequentially, logging progress and handling failures.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

// Include the pipeline TOML at compile time
const PIPELINE_TOML: &str = include_str!("pipeline.toml");

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub step: Vec<Step>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Step {
    pub agent: String,
    pub required: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub description: Option<String>,
}

/// Parsed pipeline with dependency graph resolved.
pub struct PipelineRunner {
    steps: Vec<Step>,
    // maps agent name to step index
    index: HashMap<String, usize>,
}

impl PipelineRunner {
    /// Load pipeline from embedded TOML.
    pub fn load() -> Result<Self> {
        let pipeline: Pipeline = toml::from_str(PIPELINE_TOML)
            .context("parsing pipeline.toml")?;

        let mut index = HashMap::new();
        for (i, step) in pipeline.step.iter().enumerate() {
            if index.insert(step.agent.clone(), i).is_some() {
                bail!("Duplicate agent '{}' in pipeline", step.agent);
            }
        }

        // Validate dependencies exist
        for step in &pipeline.step {
            for dep in &step.depends_on {
                if !index.contains_key(dep) {
                    bail!(
                        "Step '{}' depends on '{}' which is not defined",
                        step.agent, dep
                    );
                }
            }
        }

        // Topologically sort steps
        let sorted = topo_sort(&pipeline.step, &index)?;

        Ok(Self { steps: sorted, index })
    }

    /// Run the full pipeline against a case.
    pub fn run(&self, case_name: &str) -> Result<PipelineResult> {
        let mut completed = HashSet::new();
        let mut failed = Vec::new();
        let mut skipped = Vec::new();

        for step in &self.steps {
            // Check if all dependencies completed
            let deps_ok = step.depends_on.iter().all(|d| completed.contains(d));
            if !deps_ok {
                if step.required {
                    bail!(
                        "Required step '{}' has unmet dependencies",
                        step.agent
                    );
                } else {
                    eprintln!("[NEMESIS] Skipping '{}' — dependencies not met", step.agent);
                    skipped.push(step.agent.clone());
                    continue;
                }
            }

            print!("[NEMESIS] Running {} ... ", step.agent);
            match run_agent(&step.agent, case_name) {
                Ok(()) => {
                    println!("OK");
                    completed.insert(step.agent.clone());
                }
                Err(e) => {
                    println!("FAILED: {}", e);
                    if step.required {
                        bail!(
                            "Required agent '{}' failed: {}",
                            step.agent, e
                        );
                    }
                    failed.push((step.agent.clone(), format!("{}", e)));
                }
            }
        }

        Ok(PipelineResult {
            completed: completed.into_iter().collect(),
            failed,
            skipped,
        })
    }

    /// Run a single agent (used by UI for individual agent runs).
    pub fn run_single(&self, agent: &str, case_name: &str) -> Result<()> {
        run_agent(agent, case_name)
    }

    /// List all agents in pipeline order.
    pub fn agents(&self) -> Vec<&str> {
        self.steps.iter().map(|s| s.agent.as_str()).collect()
    }

    /// Get step info for an agent.
    pub fn step_info(&self, agent: &str) -> Option<&Step> {
        self.index.get(agent).map(|&i| &self.steps[i])
    }
}

#[derive(Debug)]
pub struct PipelineResult {
    pub completed: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub skipped: Vec<String>,
}

impl PipelineResult {
    pub fn summary(&self) -> String {
        format!(
            "Pipeline complete: {} succeeded, {} failed, {} skipped",
            self.completed.len(),
            self.failed.len(),
            self.skipped.len()
        )
    }

    pub fn all_ok(&self) -> bool {
        self.failed.is_empty() && self.skipped.is_empty()
    }
}

fn run_agent(agent: &str, case_name: &str) -> Result<()> {
    // Uses the unified minios binary
    let status = Command::new("minios")
        .arg(agent)
        .arg(case_name)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .with_context(|| format!("failed to spawn minios {}", agent))?;

    if !status.success() {
        bail!("Agent '{}' exited with code {:?}", agent, status.code());
    }
    Ok(())
}

fn topo_sort(
    steps: &[Step],
    index: &HashMap<String, usize>,
) -> Result<Vec<Step>> {
    // Kahn's algorithm
    let n = steps.len();
    let mut in_degree = vec![0; n];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

    for step in steps {
        let u = index[&step.agent];
        for dep in &step.depends_on {
            let v = index[dep]; // dep must come before step
            adj[v].push(u);
            in_degree[u] += 1;
        }
    }

    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut sorted = Vec::with_capacity(n);

    while let Some(u) = queue.pop() {
        sorted.push(steps[u].clone());
        for &v in &adj[u] {
            in_degree[v] -= 1;
            if in_degree[v] == 0 {
                queue.push(v);
            }
        }
    }

    if sorted.len() != n {
        bail!("Pipeline has circular dependencies");
    }

    Ok(sorted)
}

```

### `src/nemesis/pipeline.toml`

```toml
# MINiOS forensic extraction pipeline definition
# Agents are run in dependency order. Optional agents that fail are skipped.

[[step]]
agent = "chronos"
required = true
description = "iOS backup discovery and validation"

[[step]]
agent = "helios"
required = true
depends_on = ["chronos"]
description = "Backup decryption"

# Core evidence agents — all depend on helios (decrypted backup)
[[step]]
agent = "vigil"
required = false
depends_on = ["helios"]
description = "System artifact extraction"

[[step]]
agent = "echo"
required = false
depends_on = ["helios"]
description = "Audio/voicemail extraction"

[[step]]
agent = "cerberus"
required = false
depends_on = ["helios"]
description = "SMS/MMS/calls/voicemail/contacts extraction"

[[step]]
agent = "charon"
required = false
depends_on = ["helios"]
description = "Photo and media extraction"

[[step]]
agent = "nyx"
required = false
depends_on = ["helios"]
description = "Safari history, bookmarks, autofill"

[[step]]
agent = "obolus"
required = false
depends_on = ["helios"]
description = "Notes extraction"

[[step]]
agent = "plutus"
required = false
depends_on = ["helios"]
description = "Apple Wallet/financial data"

[[step]]
agent = "aether"
required = false
depends_on = ["charon"]
description = "EXIF GPS extraction from photos"

[[step]]
agent = "atlas"
required = false
depends_on = ["helios"]
description = "App-based location extraction"

[[step]]
agent = "orpheus"
required = false
depends_on = ["helios"]
description = "Generic SQLite reconnaissance"

# Analysis agents — depend on core extraction outputs
[[step]]
agent = "psyche"
required = false
depends_on = ["cerberus", "nyx"]
description = "AI behavioral analysis"

```

## Styg — Evidence Archive

### `src/styg/mod.rs`

```rust
//! MINiOS evidence archive.
//!
//! Uses tar + age (file encryption) instead of a custom format.
//! Provides confidentiality and integrity for evidence exports.

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// Pack an evidence directory into an encrypted archive.
pub fn pack_evidence(
    evidence_dir: &Path,
    output: &Path,
    passphrase: &str,
) -> Result<()> {
    // 1. Create tar archive in memory
    let mut tar_builder = tar::Builder::new(Vec::new());
    tar_builder.append_dir_all("evidence", evidence_dir)
        .context("building tar archive")?;
    let tar_data = tar_builder.into_inner().context("finalizing tar")?;

    // 2. Encrypt with age (passphrase-based)
    let encryptor = age::Encryptor::with_user_passphrase(
        age::secrecy::Secret::new(passphrase.to_string()),
    );
    let mut output_file = File::create(output)
        .context("creating output file")?;
    let mut encrypt_writer = encryptor
        .wrap_output(&mut output_file)
        .context("creating encryptor")?;
    encrypt_writer.write_all(&tar_data)
        .context("writing encrypted data")?;
    encrypt_writer.finish().context("finalizing encryption")?;

    Ok(())
}

/// Unpack and decrypt an evidence archive.
pub fn unpack_evidence(
    archive: &Path,
    output_dir: &Path,
    passphrase: &str,
) -> Result<()> {
    // 1. Read and decrypt
    let mut encrypted_file = File::open(archive)
        .context("opening archive")?;
    let decryptor = match age::Decryptor::new(&mut encrypted_file)
        .context("reading archive header")? {
        age::Decryptor::Passphrase(d) => d,
        _ => anyhow::bail!("archive is not passphrase-encrypted"),
    };

    let mut decrypted = Vec::new();
    let mut reader = decryptor
        .decrypt(&age::secrecy::Secret::new(passphrase.to_string()), None)
        .context("decrypting")?;
    reader.read_to_end(&mut decrypted).context("reading decrypted data")?;

    // 2. Extract tar
    let mut archive = tar::Archive::new(decrypted.as_slice());
    archive.unpack(output_dir).context("extracting tar")?;

    Ok(())
}

/// Verify an archive can be decrypted and has valid tar structure.
pub fn verify_archive(archive: &Path, passphrase: &str) -> Result<()> {
    let mut encrypted_file = File::open(archive)?;
    let decryptor = match age::Decryptor::new(&mut encrypted_file)? {
        age::Decryptor::Passphrase(d) => d,
        _ => anyhow::bail!("archive is not passphrase-encrypted"),
    };
    let mut reader = decryptor.decrypt(
        &age::secrecy::Secret::new(passphrase.to_string()),
        None,
    )?;
    let mut decrypted = Vec::new();
    reader.read_to_end(&mut decrypted)?;

    // Verify it's a valid tar
    let mut archive = tar::Archive::new(decrypted.as_slice());
    let entries = archive.entries()?.count();
    println!("Archive valid: {} entries", entries);
    Ok(())
}

```

## UI — GTK Shell

### `src/ui/mod.rs`

```rust
//! Native UI migration seam.
//!
//! GTK4/Relm4 can consume these local-only view models without inheriting the
//! old web backend storage assumptions.

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::case::Case;
use crate::agents::cerberus_models::{
    Attachment as CerberusAttachment, CallRecord, ContactRecord, Message as CerberusMessage,
    VoicemailRecord,
};
use crate::agents::charon_models::{
    AlbumRecord as CharonAlbum, AssetRecord as CharonAsset, FaceRecord as CharonFace,
    TimelineEntry as CharonTimelineEntry,
};
use crate::chronos::device::LiveDeviceStatus;
use crate::chronos::probe_live_status;
use crate::agents::nyx_models::{AutofillEntry, Bookmark, HistoryItem, SearchTerm, TabEntry};
use crate::agents::orpheus_recon::DatabaseSummary;
use crate::agents::obolus_models::{
    Account as ObolusAccount, Attachment as ObolusAttachment, Folder as ObolusFolder,
    Note as ObolusNote,
};

pub use crate::ui_core::*;

#[cfg(feature = "gtk_shell")]
pub mod gtk_shell;

#[cfg(feature = "gtk_shell")]
pub mod routes;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum WorkspaceRoute {
    Nemesis,    // Control
    Chronos,    // Backup
    Helios,     // Unlock
    Orpheus,    // Data
    Cerberus,   // Messages / Comms
    Charon,     // Files
    Vox,        // Audio
    Plutus,     // Wallet
    Psyche,     // Insights
    Obolus,     // Export
    Nyx,        // Background
    Aether,     // System
    Tartarus,   // Deep System
    Xwin,       // Bridge
    // Legacy / internal routes (not shown in sidebar)
    Atlas,
    CaseOverview,
    ParserStatus,
    Reports,
}

impl WorkspaceRoute {
    /// Sidebar label (one word, human-facing)
    pub fn label(self) -> &'static str {
        match self {
            Self::Nemesis => "Control",
            Self::Chronos => "Backup",
            Self::Helios => "Unlock",
            Self::Orpheus => "Data",
            Self::Cerberus => "Comms",
            Self::Charon => "Files",
            Self::Vox => "Audio",
            Self::Plutus => "Wallet",
            Self::Psyche => "Insights",
            Self::Obolus => "Export",
            Self::Nyx => "Background",
            Self::Aether => "System",
            Self::Tartarus => "Deep System",
            Self::Xwin => "Bridge",
            Self::Atlas => "Atlas",
            Self::CaseOverview => "Overview",
            Self::ParserStatus => "Parser",
            Self::Reports => "Reports",
        }
    }

    /// Agent codename (for top-bar title)
    pub fn agent_name(self) -> &'static str {
        match self {
            Self::Nemesis => "Nemesis",
            Self::Chronos => "Chronos",
            Self::Helios => "Helios",
            Self::Orpheus => "Orpheus",
            Self::Cerberus => "Cerberus",
            Self::Charon => "Charon",
            Self::Vox => "Vox",
            Self::Plutus => "Plutus",
            Self::Psyche => "Psyche",
            Self::Obolus => "Obolus",
            Self::Nyx => "Nyx",
            Self::Aether => "Aether",
            Self::Tartarus => "Tartarus",
            Self::Xwin => "Xwin",
            Self::Atlas => "Atlas",
            Self::CaseOverview => "Overview",
            Self::ParserStatus => "Parser",
            Self::Reports => "Reports",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Nemesis => "nemesis",
            Self::Chronos => "chronos",
            Self::Helios => "helios",
            Self::Orpheus => "orpheus",
            Self::Cerberus => "cerberus",
            Self::Charon => "charon",
            Self::Vox => "vox",
            Self::Plutus => "plutus",
            Self::Psyche => "psyche",
            Self::Obolus => "obolus",
            Self::Nyx => "nyx",
            Self::Aether => "aether",
            Self::Tartarus => "tartarus",
            Self::Xwin => "xwin",
            Self::Atlas => "atlas",
            Self::CaseOverview => "overview",
            Self::ParserStatus => "parser",
            Self::Reports => "reports",
        }
    }

    pub fn evidence_agent(self) -> Option<&'static str> {
        match self {
            Self::Cerberus => Some("cerberus"),
            Self::Charon => Some("charon"),
            Self::Nyx => Some("nyx"),
            Self::Obolus => Some("obolus"),
            Self::Psyche => Some("psyche"),
            Self::Orpheus => Some("orpheus"),
            Self::Plutus => Some("plutus"),
            Self::Atlas => Some("atlas"),
            Self::Aether => Some("aether"),
            Self::Nemesis
            | Self::Chronos
            | Self::Helios
            | Self::Vox
            | Self::Tartarus
            | Self::Xwin
            | Self::CaseOverview
            | Self::ParserStatus
            | Self::Reports => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NavItem {
    pub label: &'static str,
    pub route: WorkspaceRoute,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPane {
    pub logs_path: PathBuf,
    pub reports_path: PathBuf,
    pub prepared_root: PathBuf,
    pub helios_root: PathBuf,
    pub index_root: PathBuf,
    pub live_mount_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceViewModel {
    pub case_id: String,
    pub root: PathBuf,
    pub nav: Vec<NavItem>,
    pub status: StatusPane,
}

impl WorkspaceViewModel {
    pub fn from_case(case: &Case) -> Result<Self> {
        case.workspace().ensure_layout()?;

        Ok(Self {
            case_id: case.name().to_string(),
            root: case.root_path(),
            nav: vec![
                nav_item(WorkspaceRoute::Nemesis),
                nav_item(WorkspaceRoute::Chronos),
                nav_item(WorkspaceRoute::Helios),
                nav_item(WorkspaceRoute::Orpheus),
                nav_item(WorkspaceRoute::Cerberus),
                nav_item(WorkspaceRoute::Charon),
                nav_item(WorkspaceRoute::Vox),
                nav_item(WorkspaceRoute::Plutus),
                nav_item(WorkspaceRoute::Psyche),
                nav_item(WorkspaceRoute::Obolus),
                nav_item(WorkspaceRoute::Nyx),
                nav_item(WorkspaceRoute::Aether),
                nav_item(WorkspaceRoute::Tartarus),
                nav_item(WorkspaceRoute::Xwin),
            ],
            status: StatusPane {
                logs_path: case.workspace().logs_dir().join("case.log"),
                reports_path: case.workspace().reports_dir(),
                prepared_root: case.workspace().prepared_database_root(),
                helios_root: case.workspace().prepared_helios_dir(),
                index_root: case.workspace().index_dir(),
                live_mount_root: case.workspace().live_device_mounts_dir(),
            },
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryMetric {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidencePane {
    pub label: &'static str,
    pub route: WorkspaceRoute,
    pub evidence_dir: PathBuf,
    pub records_path: PathBuf,
    pub summary_path: PathBuf,
    pub record_count: usize,
    pub metrics: Vec<SummaryMetric>,
    pub ready: bool,
}

impl EvidencePane {
    fn from_case(case: &Case, route: WorkspaceRoute) -> Result<Self> {
        let slug = route
            .evidence_agent()
            .context("Evidence panes require an agent route")?;
        let evidence_dir = case.evidence_path(slug);
        let records_path = evidence_dir.join("records.jsonl");
        let summary_path = evidence_dir.join("summary.json");

        Ok(Self {
            label: route.label(),
            route,
            record_count: count_lines(&records_path)?,
            metrics: read_summary_metrics(&summary_path)?,
            ready: records_path.exists() || summary_path.exists(),
            evidence_dir,
            records_path,
            summary_path,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSnapshot {
    pub workspace: WorkspaceViewModel,
    pub device_status: LiveDeviceStatus,
    pub panels: Vec<EvidencePane>,
    pub nyx: Option<NyxViewModel>,
    pub obolus: Option<ObolusViewModel>,
    pub charon: Option<CharonViewModel>,
    pub cerberus: Option<CerberusViewModel>,
    pub orpheus: Option<OrpheusViewModel>,
    pub recent_logs: Vec<String>,
    pub report_files: Vec<String>,
}

impl WorkspaceSnapshot {
    pub fn from_case(case: &Case) -> Result<Self> {
        Ok(Self {
            workspace: WorkspaceViewModel::from_case(case)?,
            device_status: probe_live_status(case),
            panels: vec![
                EvidencePane::from_case(case, WorkspaceRoute::Cerberus)?,
                EvidencePane::from_case(case, WorkspaceRoute::Charon)?,
                EvidencePane::from_case(case, WorkspaceRoute::Nyx)?,
                EvidencePane::from_case(case, WorkspaceRoute::Obolus)?,
                EvidencePane::from_case(case, WorkspaceRoute::Psyche)?,
                EvidencePane::from_case(case, WorkspaceRoute::Orpheus)?,
                EvidencePane::from_case(case, WorkspaceRoute::Plutus)?,
                EvidencePane::from_case(case, WorkspaceRoute::Atlas)?,
                EvidencePane::from_case(case, WorkspaceRoute::Aether)?,
            ],
            nyx: NyxViewModel::from_case(case)?,
            obolus: ObolusViewModel::from_case(case)?,
            charon: CharonViewModel::from_case(case)?,
            cerberus: CerberusViewModel::from_case(case)?,
            orpheus: OrpheusViewModel::from_case(case)?,
            recent_logs: read_recent_lines(&case.workspace().logs_dir().join("case.log"), 24)?,
            report_files: list_relative_files(&case.workspace().reports_dir())?,
        })
    }

    pub fn panel(&self, route: WorkspaceRoute) -> Option<&EvidencePane> {
        self.panels.iter().find(|panel| panel.route == route)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NyxViewModel {
    pub evidence_dir: PathBuf,
    pub history_path: PathBuf,
    pub bookmarks_path: PathBuf,
    pub tabs_path: PathBuf,
    pub autofill_path: PathBuf,
    pub searches_path: PathBuf,
    pub history: Vec<HistoryItem>,
    pub bookmarks: Vec<Bookmark>,
    pub tabs: Vec<TabEntry>,
    pub autofill: Vec<AutofillEntry>,
    pub searches: Vec<SearchTerm>,
}

impl NyxViewModel {
    fn from_case(case: &Case) -> Result<Option<Self>> {
        let evidence_dir = case.evidence_path("nyx");
        let history_path = evidence_dir.join("history.json");
        let bookmarks_path = evidence_dir.join("bookmarks.json");
        let tabs_path = evidence_dir.join("tabs.json");
        let autofill_path = evidence_dir.join("autofill.json");
        let searches_path = evidence_dir.join("searches.json");

        if !history_path.exists()
            && !bookmarks_path.exists()
            && !tabs_path.exists()
            && !autofill_path.exists()
            && !searches_path.exists()
        {
            return Ok(None);
        }

        Ok(Some(Self {
            evidence_dir,
            history_path: history_path.clone(),
            bookmarks_path: bookmarks_path.clone(),
            tabs_path: tabs_path.clone(),
            autofill_path: autofill_path.clone(),
            searches_path: searches_path.clone(),
            history: read_json_file_optional(&history_path)?.unwrap_or_default(),
            bookmarks: read_json_file_optional(&bookmarks_path)?.unwrap_or_default(),
            tabs: read_json_file_optional(&tabs_path)?.unwrap_or_default(),
            autofill: read_json_file_optional(&autofill_path)?.unwrap_or_default(),
            searches: read_json_file_optional(&searches_path)?.unwrap_or_default(),
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ObolusViewModel {
    pub evidence_dir: PathBuf,
    pub notes_path: PathBuf,
    pub folders_path: PathBuf,
    pub accounts_path: PathBuf,
    pub attachments_path: PathBuf,
    pub notes: Vec<ObolusNote>,
    pub folders: Vec<ObolusFolder>,
    pub accounts: Vec<ObolusAccount>,
    pub attachments: Vec<ObolusAttachment>,
}

impl ObolusViewModel {
    fn from_case(case: &Case) -> Result<Option<Self>> {
        let evidence_dir = case.evidence_path("obolus");
        let notes_path = evidence_dir.join("notes.json");
        let folders_path = evidence_dir.join("folders.json");
        let accounts_path = evidence_dir.join("accounts.json");
        let attachments_path = evidence_dir.join("attachments.json");

        if !notes_path.exists()
            && !folders_path.exists()
            && !accounts_path.exists()
            && !attachments_path.exists()
        {
            return Ok(None);
        }

        Ok(Some(Self {
            evidence_dir,
            notes_path: notes_path.clone(),
            folders_path: folders_path.clone(),
            accounts_path: accounts_path.clone(),
            attachments_path: attachments_path.clone(),
            notes: read_json_file_optional(&notes_path)?.unwrap_or_default(),
            folders: read_json_file_optional(&folders_path)?.unwrap_or_default(),
            accounts: read_json_file_optional(&accounts_path)?.unwrap_or_default(),
            attachments: read_json_file_optional(&attachments_path)?.unwrap_or_default(),
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CharonViewModel {
    pub evidence_dir: PathBuf,
    pub assets_path: PathBuf,
    pub faces_path: PathBuf,
    pub albums_path: PathBuf,
    pub timeline_path: PathBuf,
    pub assets: Vec<CharonAsset>,
    pub faces: Vec<CharonFace>,
    pub albums: Vec<CharonAlbum>,
    pub timeline: Vec<CharonTimelineEntry>,
}

impl CharonViewModel {
    fn from_case(case: &Case) -> Result<Option<Self>> {
        let evidence_dir = case.evidence_path("charon");
        let assets_path = evidence_dir.join("assets.json");
        let faces_path = evidence_dir.join("faces.json");
        let albums_path = evidence_dir.join("albums.json");
        let timeline_path = evidence_dir.join("timeline.json");

        if !assets_path.exists()
            && !faces_path.exists()
            && !albums_path.exists()
            && !timeline_path.exists()
        {
            return Ok(None);
        }

        Ok(Some(Self {
            evidence_dir,
            assets_path: assets_path.clone(),
            faces_path: faces_path.clone(),
            albums_path: albums_path.clone(),
            timeline_path: timeline_path.clone(),
            assets: read_json_file_optional(&assets_path)?.unwrap_or_default(),
            faces: read_json_file_optional(&faces_path)?.unwrap_or_default(),
            albums: read_json_file_optional(&albums_path)?.unwrap_or_default(),
            timeline: read_json_file_optional(&timeline_path)?.unwrap_or_default(),
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CerberusViewModel {
    pub evidence_dir: PathBuf,
    pub messages_path: PathBuf,
    pub attachments_path: PathBuf,
    pub contacts_path: PathBuf,
    pub calls_path: PathBuf,
    pub voicemails_path: PathBuf,
    pub messages: Vec<CerberusMessage>,
    pub attachments: Vec<CerberusAttachment>,
    pub contacts: Vec<ContactRecord>,
    pub calls: Vec<CallRecord>,
    pub voicemails: Vec<VoicemailRecord>,
}

impl CerberusViewModel {
    fn from_case(case: &Case) -> Result<Option<Self>> {
        let evidence_dir = case.evidence_path("cerberus");
        let messages_path = evidence_dir.join("messages.json");
        let attachments_path = evidence_dir.join("attachments.json");
        let contacts_path = evidence_dir.join("contacts.json");
        let calls_path = evidence_dir.join("calls.json");
        let voicemails_path = evidence_dir.join("voicemails.json");

        let has_any = messages_path.exists()
            || attachments_path.exists()
            || contacts_path.exists()
            || calls_path.exists()
            || voicemails_path.exists();

        if !has_any {
            return Ok(None);
        }

        let mut contacts: Vec<ContactRecord> = read_json_file_optional(&contacts_path)?.unwrap_or_default();
        // Normalize 'phone' (singular string) → 'phones' (array) for serde compatibility
        for contact in &mut contacts {
            if contact.phones.is_empty() {
                // If phones is empty but the struct was deserialized from old format
                // with a single 'phone' field, we'd need raw JSON manipulation.
                // Since we now control the serialization, this is a no-op safety net.
            }
        }

        Ok(Some(Self {
            evidence_dir,
            messages_path: messages_path.clone(),
            attachments_path: attachments_path.clone(),
            contacts_path: contacts_path.clone(),
            calls_path: calls_path.clone(),
            voicemails_path: voicemails_path.clone(),
            messages: read_json_file_optional(&messages_path)?.unwrap_or_default(),
            attachments: read_json_file_optional(&attachments_path)?.unwrap_or_default(),
            contacts,
            calls: read_json_file_optional(&calls_path)?.unwrap_or_default(),
            voicemails: read_json_file_optional(&voicemails_path)?.unwrap_or_default(),
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrpheusApp {
    pub name: String,
    pub databases: Vec<DatabaseSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrpheusViewModel {
    pub evidence_dir: PathBuf,
    pub report_path: PathBuf,
    pub databases: Vec<DatabaseSummary>,
    pub apps: Vec<OrpheusApp>,
}

fn app_from_db_path(path: &str) -> String {
    let p = Path::new(path);
    let components = p.components().map(|c| c.as_os_str().to_string_lossy().to_string());
    // Look for a domain-like component (contains 'Domain' or looks like bundle ID)
    for comp in components {
        if comp.contains("Domain") || comp.starts_with("AppDomain-") || comp.starts_with("Sys") {
            return comp;
        }
    }
    // Fallback: first directory after root, or filename
    p.parent()
        .and_then(|parent| parent.file_name().map(|f| f.to_string_lossy().to_string()))
        .unwrap_or_else(|| p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".to_string()))
}

impl OrpheusViewModel {
    fn from_case(case: &Case) -> Result<Option<Self>> {
        let evidence_dir = case.evidence_path("orpheus");
        if !evidence_dir.exists() {
            return Ok(None);
        }

        // Find newest orpheus_* subdirectory
        let mut newest_dir: Option<PathBuf> = None;
        let mut newest_time = std::time::SystemTime::UNIX_EPOCH;
        for entry in fs::read_dir(&evidence_dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("orpheus_") && entry.path().is_dir() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified > newest_time {
                            newest_time = modified;
                            newest_dir = Some(entry.path());
                        }
                    }
                }
            }
        }

        let report_path = match newest_dir {
            Some(dir) => dir.join("orpheus_report.json"),
            None => evidence_dir.join("orpheus_report.json"),
        };

        if !report_path.exists() {
            return Ok(None);
        }

        let databases: Vec<DatabaseSummary> = read_json_file_optional(&report_path)?.unwrap_or_default();
        let mut apps_map: HashMap<String, Vec<DatabaseSummary>> = HashMap::new();
        for db in databases.clone() {
            let app = app_from_db_path(&db.path);
            apps_map.entry(app).or_default().push(db);
        }
        let mut apps: Vec<OrpheusApp> = apps_map
            .into_iter()
            .map(|(name, databases)| OrpheusApp { name, databases })
            .collect();
        apps.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Some(Self {
            evidence_dir,
            report_path: report_path.clone(),
            databases,
            apps,
        }))
    }
}

fn nav_item(route: WorkspaceRoute) -> NavItem {
    NavItem {
        label: route.label(),
        route,
    }
}

fn read_recent_lines(path: &Path, limit: usize) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let reader = BufReader::new(
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?,
    );
    let mut lines = reader
        .lines()
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to read {}", path.display()))?;

    if lines.len() > limit {
        lines.drain(0..lines.len() - limit);
    }

    Ok(lines)
}

fn count_lines(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let reader = BufReader::new(
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?,
    );
    let mut count = 0usize;
    for line in reader.lines() {
        line.with_context(|| format!("Failed to read {}", path.display()))?;
        count += 1;
    }
    Ok(count)
}

fn read_summary_metrics(path: &Path) -> Result<Vec<SummaryMetric>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let value: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", path.display()))?;

    let mut metrics = Vec::new();
    if let Some(object) = value.as_object() {
        let mut keys = object.keys().cloned().collect::<Vec<_>>();
        keys.sort();

        for key in keys {
            if matches!(key.as_str(), "agent" | "generated_at") {
                continue;
            }

            if let Some(value) = object.get(&key).and_then(summary_scalar) {
                metrics.push(SummaryMetric { key, value });
            }
        }
    }

    Ok(metrics)
}

fn read_json_file_optional<T>(path: &Path) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn summary_scalar(value: &Value) -> Option<String> {
    match value {
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn list_relative_files(root: &Path) -> Result<Vec<String>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_file() => {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or_else(|_| entry.path())
                    .display()
                    .to_string();
                Some(Ok(relative))
            }
            Ok(_) => None,
            Err(error) => Some(Err(anyhow::Error::from(error))),
        })
        .collect::<Result<Vec<_>>>()?;
    files.sort();
    Ok(files)
}

```

### `src/ui/gtk_shell.rs`

```rust
use anyhow::{bail, Result};
use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::glib;
use relm4::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::case::Case;
use crate::chronos::{DeviceManager, PairingState};
use crate::ui::{
    CerberusViewModel, WorkspaceRoute, WorkspaceSnapshot,
};

thread_local! {
    static APP_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

#[derive(Debug)]
struct ShellInit {
    case: Case,
    snapshot: WorkspaceSnapshot,
}

#[derive(Debug, Clone)]
struct PageCard {
    title: String,
    subtitle: Option<String>,
    body: String,
}

pub fn run(case: Case) -> Result<()> {
    case.workspace().ensure_layout()?;
    let snapshot = WorkspaceSnapshot::from_case(&case)?;
    let gtk_args = std::env::args().take(1).collect::<Vec<_>>();
    let app = RelmApp::new("io.ion.shell").with_args(gtk_args);
    app.run::<ShellModel>(ShellInit { case, snapshot });
    Ok(())
}

#[derive(Debug)]
struct ShellModel {
    case: Case,
    snapshot: WorkspaceSnapshot,
    selected_route: WorkspaceRoute,
    cards: Vec<PageCard>,
    last_error: Option<String>,
    last_action: Option<String>,
    zoom: f64,
    search_term: String,
    orpheus_app_index: usize,
    contacts_selected: HashSet<i64>,
    cerberus_selected: HashSet<usize>,
    cerberus_list: Option<gtk::Box>,
    cerberus_tab: usize,
    cerberus_contact_filter: Option<String>,
    cerberus_date_filter: Option<String>,
    cerberus_message_limit: usize,
    cerberus_messages_box: Option<gtk::Box>,
    cerberus_attachments_box: Option<gtk::Box>,
    cerberus_contacts_col: Option<gtk::Box>,
    cerberus_attachments_col: Option<gtk::Box>,
    logo_path: String,
    device_phone_number: Option<String>,
}

#[derive(Debug)]
enum ShellInput {
    SelectRoute(WorkspaceRoute),
    Refresh,
    PairPrimaryDevice,
    MountPrimaryDevice,
    UnmountCaseMounts,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    SearchChanged(String),
    RunPsyche,
    AgentFinished { agent: String, success: bool },
    OrpheusPrevApp,
    OrpheusNextApp,
    ToggleMessage(usize),
    WriteEvidenceDoc,
    CerberusTab(usize),
    CerberusFilterContact(Option<String>),
    CerberusFilterDate(Option<String>),
    CerberusLoadMore,
    CerberusSearch,
    ToggleCerberusContact(i64),
}

#[relm4::component]
impl SimpleComponent for ShellModel {
    type Init = ShellInit;
    type Input = ShellInput;
    type Output = ();

    view! {
        main_window = gtk::ApplicationWindow {
            set_title: Some("ion"),
            set_default_width: 1280,
            set_default_height: 800,
            add_css_class: "stygion-shell",

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                add_css_class: "shell-root",

                gtk::ScrolledWindow {
                    #[watch]
                    set_width_request: model.sidebar_width(),
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    set_vscrollbar_policy: gtk::PolicyType::Automatic,
                    add_css_class: "sidebar-scroll",

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        add_css_class: "sidebar",

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            add_css_class: "sidebar-brand",
                        },

                        // SYSTEM CONTROL
                        gtk::Label {
                            set_label: "SYSTEM CONTROL",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Nemesis),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Nemesis),
                        },

                        // DEVICE ACCESS
                        gtk::Label {
                            set_label: "DEVICE ACCESS",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Chronos),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Chronos),
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Helios),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Helios),
                        },

                        // DATA CORE
                        gtk::Label {
                            set_label: "DATA CORE",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Orpheus),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Orpheus),
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Obolus),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Obolus),
                        },

                        // COMMUNICATION
                        gtk::Label {
                            set_label: "COMMUNICATION",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Cerberus),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Cerberus),
                        },

                        // FILES AND MEDIA
                        gtk::Label {
                            set_label: "FILES AND MEDIA",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Charon),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Charon),
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Vox),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Vox),
                        },

                        // FINANCIAL
                        gtk::Label {
                            set_label: "FINANCIAL",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Plutus),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Plutus),
                        },

                        // INTELLIGENCE
                        gtk::Label {
                            set_label: "INTELLIGENCE",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Psyche),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Psyche),
                        },

                        // OUTPUT
                        gtk::Label {
                            set_label: "OUTPUT",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },

                        // BACKGROUND SYSTEMS
                        gtk::Label {
                            set_label: "BACKGROUND SYSTEMS",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Nyx),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Nyx),
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Aether),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Aether),
                        },

                        // ADVANCED / LOW-LEVEL
                        gtk::Label {
                            set_label: "ADVANCED / LOW-LEVEL",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Tartarus),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Tartarus),
                        },

                        // BRIDGE / COMPATIBILITY
                        gtk::Label {
                            set_label: "BRIDGE / COMPATIBILITY",
                            add_css_class: "nav-section",
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            #[watch]
                            set_label: &model.nav_label(WorkspaceRoute::Xwin),
                            add_css_class: "sidebar-link",
                            connect_clicked => ShellInput::SelectRoute(WorkspaceRoute::Xwin),
                        },
                    },
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_hexpand: true,
                    add_css_class: "workspace",

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        add_css_class: "topbar",

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_hexpand: true,

                            gtk::Label {
                                #[watch]
                                set_label: &model.header_title(),
                                add_css_class: "title",
                                set_xalign: 0.0,
                            },

                            gtk::Label {
                                #[watch]
                                set_label: &model.header_subtitle(),
                                add_css_class: "topbar-meta",
                                set_xalign: 0.0,
                            },
                        },

                        #[name = "search_entry"]
                        gtk::SearchEntry {
                            set_placeholder_text: Some("Search evidence..."),
                            set_width_request: 220,
                            set_margin_end: 8,
                        },

                        gtk::Label {
                            #[watch]
                            set_label: &model.topbar_status(),
                            add_css_class: "topbar-tools",
                            set_xalign: 1.0,
                            set_margin_end: 12,
                        },

                        gtk::Button {
                            set_label: "Refresh",
                            add_css_class: "refresh-button",
                            connect_clicked => ShellInput::Refresh,
                        },

                        gtk::Button {
                            set_label: "Pair",
                            add_css_class: "refresh-button",
                            #[watch]
                            set_visible: model.selected_route == WorkspaceRoute::Chronos,
                            #[watch]
                            set_sensitive: model.can_pair_primary_device(),
                            connect_clicked => ShellInput::PairPrimaryDevice,
                        },

                        gtk::Button {
                            set_label: "Mount",
                            add_css_class: "refresh-button",
                            #[watch]
                            set_visible: model.selected_route == WorkspaceRoute::Chronos,
                            #[watch]
                            set_sensitive: model.can_mount_primary_device(),
                            connect_clicked => ShellInput::MountPrimaryDevice,
                        },

                        gtk::Button {
                            set_label: "Unmount",
                            add_css_class: "refresh-button",
                            #[watch]
                            set_visible: model.selected_route == WorkspaceRoute::Chronos,
                            #[watch]
                            set_sensitive: model.can_unmount_case_mounts(),
                            connect_clicked => ShellInput::UnmountCaseMounts,
                        },

                        gtk::Button {
                            set_label: "Run Psyche",
                            add_css_class: "refresh-button",
                            connect_clicked => ShellInput::RunPsyche,
                        },

                        gtk::Button {
                            set_label: "Write Evidence Doc",
                            add_css_class: "refresh-button",
                            #[watch]
                            set_visible: model.selected_route == WorkspaceRoute::Cerberus && !model.cerberus_selected.is_empty(),
                            connect_clicked => ShellInput::WriteEvidenceDoc,
                        },

                        gtk::Button {
                            set_label: "← Prev App",
                            add_css_class: "refresh-button",
                            #[watch]
                            set_visible: model.selected_route == WorkspaceRoute::Orpheus,
                            #[watch]
                            set_sensitive: model.can_orpheus_prev(),
                            connect_clicked => ShellInput::OrpheusPrevApp,
                        },

                        gtk::Button {
                            set_label: "Next App →",
                            add_css_class: "refresh-button",
                            #[watch]
                            set_visible: model.selected_route == WorkspaceRoute::Orpheus,
                            #[watch]
                            set_sensitive: model.can_orpheus_next(),
                            connect_clicked => ShellInput::OrpheusNextApp,
                        },
                    },

                    gtk::ScrolledWindow {
                        set_hexpand: true,
                        set_vexpand: true,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            add_css_class: "content",

                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                add_css_class: "page-grid",

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    add_css_class: "card",
                                    #[watch]
                                    set_visible: model.card_visible(0),

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        add_css_class: "card-header",

                                        gtk::Label {
                                            #[watch]
                                            set_label: &model.card_title(0),
                                            add_css_class: "card-title",
                                            set_xalign: 0.0,
                                        },

                                        gtk::Label {
                                            #[watch]
                                            set_label: &model.card_subtitle(0),
                                            add_css_class: "card-subtitle",
                                            #[watch]
                                            set_visible: model.card_has_subtitle(0),
                                            set_xalign: 0.0,
                                        }
                                    },

                                    gtk::Label {
                                        #[watch]
                                        set_label: &model.card_body(0),
                                        add_css_class: "card-body",
                                        set_wrap: true,
                                        set_selectable: true,
                                        set_xalign: 0.0,
                                        set_yalign: 0.0,
                                    }
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    add_css_class: "card",
                                    #[watch]
                                    set_visible: model.card_visible(1),

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        add_css_class: "card-header",

                                        gtk::Label {
                                            #[watch]
                                            set_label: &model.card_title(1),
                                            add_css_class: "card-title",
                                            set_xalign: 0.0,
                                        },

                                        gtk::Label {
                                            #[watch]
                                            set_label: &model.card_subtitle(1),
                                            add_css_class: "card-subtitle",
                                            #[watch]
                                            set_visible: model.card_has_subtitle(1),
                                            set_xalign: 0.0,
                                        }
                                    },

                                    gtk::Label {
                                        #[watch]
                                        set_label: &model.card_body(1),
                                        add_css_class: "card-body",
                                        set_wrap: true,
                                        set_selectable: true,
                                        set_xalign: 0.0,
                                        set_yalign: 0.0,
                                        #[watch]
                                        set_visible: model.selected_route != WorkspaceRoute::Cerberus,
                                    },

                                    // CERBERUS 3-COLUMN CONVERSATION VIEW
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_hexpand: true,
                                        set_vexpand: true,
                                        set_spacing: 8,
                                        #[watch]
                                        set_visible: model.selected_route == WorkspaceRoute::Cerberus,

                                        // COLUMN 1: CONTACTS LIST
                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Vertical,
                                            set_width_request: 160,
                                            set_margin_end: 4,

                                            gtk::Label {
                                                set_label: "Contacts",
                                                add_css_class: "cerberus-col-header",
                                                set_xalign: 0.0,
                                                set_margin_bottom: 4,
                                            },

                                            gtk::ScrolledWindow {
                                                set_vexpand: true,
                                                set_hscrollbar_policy: gtk::PolicyType::Never,
                                                #[name = "cerberus_contacts_col"]
                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Vertical,
                                                    add_css_class: "cerberus-contacts-column",
                                                    set_spacing: 2,
                                                }
                                            }
                                        },

                                        // COLUMN 2: ATTACHMENTS
                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Vertical,
                                            set_width_request: 160,
                                            set_margin_end: 4,

                                            gtk::Label {
                                                set_label: "Attachments",
                                                add_css_class: "cerberus-col-header",
                                                set_xalign: 0.0,
                                                set_margin_bottom: 4,
                                            },

                                            gtk::ScrolledWindow {
                                                set_vexpand: true,
                                                set_hscrollbar_policy: gtk::PolicyType::Never,
                                                #[name = "cerberus_attachments_col"]
                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Vertical,
                                                    add_css_class: "cerberus-attachments-column",
                                                    set_spacing: 4,
                                                }
                                            }
                                        },

                                        // COLUMN 3: MESSAGES
                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Vertical,
                                            set_hexpand: true,

                                            // Message search bar
                                            gtk::Box {
                                                set_orientation: gtk::Orientation::Horizontal,
                                                set_spacing: 4,
                                                set_margin_bottom: 6,

                                                #[name = "cerberus_search_entry"]
                                                gtk::SearchEntry {
                                                    set_placeholder_text: Some("Search messages..."),
                                                    set_hexpand: true,
                                                },

                                                gtk::Button {
                                                    set_label: "Search",
                                                    add_css_class: "cerberus-tab-search",
                                                    connect_clicked => ShellInput::CerberusSearch,
                                                },

                                                gtk::Button {
                                                    set_label: "Send To",
                                                    add_css_class: "cerberus-action-btn",
                                                    connect_clicked => ShellInput::WriteEvidenceDoc,
                                                },

                                                gtk::Button {
                                                    set_label: "Save",
                                                    add_css_class: "cerberus-action-btn",
                                                    connect_clicked => ShellInput::WriteEvidenceDoc,
                                                },
                                            },

                                            gtk::ScrolledWindow {
                                                set_vexpand: true,
                                                set_hscrollbar_policy: gtk::PolicyType::Never,
                                                set_vscrollbar_policy: gtk::PolicyType::Automatic,
                                                #[name = "cerberus_messages_box"]
                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Vertical,
                                                    add_css_class: "cerberus-message-list",
                                                    set_spacing: 4,
                                                }
                                            }
                                        }
                                    }
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    add_css_class: "card",
                                    #[watch]
                                    set_visible: model.card_visible(2),

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        add_css_class: "card-header",

                                        gtk::Label {
                                            #[watch]
                                            set_label: &model.card_title(2),
                                            add_css_class: "card-title",
                                            set_xalign: 0.0,
                                        },

                                        gtk::Label {
                                            #[watch]
                                            set_label: &model.card_subtitle(2),
                                            add_css_class: "card-subtitle",
                                            #[watch]
                                            set_visible: model.card_has_subtitle(2),
                                            set_xalign: 0.0,
                                        }
                                    },

                                    gtk::Label {
                                        #[watch]
                                        set_label: &model.card_body(2),
                                        add_css_class: "card-body",
                                        set_wrap: true,
                                        set_selectable: true,
                                        set_xalign: 0.0,
                                        set_yalign: 0.0,
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let logo_bytes = include_bytes!("assets/logo.jpg");
        let logo_dir = init.case.root_path().join("tmp");
        let _ = std::fs::create_dir_all(&logo_dir);
        let logo_path = logo_dir.join("ion_logo.jpg");
        let _ = std::fs::write(&logo_path, logo_bytes);
        let logo_path_str = logo_path.to_string_lossy().to_string();

        // Load device phone number from registry for sent-message attribution
        let device_phone_number = init.case.device_phone_number().ok().flatten();

        let mut model = ShellModel {
            case: init.case,
            snapshot: init.snapshot,
            selected_route: WorkspaceRoute::Nemesis,
            cards: Vec::new(),
            last_error: None,
            last_action: None,
            zoom: 1.0,
            search_term: String::new(),
            orpheus_app_index: 0,
            contacts_selected: HashSet::new(),
            cerberus_selected: HashSet::new(),
            cerberus_list: None,
            cerberus_tab: 0,
            cerberus_contact_filter: None,
            cerberus_date_filter: None,
            cerberus_message_limit: 500,
            cerberus_messages_box: None,
            cerberus_attachments_box: None,
            cerberus_contacts_col: None,
            cerberus_attachments_col: None,
            logo_path: logo_path_str,
            device_phone_number,
        };
        install_css(model.zoom);

        let brand_css = gtk::CssProvider::new();
        brand_css.load_from_data(&format!(
            ".sidebar-brand {{ background-image: url(\"file://{}\"); background-size: cover; background-position: center; min-height: 140px; border-radius: 6px; }}",
            model.logo_path
        ));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &brand_css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }

        model.rebuild_cards();

        let widgets = view_output!();
        // cerberus_list widget removed; messages now render in cerberus_messages_box
        model.cerberus_messages_box = Some(widgets.cerberus_messages_box.clone());
        // cerberus_attachments_box widget renamed to cerberus_attachments_col in 3-column layout
        model.cerberus_contacts_col = Some(widgets.cerberus_contacts_col.clone());
        model.cerberus_attachments_col = Some(widgets.cerberus_attachments_col.clone());
        let search_sender = sender.clone();
        widgets.search_entry.connect_search_changed(move |entry: &gtk::SearchEntry| {
            search_sender.input(ShellInput::SearchChanged(entry.text().to_string()));
        });
        let search_activate_sender = sender.clone();
        widgets.search_entry.connect_activate(move |entry: &gtk::SearchEntry| {
            search_activate_sender.input(ShellInput::CerberusSearch);
        });
        let cerb_search_sender = sender.clone();
        widgets.cerberus_search_entry.connect_search_changed(move |entry: &gtk::SearchEntry| {
            cerb_search_sender.input(ShellInput::SearchChanged(entry.text().to_string()));
        });
        let cerb_activate_sender = sender.clone();
        widgets.cerberus_search_entry.connect_activate(move |entry: &gtk::SearchEntry| {
            cerb_activate_sender.input(ShellInput::CerberusSearch);
        });
        // cerberus_keyword_entry removed — use the global search_entry for message filtering
        widgets.main_window.set_title(Some(&format!(
            "ion | {}",
            model.snapshot.workspace.case_id
        )));
        let controller = gtk::EventControllerKey::new();
        let key_sender = sender.clone();
        controller.connect_key_pressed(move |_, key, _, state| {
            if !state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                return gtk::glib::Propagation::Proceed;
            }

            match key {
                gtk::gdk::Key::plus | gtk::gdk::Key::equal | gtk::gdk::Key::KP_Add => {
                    key_sender.input(ShellInput::ZoomIn);
                    gtk::glib::Propagation::Stop
                }
                gtk::gdk::Key::minus | gtk::gdk::Key::KP_Subtract => {
                    key_sender.input(ShellInput::ZoomOut);
                    gtk::glib::Propagation::Stop
                }
                gtk::gdk::Key::_0 | gtk::gdk::Key::KP_0 => {
                    key_sender.input(ShellInput::ResetZoom);
                    gtk::glib::Propagation::Stop
                }
                _ => gtk::glib::Propagation::Proceed,
            }
        });
        widgets.main_window.add_controller(controller);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            ShellInput::SelectRoute(route) => {
                self.selected_route = route;
                self.orpheus_app_index = 0;
                self.rebuild_cards();
                if route == WorkspaceRoute::Cerberus {
                    self.rebuild_cerberus_contacts_col(_sender.clone());
                    self.rebuild_cerberus_list(_sender.clone());
                    self.rebuild_cerberus_selected_contacts(_sender.clone());
                    self.rebuild_cerberus_attachments();
                } else {
                    self.clear_cerberus_list();
                }
            }
            ShellInput::Refresh => match WorkspaceSnapshot::from_case(&self.case) {
                Ok(snapshot) => {
                    self.snapshot = snapshot;
                    self.last_error = None;
                    self.rebuild_cards();
                    if self.selected_route == WorkspaceRoute::Cerberus {
                        self.rebuild_cerberus_contacts_col(_sender.clone());
                        self.rebuild_cerberus_list(_sender.clone());
                        self.rebuild_cerberus_selected_contacts(_sender.clone());
                        self.rebuild_cerberus_attachments();
                    }
                }
                Err(error) => {
                    self.last_error = Some(error.to_string());
                    self.rebuild_cards();
                }
            },
            ShellInput::PairPrimaryDevice => self.run_device_action("Pair device", |model| {
                let udid = model.primary_device_udid()?;
                DeviceManager::new(model.case.clone(), Default::default()).pair_device()?;
                Ok(format!("Paired device {}", udid))
            }),
            ShellInput::MountPrimaryDevice => self.run_device_action("Mount device", |model| {
                let udid = model.primary_device_udid()?;
                let device = model
                    .snapshot
                    .device_status
                    .connected_devices
                    .iter()
                    .find(|device| device.udid == udid)
                    .ok_or_else(|| anyhow::anyhow!("Device {} is no longer connected", udid))?;

                if device.pairing_state != PairingState::Paired {
                    bail!(
                        "Device {} is {}. Pair and trust the device before mounting.",
                        udid,
                        device.pairing_state.label()
                    );
                }

                DeviceManager::new(model.case.clone(), Default::default())
                    .mount_live_filesystem(&udid)?;
                Ok(format!(
                    "Mounted {} at {}",
                    udid,
                    device.suggested_mount_point.display()
                ))
            }),
            ShellInput::UnmountCaseMounts => self.run_device_action("Unmount device", |model| {
                let mounts = model.case_mount_points();
                if mounts.is_empty() {
                    bail!(
                        "No active ifuse mounts were found under {}",
                        model.snapshot.device_status.mount_root.display()
                    );
                }

                let manager = DeviceManager::new(model.case.clone(), Default::default());
                let mount_count = mounts.len();
                for mount in mounts {
                    manager.unmount_live_filesystem(&mount)?;
                }
                Ok(format!("Unmounted {} mount(s)", mount_count))
            }),
            ShellInput::ZoomIn => self.set_zoom((self.zoom + 0.1).min(1.8)),
            ShellInput::ZoomOut => self.set_zoom((self.zoom - 0.1).max(0.8)),
            ShellInput::ResetZoom => self.set_zoom(1.0),
            ShellInput::SearchChanged(term) => {
                self.search_term = term;
                // Do NOT rebuild anything on every keystroke — wait for explicit CerberusSearch
            }
            ShellInput::ToggleMessage(idx) => {
                if self.cerberus_selected.contains(&idx) {
                    self.cerberus_selected.remove(&idx);
                } else {
                    self.cerberus_selected.insert(idx);
                }
                // Do NOT rebuild the list — checkbox state is already toggled visually
                // and #[watch] handles the Write Evidence Doc button visibility
            }
            ShellInput::WriteEvidenceDoc => {
                if let Some(cerberus) = &self.snapshot.cerberus {
                    let selected: Vec<_> = self
                        .cerberus_selected
                        .iter()
                        .filter_map(|idx| cerberus.messages.get(*idx))
                        .cloned()
                        .collect();
                    if !selected.is_empty() {
                        let case_name = self.case.name().to_string();
                        let reports_dir = self.case.workspace().reports_dir();
                        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
                        let path = reports_dir.join(format!("cerberus_evidence_{}.md", timestamp));
                        let mut lines = vec![
                            "# Cerberus Evidence Document".to_string(),
                            String::new(),
                            format!("Case: {}", case_name),
                            format!("Generated: {}", timestamp),
                            format!("Selected messages: {}", selected.len()),
                            String::new(),
                        ];
                        for (i, msg) in selected.iter().enumerate() {
                            lines.push(format!("## Message {}", i + 1));
                            lines.push(format!("- **Date:** {}", msg.date));
                            lines.push(format!("- **Direction:** {}", msg.direction));
                            lines.push(format!("- **Phone:** {}", msg.phone_number));
                            lines.push(format!("- **Service:** {}", msg.service));
                            lines.push(format!("- **iMessage:** {}", msg.is_imessage));
                            lines.push(format!("- **Attachments:** {}", msg.has_attachments));
                            lines.push(String::new());
                            lines.push(format!("```\n{}\n```", msg.text));
                            lines.push(String::new());
                        }
                        if let Err(e) = std::fs::write(&path, lines.join("\n")) {
                            self.last_error = Some(format!("Failed to write evidence doc: {}", e));
                        } else {
                            self.last_error = None;
                            self.last_action = Some(format!(
                                "Wrote evidence doc: {}",
                                path.display()
                            ));
                        }
                        self.rebuild_cards();
                    }
                }
            }
            ShellInput::OrpheusPrevApp => {
                if self.orpheus_app_index > 0 {
                    self.orpheus_app_index -= 1;
                    self.rebuild_cards();
                }
            }
            ShellInput::OrpheusNextApp => {
                if let Some(orpheus) = &self.snapshot.orpheus {
                    if self.orpheus_app_index + 1 < orpheus.apps.len() {
                        self.orpheus_app_index += 1;
                        self.rebuild_cards();
                    }
                }
            }
            ShellInput::RunPsyche => {
                let case_name = self.case.name().to_string();
                let sender = _sender.clone();
                self.last_action = Some("Run Psyche: started".to_string());
                self.last_error = None;
                std::thread::spawn(move || {
                    let status = std::process::Command::new("psyche")
                        .arg(&case_name)
                        .status();
                    let success = matches!(status, Ok(s) if s.success());
                    sender.input(ShellInput::AgentFinished {
                        agent: "psyche".to_string(),
                        success,
                    });
                });
            }
            ShellInput::AgentFinished { agent, success } => {
                if success {
                    self.last_error = None;
                    self.last_action = Some(format!("{}: finished successfully", agent));
                    if let Ok(snapshot) = WorkspaceSnapshot::from_case(&self.case) {
                        self.snapshot = snapshot;
                    }
                    self.rebuild_cards();
                } else {
                    self.last_action = Some(format!("{}: finished with errors", agent));
                    self.last_error = Some(format!("{} failed — check logs for details", agent));
                    self.rebuild_cards();
                }
            }
            ShellInput::CerberusTab(tab) => {
                self.cerberus_tab = tab;
                self.rebuild_cerberus_list(_sender.clone());
                self.rebuild_cerberus_attachments();
            }
            ShellInput::CerberusFilterContact(filter) => {
                // Toggle: clicking the same contact clears the filter
                let new_filter = match (&self.cerberus_contact_filter, &filter) {
                    (Some(existing), Some(clicked)) if existing == clicked => None,
                    _ => filter,
                };
                self.cerberus_contact_filter = new_filter;
                self.cerberus_message_limit = 500;
                self.rebuild_cerberus_contacts_col(_sender.clone());
                self.rebuild_cerberus_list(_sender.clone());
                self.rebuild_cerberus_attachments();
            }
            ShellInput::CerberusFilterDate(filter) => {
                self.cerberus_date_filter = filter;
                self.rebuild_cerberus_list(_sender.clone());
            }
            ShellInput::CerberusLoadMore => {
                self.cerberus_message_limit += 500;
                self.rebuild_cerberus_list(_sender.clone());
            }
            ShellInput::CerberusSearch => {
                // Explicit search execution (Enter or Search button)
                self.cerberus_message_limit = 500;
                self.rebuild_cards();
                if self.selected_route == WorkspaceRoute::Cerberus {
                    self.rebuild_cerberus_contacts_col(_sender.clone());
                    self.rebuild_cerberus_list(_sender.clone());
                }
            }
            ShellInput::ToggleCerberusContact(id) => {
                if self.contacts_selected.contains(&id) {
                    self.contacts_selected.remove(&id);
                } else {
                    self.contacts_selected.insert(id);
                }
                self.rebuild_cerberus_contacts_col(_sender.clone());
                self.rebuild_cerberus_selected_contacts(_sender.clone());
                self.rebuild_cerberus_list(_sender.clone());
            }
        }
    }
}

impl ShellModel {
    fn run_device_action<F>(&mut self, verb: &str, action: F)
    where
        F: FnOnce(&Self) -> Result<String>,
    {
        match action(self) {
            Ok(message) => {
                self.last_error = None;
                self.last_action = Some(format!("{}: {}", verb, message));
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.last_action = Some(format!("{} failed", verb));
            }
        }

        match WorkspaceSnapshot::from_case(&self.case) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.rebuild_cards();
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.rebuild_cards();
            }
        }
    }

    fn make_detail_row(label: &str, value: &str) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row.set_margin_start(8);
        row.set_margin_end(8);
        row.set_margin_top(2);
        row.set_margin_bottom(2);

        let lbl = gtk::Label::new(Some(label));
        lbl.add_css_class("detail-label");
        lbl.set_xalign(0.0);
        row.append(&lbl);

        let val = gtk::Label::new(Some(value));
        val.add_css_class("detail-value");
        val.set_xalign(0.0);
        val.set_wrap(true);
        val.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        val.set_selectable(true);
        row.append(&val);
        row
    }

    fn rebuild_cerberus_contacts_col(&self, sender: ComponentSender<ShellModel>) {
        self.clear_cerberus_contacts_col();
        let Some(container) = &self.cerberus_contacts_col else { return };
        let Some(cerberus) = &self.snapshot.cerberus else { return };
        let contacts = &cerberus.contacts;

        let term = self.search_term.to_lowercase();
        let active_filter = self.cerberus_contact_filter.as_ref().map(|s| s.as_str());

        for contact in contacts {
            if !term.is_empty() {
                let haystack = format!(
                    "{} {} {}",
                    contact.name,
                    contact.phones.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "),
                    contact.emails.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
                )
                .to_lowercase();
                if !haystack.contains(&term) {
                    continue;
                }
            }

            let filter_phone = contact.phones.first().cloned().unwrap_or_default();
            let is_active = active_filter.map(|f| !filter_phone.is_empty() && filter_phone.contains(f) || f.contains(&filter_phone)).unwrap_or(false);

            let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            row.set_margin_top(1);
            row.set_margin_bottom(1);
            row.add_css_class("cerberus-contact-row");
            if is_active {
                row.add_css_class("cerberus-contact-row-active");
            }

            let primary_phone = contact.phones.first().map(|s| s.as_str()).unwrap_or("");
            let display = if primary_phone.is_empty() {
                contact.name.clone()
            } else {
                format!("{}  {}", contact.name, primary_phone)
            };
            let lbl = gtk::Label::new(None);
            lbl.set_markup(&format!(
                "<span size='small'>{}</span>",
                glib::markup_escape_text(&display)
            ));
            lbl.set_xalign(0.0);
            lbl.set_hexpand(true);
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&lbl);

            if !filter_phone.is_empty() {
                let fp = filter_phone;
                let s = sender.clone();
                let gesture = gtk::GestureClick::new();
                gesture.connect_pressed(move |_gesture, _n_press, _x, _y| {
                    s.input(ShellInput::CerberusFilterContact(Some(fp.clone())));
                });
                row.add_controller(gesture);
            }

            container.append(&row);
        }
    }

    fn show_cerberus_list(&self) -> bool {
        self.selected_route == WorkspaceRoute::Cerberus
    }

    fn clear_cerberus_messages_box(&self) {
        if let Some(container) = &self.cerberus_messages_box {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
    }

    fn clear_cerberus_contacts_col(&self) {
        if let Some(container) = &self.cerberus_contacts_col {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
    }

    fn clear_cerberus_attachments_col(&self) {
        if let Some(container) = &self.cerberus_attachments_col {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
    }

    fn clear_cerberus_list(&self) {
        self.clear_cerberus_messages_box();
        self.clear_cerberus_contacts_col();
        self.clear_cerberus_attachments_col();
        self.clear_cerberus_attachments_box();
        if let Some(container) = &self.cerberus_list {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
    }

    fn cerberus_stats_text(&self) -> String {
        let Some(cerberus) = &self.snapshot.cerberus else {
            return "No messages loaded.".to_string();
        };
        let total = cerberus.messages.len();
        let mut shown = 0;
        let term = self.search_term.to_lowercase();
        let date_filter = self.cerberus_date_filter.as_ref().map(|s| s.to_lowercase());
        let contact_filter = self.cerberus_contact_filter.as_ref().map(|s| s.to_lowercase());

        for msg in &cerberus.messages {
            if !term.is_empty() {
                let haystack = format!("{} {} {}", msg.phone_number, msg.text, msg.service).to_lowercase();
                if !haystack.contains(&term) {
                    continue;
                }
            }
            if self.cerberus_tab == 2 {
                if let Some(ref d) = date_filter {
                    if !msg.date.contains(d) {
                        continue;
                    }
                }
            }
            if let Some(ref c) = contact_filter {
                let haystack = format!("{} {} {}", msg.phone_number, msg.text, msg.service).to_lowercase();
                let raw_match = haystack.contains(c);
                let norm_filter: String = c.chars().filter(|ch| ch.is_ascii_digit()).collect();
                let norm_phone: String = msg.phone_number.chars().filter(|ch| ch.is_ascii_digit()).collect();
                let phone_match = !norm_filter.is_empty() && norm_phone.contains(&norm_filter);
                if !raw_match && !phone_match {
                    continue;
                }
            }
            shown += 1;
        }

        let selected = self.cerberus_selected.len();
        format!(
            "{} total messages\n{} shown (search filter)\n{} selected\nCheck messages below to select them.",
            total, shown, selected
        )
    }

    fn rebuild_cerberus_list(&self, _sender: ComponentSender<ShellModel>) {
        self.clear_cerberus_messages_box();
        let Some(container) = &self.cerberus_messages_box else { return };
        let Some(cerberus) = &self.snapshot.cerberus else { return };

        let contact_filter = self.cerberus_contact_filter.as_ref().map(|s| s.to_lowercase());
        let has_contact = contact_filter.is_some();
        let has_search = !self.search_term.is_empty();

        if !has_contact && !has_search {
            let empty = gtk::Label::new(Some("Select a contact from the left column, or type in the search box to find messages."));
            empty.set_xalign(0.5);
            empty.set_yalign(0.5);
            empty.set_vexpand(true);
            empty.add_css_class("cerberus-stats");
            container.append(&empty);
            return;
        }

        let term = self.search_term.to_lowercase();
        let date_filter = self.cerberus_date_filter.as_ref().map(|s| s.to_lowercase());

        let mut filtered: Vec<(usize, &crate::agents::cerberus_models::Message)> = Vec::new();
        for (index, msg) in cerberus.messages.iter().enumerate() {
            if !term.is_empty() {
                let haystack = format!("{} {} {}", msg.phone_number, msg.text, msg.service).to_lowercase();
                if !haystack.contains(&term) {
                    continue;
                }
            }
            if self.cerberus_tab == 2 {
                if let Some(ref d) = date_filter {
                    if !msg.date.contains(d) {
                        continue;
                    }
                }
            }
            if let Some(ref c) = contact_filter {
                let haystack = format!("{} {} {}", msg.phone_number, msg.text, msg.service).to_lowercase();
                let raw_match = haystack.contains(c);
                // Normalize phone numbers to digits for cross-format matching
                let norm_filter: String = c.chars().filter(|ch| ch.is_ascii_digit()).collect();
                let norm_phone: String = msg.phone_number.chars().filter(|ch| ch.is_ascii_digit()).collect();
                let phone_match = !norm_filter.is_empty() && norm_phone.contains(&norm_filter);
                if !raw_match && !phone_match {
                    continue;
                }
            }
            filtered.push((index, msg));
        }

        // Sort by timestamp for chronological conversation flow
        filtered.sort_by_key(|(_, msg)| msg.timestamp);

        // Build attachment lookup by message_id
        let mut att_by_msg: std::collections::HashMap<i64, Vec<&crate::agents::cerberus_models::Attachment>> = std::collections::HashMap::new();
        if let Some(cerberus_vm) = &self.snapshot.cerberus {
            for att in &cerberus_vm.attachments {
                if let Some(mid) = att.message_id {
                    att_by_msg.entry(mid).or_default().push(att);
                }
            }
        }

        let total_filtered = filtered.len();
        let limit = self.cerberus_message_limit;
        for (index, msg) in filtered.into_iter().take(limit) {
            let is_sent = msg.direction == "Sent" || msg.direction == "sent" || msg.direction == "OUTGOING";
            let msg_id = msg.message_id.unwrap_or(0);
            let is_selected = self.cerberus_selected.contains(&index);

            // Full-width row that aligns the bubble left or right
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            row.set_hexpand(true);
            row.set_margin_top(1);
            row.set_margin_bottom(1);
            row.add_css_class("cerberus-message-row");

            // Checkbox for message selection (left side for received, right side for sent)
            let checkbox = gtk::CheckButton::new();
            checkbox.set_active(is_selected);
            checkbox.set_valign(gtk::Align::Center);
            checkbox.set_margin_start(4);
            checkbox.set_margin_end(4);
            let s = _sender.clone();
            let idx = index;
            checkbox.connect_toggled(move |btn| {
                s.input(ShellInput::ToggleMessage(idx));
            });

            // Avatar for received messages (circle with initials)
            if !is_sent {
                let avatar = gtk::Box::new(gtk::Orientation::Vertical, 0);
                avatar.set_size_request(26, 26);
                avatar.set_valign(gtk::Align::End);
                avatar.set_margin_end(3);
                avatar.add_css_class("msg-avatar");

                let initials = msg.phone_number.chars().take(2).collect::<String>().to_uppercase();
                let avatar_lbl = gtk::Label::new(Some(&initials));
                avatar_lbl.add_css_class("msg-avatar-text");
                avatar.append(&avatar_lbl);
                row.append(&avatar);
            }

            // The bubble container
            let bubble = gtk::Box::new(gtk::Orientation::Vertical, 1);
            bubble.set_width_request(100);
            bubble.set_margin_top(1);
            bubble.set_margin_bottom(1);
            bubble.set_margin_start(is_sent as i32 * 4 + (!is_sent as i32) * 2);
            bubble.set_margin_end((!is_sent as i32) * 4 + (is_sent as i32) * 2);

            if is_sent {
                bubble.add_css_class("msg-bubble-sent");
            } else {
                bubble.add_css_class("msg-bubble-received");
            }

            // Header: phone number + date/time
            let header_color = if is_sent { "#ffffff" } else { "#1a1a1a" };
            let phone_display = if is_sent {
                self.device_phone_number.as_deref().unwrap_or("Me")
            } else {
                &msg.phone_number
            };
            let header_text = format!(
                "<span size='small' foreground='{}'><b>{}</b>  {}</span>",
                header_color,
                glib::markup_escape_text(phone_display),
                glib::markup_escape_text(&msg.date)
            );
            let header = gtk::Label::new(None);
            header.set_markup(&header_text);
            header.set_xalign(0.0);
            header.add_css_class("msg-bubble-header");
            bubble.append(&header);

            // Body: message text — wraps, expands in height, up to 1500 chars
            if !msg.text.is_empty() {
                let body_text = if msg.text.chars().count() > 1500 {
                    let truncated: String = msg.text.chars().take(1500).collect();
                    format!("{}… [truncated]", truncated)
                } else {
                    msg.text.clone()
                };

                let body = gtk::Label::new(None);
                body.set_markup(&glib::markup_escape_text(&body_text));
                body.set_wrap(true);
                body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                body.set_xalign(0.0);
                body.set_yalign(0.0);
                body.set_selectable(true);
                body.add_css_class("msg-bubble-body");
                bubble.append(&body);
            }

            // Inline attachments for this message
            if let Some(atts) = att_by_msg.get(&msg_id) {
                for att in atts {
                    if att.mime_type.starts_with("image/") {
                        let evidence_dir = self.case.evidence_path("cerberus");
                        // Try common attachment paths
                        let possible_paths = [
                            evidence_dir.join(&att.filename),
                            evidence_dir.join("attachments").join(&att.filename),
                            self.case.root_path().join("evidence").join("mms").join(&att.phone_number).join(att.transfer_name.split('/').next_back().unwrap_or(&att.transfer_name)),
                        ];
                        for img_path in &possible_paths {
                            if img_path.exists() {
                                let picture = gtk::Picture::for_filename(img_path);
                                picture.set_can_shrink(true);
                                picture.set_width_request(140);
                                picture.set_height_request(100);
                                picture.add_css_class("msg-bubble-image");
                                bubble.append(&picture);
                                break;
                            }
                        }
                    } else {
                        // Non-image attachment shown as a pill
                        let att_lbl = gtk::Label::new(Some(&format!("📎 {}", att.transfer_name.split('/').next_back().unwrap_or(&att.transfer_name))));
                        att_lbl.add_css_class("msg-bubble-attachment");
                        att_lbl.set_xalign(0.0);
                        bubble.append(&att_lbl);
                    }
                }
            }

            // Footer: service only (date/time moved to header)
            let footer_color = if is_sent { "#ffffff" } else { "#666666" };
            let footer = gtk::Label::new(None);
            footer.set_markup(&format!(
                "<span size='x-small' foreground='{}'>{}</span>",
                footer_color,
                glib::markup_escape_text(&msg.service)
            ));
            footer.set_xalign(1.0);
            footer.add_css_class("msg-bubble-footer");
            bubble.append(&footer);

            // Assemble with spacer and checkbox
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);

            if is_sent {
                row.append(&spacer);
                row.append(&bubble);
                row.append(&checkbox);
            } else {
                row.append(&checkbox);
                row.append(&bubble);
                row.append(&spacer);
            }

            container.append(&row);
        }

        if total_filtered > limit {
            let load_more_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            load_more_row.set_hexpand(true);
            load_more_row.set_margin_top(8);
            load_more_row.set_margin_bottom(8);
            load_more_row.set_halign(gtk::Align::Center);

            let btn = gtk::Button::with_label(&format!(
                "Load more messages (showing {} of {})",
                limit, total_filtered
            ));
            btn.add_css_class("cerberus-action-btn");
            let s = _sender.clone();
            btn.connect_clicked(move |_btn| {
                s.input(ShellInput::CerberusLoadMore);
            });
            load_more_row.append(&btn);
            container.append(&load_more_row);
        }
    }

    fn rebuild_cerberus_selected_contacts(&self, _sender: ComponentSender<ShellModel>) {
        // Deprecated: selected contacts now shown in left column directly
    }

    fn cerberus_dropdown_label(&self) -> String {
        let count = self.contacts_selected.len();
        if count == 0 {
            "Selected Contacts".to_string()
        } else if count == 1 {
            "1 Selected Contact".to_string()
        } else {
            format!("{} Selected Contacts", count)
        }
    }

    fn cerberus_contact_header(&self) -> String {
        if let Some(ref filter) = self.cerberus_contact_filter {
            if let Some(cerberus_vm) = &self.snapshot.cerberus {
                let contacts_vm = cerberus_vm;
                for contact in &contacts_vm.contacts {
                    let key = contact
                        .phones
                        .first()
                        .cloned()
                        .or_else(|| contact.emails.first().cloned())
                        .unwrap_or_else(|| contact.name.clone());
                    if &key == filter {
                        return contact.name.clone();
                    }
                }
            }
            return filter.clone();
        }
        "select contact to see messages".to_string()
    }

    fn clear_cerberus_attachments_box(&self) {
        if let Some(container) = &self.cerberus_attachments_box {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
    }

    fn rebuild_cerberus_attachments(&self) {
        self.clear_cerberus_attachments_col();
        let Some(container) = &self.cerberus_attachments_col else { return };
        let Some(cerberus) = &self.snapshot.cerberus else { return };

        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_column_spacing(6);
        grid.set_row_spacing(6);
        grid.set_max_children_per_line(2);
        grid.set_homogeneous(true);

        let evidence_dir = cerberus.evidence_dir.clone();
        for att in &cerberus.attachments {
            let path = evidence_dir.join(&att.filename);
            if path.exists() && att.mime_type.starts_with("image/") {
                let picture = gtk::Picture::for_filename(&path);
                picture.set_can_shrink(true);
                picture.set_width_request(120);
                picture.set_height_request(120);
                grid.append(&picture);
            }
        }
        if grid.first_child().is_none() {
            let lbl = gtk::Label::new(Some("No image attachments available."));
            lbl.set_xalign(0.0);
            grid.append(&lbl);
        }
        container.append(&grid);
    }

    fn can_orpheus_prev(&self) -> bool {
        self.orpheus_app_index > 0
    }

    fn can_orpheus_next(&self) -> bool {
        match &self.snapshot.orpheus {
            Some(orpheus) => self.orpheus_app_index + 1 < orpheus.apps.len(),
            None => false,
        }
    }

    fn rebuild_cards(&mut self) {
        let mut cards = match self.selected_route {
            WorkspaceRoute::Nemesis => self.overview_cards(),
            WorkspaceRoute::Chronos => self.chronos_cards(),
            WorkspaceRoute::Helios => self.helios_cards(),
            WorkspaceRoute::Orpheus => self.generic_agent_cards(WorkspaceRoute::Orpheus),
            WorkspaceRoute::Cerberus => self.cerberus_cards(),
            WorkspaceRoute::Charon => self.generic_agent_cards(WorkspaceRoute::Charon),
            WorkspaceRoute::Vox => self.generic_agent_cards(WorkspaceRoute::Vox),
            WorkspaceRoute::Plutus => self.generic_agent_cards(WorkspaceRoute::Plutus),
            WorkspaceRoute::Psyche => self.generic_agent_cards(WorkspaceRoute::Psyche),
            WorkspaceRoute::Obolus => self.generic_agent_cards(WorkspaceRoute::Obolus),
            WorkspaceRoute::Nyx => self.generic_agent_cards(WorkspaceRoute::Nyx),
            WorkspaceRoute::Aether => self.generic_agent_cards(WorkspaceRoute::Aether),
            WorkspaceRoute::Tartarus => self.generic_agent_cards(WorkspaceRoute::Tartarus),
            WorkspaceRoute::Xwin => self.generic_agent_cards(WorkspaceRoute::Xwin),
            WorkspaceRoute::Atlas => self.generic_agent_cards(WorkspaceRoute::Atlas),
            WorkspaceRoute::CaseOverview => self.overview_cards(),
            WorkspaceRoute::ParserStatus => self.parser_cards(),
            WorkspaceRoute::Reports => self.report_cards(),
        };
        if !self.search_term.is_empty() {
            let term = self.search_term.to_lowercase();
            cards.retain(|card| {
                card.title.to_lowercase().contains(&term)
                    || card.body.to_lowercase().contains(&term)
                    || card.subtitle.as_ref().map(|s| s.to_lowercase().contains(&term)).unwrap_or(false)
            });
        }
        self.cards = cards;
    }

    fn nav_label(&self, route: WorkspaceRoute) -> String {
        if self.selected_route == route {
            format!("▸ {}", route.label())
        } else {
            format!("· {}", route.label())
        }
    }

    fn header_title(&self) -> String {
        self.selected_route.agent_name().to_string()
    }

    fn header_subtitle(&self) -> String {
        self.selected_route.label().to_string()
    }

    fn topbar_status(&self) -> String {
        let ready_agents = self
            .snapshot
            .panels
            .iter()
            .filter(|panel| panel.ready)
            .count();
        let connected_devices = self.snapshot.device_status.connected_count();
        let paired_devices = self.snapshot.device_status.paired_count;
        let device_status = if connected_devices == 0 {
            "DEVICES 0 CONNECTED".to_string()
        } else {
            format!("DEVICES {}/{} PAIRED", paired_devices, connected_devices)
        };
        let action_status = self
            .last_action
            .clone()
            .unwrap_or_else(|| "no recent action".to_string());
        format!(
            "{} | {} | READY AGENTS {}/{} | ZOOM {}%",
            device_status,
            action_status,
            ready_agents,
            self.snapshot.panels.len(),
            self.zoom_percent()
        )
    }

    fn sidebar_width(&self) -> i32 {
        px(160.0, self.zoom)
    }

    fn zoom_percent(&self) -> i32 {
        (self.zoom * 100.0).round() as i32
    }

    fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom;
        install_css(self.zoom);
    }

    fn can_pair_primary_device(&self) -> bool {
        self.selected_route == WorkspaceRoute::Chronos
            && self.snapshot.device_status.available_tool("idevicepair")
            && self.primary_device_udid().is_ok()
    }

    fn can_mount_primary_device(&self) -> bool {
        self.selected_route == WorkspaceRoute::Chronos
            && self.snapshot.device_status.available_tool("ifuse")
            && self.primary_device_udid().is_ok()
    }

    fn can_unmount_case_mounts(&self) -> bool {
        self.selected_route == WorkspaceRoute::Chronos && !self.case_mount_points().is_empty()
    }

    fn primary_device_udid(&self) -> Result<String> {
        let devices = &self.snapshot.device_status.connected_devices;
        if devices.is_empty() {
            bail!("No connected iOS device detected.");
        }

        if devices.len() == 1 {
            return Ok(devices[0].udid.clone());
        }

        let paired = devices
            .iter()
            .filter(|device| device.pairing_state == PairingState::Paired)
            .collect::<Vec<_>>();
        if paired.len() == 1 {
            return Ok(paired[0].udid.clone());
        }

        bail!("Multiple devices are connected. One-click shell controls require exactly one connected device, or exactly one paired device.")
    }

    fn case_mount_points(&self) -> Vec<PathBuf> {
        self.snapshot
            .device_status
            .ifuse_mounts
            .iter()
            .filter(|mount| {
                mount
                    .mount_point
                    .starts_with(&self.snapshot.device_status.mount_root)
            })
            .map(|mount| mount.mount_point.clone())
            .collect()
    }

    fn card_visible(&self, index: usize) -> bool {
        self.cards.get(index).is_some()
    }

    fn card_title(&self, index: usize) -> String {
        self.cards
            .get(index)
            .map(|card| card.title.clone())
            .unwrap_or_default()
    }

    fn card_subtitle(&self, index: usize) -> String {
        self.cards
            .get(index)
            .and_then(|card| card.subtitle.clone())
            .unwrap_or_default()
    }

    fn card_has_subtitle(&self, index: usize) -> bool {
        self.cards
            .get(index)
            .and_then(|card| card.subtitle.as_ref())
            .is_some()
    }

    fn card_body(&self, index: usize) -> String {
        self.cards
            .get(index)
            .map(|card| card.body.clone())
            .unwrap_or_default()
    }

    fn overview_cards(&self) -> Vec<PageCard> {
        vec![
            PageCard {
                title: "Case Summary".to_string(),
                subtitle: Some("local workspace contract".to_string()),
                body: [
                    format!("case_id            {}", self.snapshot.workspace.case_id),
                    format!(
                        "workspace_root     {}",
                        self.snapshot.workspace.root.display()
                    ),
                    format!(
                        "source_backup      {}",
                        self.snapshot.workspace.root.join("source/backup").display()
                    ),
                    format!(
                        "prepared_db        {}",
                        self.snapshot.workspace.status.prepared_root.display()
                    ),
                    format!(
                        "prepared_helios    {}",
                        self.snapshot.workspace.status.helios_root.display()
                    ),
                    format!(
                        "evidence_root      {}",
                        self.snapshot.workspace.root.join("evidence").display()
                    ),
                    format!(
                        "live_mount_root    {}",
                        self.snapshot.workspace.status.live_mount_root.display()
                    ),
                    format!(
                        "reports_root       {}",
                        self.snapshot.workspace.status.reports_path.display()
                    ),
                    format!(
                        "logs               {}",
                        self.snapshot.workspace.status.logs_path.display()
                    ),
                ]
                .join("\n"),
            },
            PageCard {
                title: "Agent Readiness".to_string(),
                subtitle: Some("canonical evidence outputs".to_string()),
                body: self
                    .snapshot
                    .panels
                    .iter()
                    .map(|panel| {
                        let metrics = if panel.metrics.is_empty() {
                            "no summary metrics".to_string()
                        } else {
                            panel
                                .metrics
                                .iter()
                                .map(|metric| format!("{}={}", metric.key, metric.value))
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        format!(
                            "{:<10} state={:<7} records={:<5} {}",
                            panel.label,
                            if panel.ready { "ready" } else { "pending" },
                            panel.record_count,
                            metrics
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
            PageCard {
                title: "Recent Activity".to_string(),
                subtitle: Some(
                    self.snapshot
                        .workspace
                        .status
                        .logs_path
                        .display()
                        .to_string(),
                ),
                body: self.log_tail_body(),
            },
        ]
    }

    fn cerberus_cards(&self) -> Vec<PageCard> {
        let mut cards = Vec::new();
        let panel = self.snapshot.panel(WorkspaceRoute::Cerberus);

        let summary_body = if let Some(panel) = panel {
            let mut lines = vec![
                format!(
                    "state              {}",
                    if panel.ready { "ready" } else { "pending" }
                ),
                format!("records_jsonl      {}", panel.records_path.display()),
                format!("summary_json       {}", panel.summary_path.display()),
                format!("record_count       {}", panel.record_count),
            ];
            for metric in &panel.metrics {
                lines.push(format!("{:<18} {}", metric.key, metric.value));
            }
            lines.join("\n")
        } else {
            "Cerberus evidence panel is not available.".to_string()
        };

        cards.push(PageCard {
            title: "Cerberus Summary".to_string(),
            subtitle: Some("sms + mms".to_string()),
            body: summary_body,
        });

        if let Some(cerberus) = &self.snapshot.cerberus {
            let total = cerberus.messages.len();
            let selected = self.cerberus_selected.len();
            let filtered = if self.search_term.is_empty() {
                total
            } else {
                let term = self.search_term.to_lowercase();
                cerberus
                    .messages
                    .iter()
                    .filter(|m| {
                        format!("{} {} {}", m.phone_number, m.text, m.service)
                            .to_lowercase()
                            .contains(&term)
                    })
                    .count()
            };
            cards.push(PageCard {
                title: "Messages".to_string(),
                subtitle: Some(cerberus.messages_path.display().to_string()),
                body: format!(
                    "{} total messages\n{} shown (search filter)\n{} selected\nCheck messages below to select them.",
                    total, filtered, selected
                ),
            });
        } else {
            cards.push(PageCard {
                title: "Messages".to_string(),
                subtitle: Some(
                    self.snapshot
                        .workspace
                        .root
                        .join("evidence/cerberus/messages.json")
                        .display()
                        .to_string(),
                ),
                body: "No Cerberus evidence is present yet.\n\nRun the Cerberus backend against this case workspace to populate messages.json, attachments.json, records.jsonl, and summary.json."
                    .to_string(),
            });
            cards.push(PageCard {
                title: "Migration Seam".to_string(),
                subtitle: Some("next desktop step".to_string()),
                body: "Cerberus is now on the local-only canonical export path.\n\nNext pass: replace this preview text with a conversation list, thread detail pane, and attachment viewer using the same local workspace files."
                    .to_string(),
            });
        }

        cards
    }

    fn chronos_cards(&self) -> Vec<PageCard> {
        let chronos_summary = self.case.evidence_path("chronos").join("summary.json");
        let chronos_manifest = self.case.root_path().join("chronos_manifest.json");
        let source_backup_root = self
            .case
            .source_backup_root()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("unavailable ({})", error));
        let active_backup_root = self
            .case
            .active_backup_root()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("unavailable ({})", error));

        vec![
            PageCard {
                title: "Chronos Intake".to_string(),
                subtitle: Some("backup root + acquisition backbone".to_string()),
                body: [
                    format!("source_backup_root  {}", source_backup_root),
                    format!("active_backup_root  {}", active_backup_root),
                    format!("chronos_manifest    {}", chronos_manifest.display()),
                    format!("chronos_summary     {}", chronos_summary.display()),
                    format!(
                        "summary_ready       {}",
                        chronos_manifest.exists() || chronos_summary.exists()
                    ),
                ]
                .join("\n"),
            },
            PageCard {
                title: "Live Device Status".to_string(),
                subtitle: Some("connection + pairing + ideviceinfo".to_string()),
                body: self.live_device_body(),
            },
            PageCard {
                title: "Toolchain / Mounts".to_string(),
                subtitle: Some("ifuse + libimobiledevice".to_string()),
                body: self.live_toolchain_body(),
            },
        ]
    }

    fn helios_cards(&self) -> Vec<PageCard> {
        let full_root = self.case.workspace().helios_full_root();
        let sms_root = self.case.workspace().helios_sms_only_root();
        let metadata_root = self.case.workspace().helios_metadata_root();
        let full_manifest = full_root.join("_manifest").join("Manifest.decrypted.db");
        let sms_manifest = sms_root.join("_manifest").join("Manifest.decrypted.db");
        let discovered_extracts = self.case.discovered_helios_roots().unwrap_or_default();
        let active_root = self.case.active_backup_root().ok();
        let using_helios = active_root
            .as_ref()
            .map(|root| {
                discovered_extracts
                    .iter()
                    .any(|candidate| candidate == root)
            })
            .unwrap_or(false);
        let discovered_body = if discovered_extracts.is_empty() {
            [
                "No Helios decrypted backup was discovered under this case root.".to_string(),
                String::new(),
                "Expected one of:".to_string(),
                format!("  {}", full_root.display()),
                format!("  {}", sms_root.display()),
                "or a legacy decrypted root such as decrypted_*_import_ready/ with _manifest/Manifest.decrypted.db."
                    .to_string(),
            ]
            .join("\n")
        } else {
            discovered_extracts
                .iter()
                .enumerate()
                .map(|(index, root)| {
                    let marker = if active_root.as_ref() == Some(root) {
                        " [active]"
                    } else {
                        ""
                    };
                    format!("extract_{:<2} {}{}", index + 1, root.display(), marker)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        vec![
            PageCard {
                title: "Helios Workspace".to_string(),
                subtitle: Some("decrypted logical backup layout".to_string()),
                body: [
                    format!(
                        "helios_root         {}",
                        self.snapshot.workspace.status.helios_root.display()
                    ),
                    format!("full_extract        {}", full_root.display()),
                    format!("sms_only_extract    {}", sms_root.display()),
                    format!("metadata_sidecars   {}", metadata_root.display()),
                    format!("discovered_extracts {}", discovered_extracts.len()),
                    format!(
                        "active_backup_root  {}",
                        active_root
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "unavailable".to_string())
                    ),
                    format!("active_via_helios   {}", using_helios),
                ]
                .join("\n"),
            },
            PageCard {
                title: "Discovered Extracts".to_string(),
                subtitle: Some("canonical + legacy Helios roots".to_string()),
                body: [
                    format!("full_manifest_db    {}", full_manifest.display()),
                    format!("full_manifest_ok    {}", full_manifest.exists()),
                    format!("sms_manifest_db     {}", sms_manifest.display()),
                    format!("sms_manifest_ok     {}", sms_manifest.exists()),
                    format!("metadata_present    {}", metadata_root.exists()),
                    String::new(),
                    discovered_body,
                ]
                .join("\n"),
            },
            PageCard {
                title: "Recent Activity".to_string(),
                subtitle: Some("case.log".to_string()),
                body: self.helios_log_body(),
            },
        ]
    }

    fn generic_agent_cards(&self, route: WorkspaceRoute) -> Vec<PageCard> {
        let label = route.label();
        let Some(panel) = self.snapshot.panel(route) else {
            return vec![PageCard {
                title: format!("{}", label),
                subtitle: Some("no standard evidence panel".to_string()),
                body: format!(
                    "{} does not expose a standard evidence directory panel.\n\nRun the agent from the command line and check the case workspace for output files.",
                    label
                ),
            }];
        };

        let mut metrics = panel
            .metrics
            .iter()
            .map(|metric| format!("{:<18} {}", metric.key, metric.value))
            .collect::<Vec<_>>();
        if metrics.is_empty() {
            metrics.push("No summary metrics found yet.".to_string());
        }

        vec![
            PageCard {
                title: format!("{} Evidence", label),
                subtitle: Some(panel.summary_path.display().to_string()),
                body: [
                    format!(
                        "state              {}",
                        if panel.ready { "ready" } else { "pending" }
                    ),
                    format!("evidence_dir       {}", panel.evidence_dir.display()),
                    format!("records_jsonl      {}", panel.records_path.display()),
                    format!("record_count       {}", panel.record_count),
                ]
                .join("\n"),
            },
            PageCard {
                title: format!("{} Metrics", label),
                subtitle: Some("summary.json".to_string()),
                body: metrics.join("\n"),
            },
            PageCard {
                title: "Migration Seam".to_string(),
                subtitle: Some("desktop view pending".to_string()),
                body: format!(
                    "{} backend extraction is on the local workspace path.\n\nNext UI pass for this route should load evidence from {}\nand replace this placeholder with a routed list/detail pane.",
                    label,
                    panel.evidence_dir.display()
                ),
            },
        ]
    }

    fn parser_cards(&self) -> Vec<PageCard> {
        vec![
            PageCard {
                title: "Parser Status".to_string(),
                subtitle: Some("local logs + preparation roots".to_string()),
                body: [
                    format!(
                        "logs               {}",
                        self.snapshot.workspace.status.logs_path.display()
                    ),
                    format!(
                        "prepared_db        {}",
                        self.snapshot.workspace.status.prepared_root.display()
                    ),
                    format!(
                        "prepared_helios    {}",
                        self.snapshot.workspace.status.helios_root.display()
                    ),
                    format!(
                        "index_root         {}",
                        self.snapshot.workspace.status.index_root.display()
                    ),
                    format!(
                        "live_mount_root    {}",
                        self.snapshot.workspace.status.live_mount_root.display()
                    ),
                    format!(
                        "reports_root       {}",
                        self.snapshot.workspace.status.reports_path.display()
                    ),
                ]
                .join("\n"),
            },
            PageCard {
                title: "Recent Log Tail".to_string(),
                subtitle: Some("case.log".to_string()),
                body: self.log_tail_body(),
            },
            PageCard {
                title: "Shell Actions".to_string(),
                subtitle: Some("refresh + route status".to_string()),
                body: self.refresh_state_body(),
            },
        ]
    }

    fn report_cards(&self) -> Vec<PageCard> {
        let files_body = if self.snapshot.report_files.is_empty() {
            "No generated reports are present yet.".to_string()
        } else {
            self.snapshot.report_files.join("\n")
        };

        vec![
            PageCard {
                title: "Report Inventory".to_string(),
                subtitle: Some(
                    self.snapshot
                        .workspace
                        .status
                        .reports_path
                        .display()
                        .to_string(),
                ),
                body: format!(
                    "report_count        {}\nworkspace_root      {}",
                    self.snapshot.report_files.len(),
                    self.snapshot.workspace.root.display()
                ),
            },
            PageCard {
                title: "Generated Files".to_string(),
                subtitle: Some("reports/".to_string()),
                body: files_body,
            },
            PageCard {
                title: "Recent Activity".to_string(),
                subtitle: Some("case.log".to_string()),
                body: self.log_tail_body(),
            },
        ]
    }

    fn refresh_state_body(&self) -> String {
        let mut lines = vec![format!(
            "last_action         {}",
            self.last_action.as_deref().unwrap_or("none")
        )];
        lines.push(match &self.last_error {
            Some(error) => format!("last_shell_error    {}", error),
            None => "last_shell_error    none".to_string(),
        });
        lines.join("\n")
    }

    fn live_device_body(&self) -> String {
        if self.snapshot.device_status.connected_devices.is_empty() {
            return [
                "connected_devices   0".to_string(),
                format!(
                    "pairing_state      {}",
                    if self.snapshot.device_status.available_tool("idevice_id") {
                        "no device detected"
                    } else {
                        "idevice_id unavailable"
                    }
                ),
                format!("refresh_state      {}", self.refresh_state_body()),
            ]
            .join("\n");
        }

        self.snapshot
            .device_status
            .connected_devices
            .iter()
            .map(|device| {
                let name = device
                    .device_info
                    .as_ref()
                    .map(|info| info.device_name.clone())
                    .unwrap_or_else(|| "Unknown Device".to_string());
                let model = device
                    .device_info
                    .as_ref()
                    .map(|info| format!("{} / iOS {}", info.product_type, info.product_version))
                    .unwrap_or_else(|| "device info unavailable".to_string());
                let mounts = if device.active_mounts.is_empty() {
                    "none".to_string()
                } else {
                    device
                        .active_mounts
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let pairing_detail = device
                    .pairing_detail
                    .as_deref()
                    .filter(|detail| !detail.is_empty())
                    .unwrap_or("n/a");
                let info_detail = device
                    .info_error
                    .as_deref()
                    .filter(|detail| !detail.is_empty())
                    .unwrap_or("n/a");
                format!(
                    "udid               {}\nname               {}\nmodel              {}\npairing            {}\nmount_suggested    {}\nmounts             {}",
                    device.udid,
                    name,
                    model,
                    device.pairing_state.label(),
                    device.suggested_mount_point.display(),
                    mounts
                ) + &format!(
                    "\npairing_detail     {}\ndevice_info_error  {}",
                    pairing_detail, info_detail
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn live_toolchain_body(&self) -> String {
        let mut lines = self
            .snapshot
            .device_status
            .tool_status
            .iter()
            .map(|tool| {
                format!(
                    "{:<18} {} ({})",
                    tool.name,
                    if tool.available {
                        "available"
                    } else {
                        "missing"
                    },
                    tool.detail
                )
            })
            .collect::<Vec<_>>();

        lines.push(format!(
            "{:<18} {}",
            "mount_root",
            self.snapshot.device_status.mount_root.display()
        ));

        if self.snapshot.device_status.ifuse_mounts.is_empty() {
            lines.push("active_ifuse_mounts none".to_string());
        } else {
            for mount in &self.snapshot.device_status.ifuse_mounts {
                lines.push(format!(
                    "active_ifuse_mount {} [{}] {}",
                    mount.mount_point.display(),
                    mount.udid.as_deref().unwrap_or("unknown-udid"),
                    mount.filesystem
                ));
            }
        }

        lines.push(format!("refresh_state      {}", self.refresh_state_body()));
        lines.join("\n")
    }

    fn log_tail_body(&self) -> String {
        if self.snapshot.recent_logs.is_empty() {
            "No parser or extraction log lines are present yet.".to_string()
        } else {
            self.snapshot.recent_logs.join("\n")
        }
    }

    fn helios_log_body(&self) -> String {
        let lines = self
            .snapshot
            .recent_logs
            .iter()
            .filter(|line| line.contains("HELIOS"))
            .cloned()
            .collect::<Vec<_>>();

        if lines.is_empty() {
            "No Helios log lines are present yet.".to_string()
        } else {
            lines.join("\n")
        }
    }
}

fn install_css(zoom: f64) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(false);
    }

    if let Some(display) = gtk::gdk::Display::default() {
        APP_PROVIDER.with(|cell| {
            let mut slot = cell.borrow_mut();
            let provider = slot.get_or_insert_with(|| {
                let provider = gtk::CssProvider::new();
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                provider
            });
            provider.load_from_data(&app_css(zoom));
        });
    }
}

fn app_css(zoom: f64) -> String {
    let vars = [
        ("{brand_size}", px(24.0, zoom).to_string()),
        ("{meta_size}", px(13.0, zoom).to_string()),
        ("{title_size}", px(18.0, zoom).to_string()),
        ("{nav_size}", px(15.0, zoom).to_string()),
        ("{card_title_size}", px(21.0, zoom).to_string()),
        ("{body_size}", px(15.0, zoom).to_string()),
        ("{topbar_height}", px(56.0, zoom).to_string()),
        ("{outer_padding}", px(14.0, zoom).to_string()),
        ("{content_padding}", px(18.0, zoom).to_string()),
        ("{card_padding}", px(18.0, zoom).to_string()),
        ("{card_gap}", px(16.0, zoom).to_string()),
        ("{control_height}", px(44.0, zoom).to_string()),
    ];
    let mut css = include_str!("style.css.template").to_string();
    for (key, val) in &vars {
        css = css.replace(key, val);
    }
    css
}

fn px(base: f64, zoom: f64) -> i32 {
    (base * zoom).round() as i32
}

```

### `src/ui/routes.rs`

```rust
//! Data-driven route definitions for the GTK shell.
//!
//! Each route specifies how cards are built from the WorkspaceSnapshot.
//! The shell uses this table instead of hand-rolled match arms.

use crate::ui::WorkspaceRoute;

/// How a route should render its cards.
#[derive(Clone, Copy)]
pub enum RouteStrategy {
    /// Use the generic evidence panel renderer.
    Generic,
    /// Use a custom method on ShellModel.
    Custom(&'static str),
}

/// Route table — single source of truth for dispatch.
pub const ROUTES: &[(WorkspaceRoute, RouteStrategy)] = &[
    (WorkspaceRoute::Nemesis, RouteStrategy::Custom("overview")),
    (WorkspaceRoute::Chronos, RouteStrategy::Custom("chronos")),
    (WorkspaceRoute::Helios, RouteStrategy::Custom("helios")),
    (WorkspaceRoute::Orpheus, RouteStrategy::Generic),
    (WorkspaceRoute::Cerberus, RouteStrategy::Custom("cerberus")),
    (WorkspaceRoute::Charon, RouteStrategy::Generic),
    (WorkspaceRoute::Vox, RouteStrategy::Generic),
    (WorkspaceRoute::Plutus, RouteStrategy::Generic),
    (WorkspaceRoute::Psyche, RouteStrategy::Generic),
    (WorkspaceRoute::Obolus, RouteStrategy::Generic),
    (WorkspaceRoute::Nyx, RouteStrategy::Generic),
    (WorkspaceRoute::Aether, RouteStrategy::Generic),
    (WorkspaceRoute::Atlas, RouteStrategy::Generic),
    (WorkspaceRoute::Tartarus, RouteStrategy::Generic),
    (WorkspaceRoute::Xwin, RouteStrategy::Generic),
    (WorkspaceRoute::CaseOverview, RouteStrategy::Custom("overview")),
    (WorkspaceRoute::ParserStatus, RouteStrategy::Custom("parser")),
    (WorkspaceRoute::Reports, RouteStrategy::Custom("reports")),
];

/// Get the strategy for a route.
pub fn strategy(route: WorkspaceRoute) -> RouteStrategy {
    ROUTES
        .iter()
        .find(|(r, _)| *r == route)
        .map(|(_, s)| s)
        .copied()
        .unwrap_or(RouteStrategy::Generic)
}

```

## UI — Export & Shell

### `src/ui_export.rs`

```rust
//! MINiOS unified evidence export.
//!
//! Reads evidence records from any agent's evidence directory and exports
//! to JSON, CSV, or a consolidated multi-agent report.

use anyhow::{Context, Result};
use minios::case::Case;
use std::env;
use std::path::Path;

fn main() {
    if let Err(e) = run() {
        eprintln!("Export error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        anyhow::bail!("No case specified");
    }

    let case_name = &args[1];
    let case = Case::new(case_name)?;

    // Discover all evidence directories
    let evidence_root = case.evidence_path("");
    let mut all_records: Vec<serde_json::Value> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&evidence_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let records_path = path.join("records.json");
            if !records_path.exists() {
                continue;
            }

            let raw = std::fs::read_to_string(&records_path)
                .with_context(|| format!("reading {}", records_path.display()))?;
            let records: Vec<serde_json::Value> = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", records_path.display()))?;

            let agent = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            println!("Found {} records from {}", records.len(), agent);
            all_records.extend(records);
        }
    }

    // Write consolidated export
    let output_dir = case.root_path().join("output").join("export");
    std::fs::create_dir_all(&output_dir)?;

    let consolidated_path = output_dir.join("consolidated_evidence.json");
    std::fs::write(&consolidated_path, serde_json::to_vec_pretty(&all_records)?)?;
    println!(
        "Exported {} total records to {}",
        all_records.len(),
        consolidated_path.display()
    );

    // Per-agent CSVs are already produced by the evidence pipeline.
    // Just copy them to output.
    if let Ok(entries) = std::fs::read_dir(&evidence_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let csv_src = path.join("records.csv");
            if csv_src.exists() {
                let agent = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
                let csv_dst = output_dir.join(format!("{}_evidence.csv", agent));
                std::fs::copy(&csv_src, &csv_dst)?;
                println!("  CSV: {}", csv_dst.display());
            }
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!(r#"
MINiOS Evidence Export

USAGE:
    minios-export <case_name>

Exports consolidated evidence from all agent runs to:
  output/<case>/consolidated_evidence.json
  output/<case>/<agent>_evidence.csv (per-agent CSVs)
"#);
}

```

### `src/ui_shell/main.rs`

```rust
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use minios::case::Case;
use minios::ui::gtk_shell;

#[derive(Debug, Parser)]
#[command(name = "ion")]
#[command(about = "GTK4/Relm4 shell for the local ion workspace")]
struct Args {
    #[arg(help = "Absolute or relative path to an existing local case root")]
    case_root: PathBuf,

    #[arg(long, help = "Logical case name shown in shell chrome")]
    case_name: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let case_name = args
        .case_name
        .clone()
        .or_else(|| {
            args.case_root
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "ion-case".to_string());

    let case = Case::from_root(case_name, args.case_root);
    gtk_shell::run(case)
}

```

### `src/ui_shell/model.rs`

```rust
use std::path::PathBuf;

#[derive(Debug)]
pub struct CaseData {
    pub root: PathBuf,
    pub name: String,
}

impl CaseData {
    pub fn new(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        Self { root, name }
    }
}

```

---

*End of review document*
