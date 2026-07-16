//! Unified agent framework.
//!
//! All forensic agents implement the `Agent` trait.
//! A single binary (`minios`) dispatches to the correct agent at runtime.

use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static JSON_STREAMING: AtomicBool = AtomicBool::new(false);

/// Enable NDJSON streaming to stdout for agent progress events.
pub fn set_json_streaming(enabled: bool) {
    JSON_STREAMING.store(enabled, Ordering::SeqCst);
}

/// Check if NDJSON streaming is enabled.
pub fn is_json_streaming() -> bool {
    JSON_STREAMING.load(Ordering::SeqCst)
}

/// Emit an NDJSON progress event to stdout when streaming is enabled.
pub fn emit_json_event(agent: &str, event: &str, payload: serde_json::Value) {
    if !is_json_streaming() {
        return;
    }
    let line = serde_json::json!({
        "event": event,
        "agent": agent,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "payload": payload,
    });
    println!("{}", line);
}

pub mod models;

// Model modules used by UI or multiple agents
pub mod cerberus_models;
pub mod charon_models;
pub mod nyx_models;
pub mod obolus_models;
pub mod orpheus_crypto;
pub mod orpheus_ingest;
pub mod orpheus_recon;
pub mod orpheus_report;
pub mod psyche_models;

// Agent modules (ported in Session 3)
pub mod aether;
pub mod atlas;
pub mod cerberus;
pub mod charon;
pub mod nyx;
pub mod obolus;
pub mod orpheus;
pub mod orpheus_inventory;
pub mod plutus;
pub mod psyche;

// Lightweight agents (no separate model files)
pub mod echo;
pub mod hermes;
pub mod intake;
pub mod vigil;
pub mod voicemail;

use crate::case::Case;
use crate::evidence::{write_evidence, EvidenceRecord};
use serde::Serialize;

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
        let ctx = AgentCtx::new(case_name, Self::SLUG, Self::NAME)?;

        ctx.log(&format!("{} extraction started", Self::NAME));

        let records =
            Self::extract(&ctx).with_context(|| format!("{} extraction failed", Self::NAME))?;

        write_evidence(
            &ctx.evidence_dir,
            Self::NAME,
            Self::SLUG,
            Self::SCHEMA_VERSION,
            case_name,
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
    pub agent_name: String,
}

impl AgentCtx {
    /// Create context from an explicit case name and agent slug.
    pub fn new(case_name: &str, slug: &str, agent_name: &str) -> Result<Self> {
        let case = Case::new(case_name)?;
        case.open(slug)?;

        let evidence_dir = case.evidence_path(slug);
        std::fs::create_dir_all(&evidence_dir)?;

        let backup_root = case.active_backup_root()?;

        Ok(Self {
            case,
            evidence_dir,
            backup_root,
            agent_name: agent_name.to_string(),
        })
    }

    /// Open a SQLite database from the iOS backup by relative path.
    /// Path is relative to backup root (e.g., "SMS/sms.db").
    pub fn open_backup_db(&self, relative_path: &str) -> Result<crate::common::sqlite::SqliteConn> {
        let path = self.backup_root.join(relative_path);
        crate::common::sqlite::open_readonly(&path)
    }

    /// Log a message to the case log.
    /// When NDJSON streaming is enabled, also emits the message to stdout.
    pub fn log(&self, message: &str) {
        let _ = self.case.log(message, None);
        if is_json_streaming() {
            emit_json_event(
                &self.agent_name,
                "log",
                serde_json::json!({"message": message}),
            );
        }
    }

    /// Emit a structured progress event to stdout when NDJSON streaming is enabled.
    pub fn emit_progress(&self, _agent: &str, step: usize, total: usize, label: &str) {
        if is_json_streaming() {
            emit_json_event(
                &self.agent_name,
                "progress",
                serde_json::json!({
                    "step": step,
                    "total": total,
                    "label": label,
                }),
            );
        }
    }

    /// Read a plist file from the backup.
    pub fn read_plist(&self, relative_path: &str) -> Result<plist::Value> {
        let path = self.backup_root.join(relative_path);
        let file = std::fs::File::open(&path)
            .with_context(|| format!("opening plist {}", path.display()))?;
        plist::from_reader(file).with_context(|| format!("parsing plist {}", path.display()))
    }
}

/// Static agent definition for UI consumption.
#[derive(Debug, Clone, Serialize)]
pub struct AgentDefinition {
    pub name: &'static str,
    pub slug: &'static str,
    pub description: &'static str,
    pub schema_version: u32,
    pub category: &'static str,
}

/// Return definitions for all available agents.
pub fn get_agent_definitions() -> Vec<AgentDefinition> {
    vec![
        AgentDefinition {
            name: "Cerberus",
            slug: "cerberus",
            description: "Unified communications extraction (SMS/MMS, calls, voicemail, contacts)",
            schema_version: 1,
            category: "Core Evidence",
        },
        AgentDefinition {
            name: "Charon",
            slug: "charon",
            description: "Photo and media extraction with metadata recovery",
            schema_version: 1,
            category: "Core Evidence",
        },
        AgentDefinition {
            name: "Nyx",
            slug: "nyx",
            description: "Safari history, bookmarks, autofill data extraction",
            schema_version: 1,
            category: "Core Evidence",
        },
        AgentDefinition {
            name: "Obolus",
            slug: "obolus",
            description: "Apple Notes extraction and analysis",
            schema_version: 1,
            category: "Core Evidence",
        },
        AgentDefinition {
            name: "Plutus",
            slug: "plutus",
            description: "Apple Wallet, financial data and transactions",
            schema_version: 1,
            category: "Core Evidence",
        },
        AgentDefinition {
            name: "Aether",
            slug: "aether",
            description: "EXIF GPS extraction from photos for location intelligence",
            schema_version: 1,
            category: "Location",
        },
        AgentDefinition {
            name: "Atlas",
            slug: "atlas",
            description: "App-based location extraction (Maps, Snapchat, etc.)",
            schema_version: 1,
            category: "Location",
        },
        AgentDefinition {
            name: "Vigil",
            slug: "vigil",
            description: "System-level artifact extraction",
            schema_version: 1,
            category: "System",
        },
        AgentDefinition {
            name: "Echo",
            slug: "echo",
            description: "Audio evidence extraction (voicemail, recordings)",
            schema_version: 1,
            category: "Media",
        },
        AgentDefinition {
            name: "Voicemail",
            slug: "voicemail",
            description: "First-class voicemail database extraction and normalization",
            schema_version: 1,
            category: "Media",
        },
        AgentDefinition {
            name: "Orpheus",
            slug: "orpheus",
            description: "Generic SQLite database reconnaissance",
            schema_version: 1,
            category: "Advanced",
        },
        AgentDefinition {
            name: "Psyche",
            slug: "psyche",
            description: "AI behavioral analysis and pattern detection",
            schema_version: 1,
            category: "AI Analysis",
        },
        AgentDefinition {
            name: "Hermes",
            slug: "hermes",
            description: "Media catalog, transcription and speaker diarization",
            schema_version: 1,
            category: "Media",
        },
        AgentDefinition {
            name: "Intake",
            slug: "intake",
            description:
                "External evidence intake for documents, spreadsheets, recordings, and loose media",
            schema_version: 1,
            category: "Case Evidence",
        },
    ]
}

/// Agent dispatch table. Maps agent name to agent runner.
pub fn dispatch(agent_name: &str, case_name: &str) -> Result<()> {
    match agent_name {
        "vigil" => vigil::VigilAgent::run_with_case(case_name),
        "voicemail" => voicemail::VoicemailAgent::run_with_case(case_name),
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
        "hermes" => hermes::HermesAgent::run_with_case(case_name),
        "intake" => intake::IntakeAgent::run_with_case(case_name),
        "intake-watch" => intake::watch_case(case_name),
        _ => anyhow::bail!(
            "Unknown agent: {}. Available: vigil, voicemail, echo, aether, plutus, cerberus, charon, nyx, obolus, atlas, orpheus, psyche, hermes, intake, intake-watch",
            agent_name
        ),
    }
}
