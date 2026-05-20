//! Voicemail - first-class voicemail database extraction.

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agents::cerberus_models::VoicemailRecord as HermesVoicemailRecord;
use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver};
use crate::common::target::voicemail_target;
use crate::evidence::EvidenceRecord;

pub const VOICEMAIL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VoicemailEvidence {
    record_id: String,
    rowid: i64,
    remote_uid: Option<i64>,
    date_raw: Option<i64>,
    date_utc: Option<String>,
    token: Option<String>,
    sender: Option<String>,
    callback_num: Option<String>,
    phone_number: Option<String>,
    receiver: Option<String>,
    duration_seconds: Option<i64>,
    expiration_raw: Option<i64>,
    trashed_date_raw: Option<i64>,
    flags: Option<i64>,
    label: Option<String>,
    uuid: Option<String>,
    source_db_path: String,
    source_media_path: Option<String>,
    transcript_path: Option<String>,
    is_transcribed: bool,
}

pub struct VoicemailAgent;

impl Agent for VoicemailAgent {
    const NAME: &'static str = "Voicemail";
    const SLUG: &'static str = "voicemail";
    const SCHEMA_VERSION: u32 = VOICEMAIL_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let source_db = resolve_voicemail_db(ctx)?;
        let records = extract_voicemail_records(&source_db)?;
        write_sidecar_records(ctx, &records)?;

        let mut evidence = Vec::new();
        evidence.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "voicemail_report".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "source_db_path": source_db.display().to_string(),
                "voicemails_extracted": records.len(),
                "unique_senders": unique_count(records.iter().filter_map(|record| record.sender.as_deref())),
                "unique_receivers": unique_count(records.iter().filter_map(|record| record.receiver.as_deref())),
                "total_duration_seconds": records.iter().filter_map(|record| record.duration_seconds).sum::<i64>(),
                "transcribed_count": records.iter().filter(|record| record.is_transcribed).count(),
                "by_sender": count_by(records.iter().filter_map(|record| record.sender.as_deref())),
            }),
        });

        for record in records {
            evidence.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "voicemail".to_string(),
                timestamp: record
                    .date_utc
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(record)?,
            });
        }

        Ok(evidence)
    }
}

fn resolve_voicemail_db(ctx: &AgentCtx) -> Result<PathBuf> {
    let clean_db = ctx
        .case
        .root_path()
        .join("clean")
        .join("Library")
        .join("Voicemail")
        .join("voicemail.db");
    if clean_db.exists() {
        return Ok(clean_db);
    }

    let flat_clean_db = ctx.case.root_path().join("clean").join("voicemail.db");
    if flat_clean_db.exists() {
        return Ok(flat_clean_db);
    }

    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = voicemail_target();
    let Some(resolved) = resolver.resolve_known_target(&target)? else {
        anyhow::bail!("voicemail.db was not found in clean output or backup manifest");
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
    Ok(prepared.working_path)
}

fn extract_voicemail_records(source_db: &Path) -> Result<Vec<VoicemailEvidence>> {
    let conn = crate::common::sqlite::open_readonly(source_db)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT
            ROWID,
            remote_uid,
            date,
            token,
            sender,
            callback_num,
            duration,
            expiration,
            trashed_date,
            flags,
            receiver,
            label,
            uuid
        FROM voicemail
        ORDER BY date DESC, ROWID DESC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        let rowid = row.get::<_, i64>(0)?;
        let remote_uid = row.get::<_, Option<i64>>(1)?;
        let date_raw = row.get::<_, Option<i64>>(2)?;
        let token = row.get::<_, Option<String>>(3)?;
        let sender = row.get::<_, Option<String>>(4)?;
        let callback_num = row.get::<_, Option<String>>(5)?;
        let duration_seconds = row.get::<_, Option<i64>>(6)?;
        let expiration_raw = row.get::<_, Option<i64>>(7)?;
        let trashed_date_raw = row.get::<_, Option<i64>>(8)?;
        let flags = row.get::<_, Option<i64>>(9)?;
        let receiver = row.get::<_, Option<String>>(10)?;
        let label = row.get::<_, Option<String>>(11)?;
        let uuid = row.get::<_, Option<String>>(12)?;

        Ok(VoicemailEvidence {
            record_id: format!("voicemail:{}", rowid),
            rowid,
            remote_uid,
            date_raw,
            date_utc: date_raw.and_then(unix_to_rfc3339),
            token,
            phone_number: callback_num.clone().or_else(|| sender.clone()),
            sender,
            callback_num,
            receiver,
            duration_seconds,
            expiration_raw,
            trashed_date_raw,
            flags,
            label,
            uuid,
            source_db_path: source_db.display().to_string(),
            source_media_path: None,
            transcript_path: None,
            is_transcribed: false,
        })
    })?;

    Ok(rows.filter_map(|row| row.ok()).collect())
}

fn write_sidecar_records(ctx: &AgentCtx, records: &[VoicemailEvidence]) -> Result<()> {
    let sidecar = records
        .iter()
        .map(|record| HermesVoicemailRecord {
            id: record.rowid,
            date: record.date_raw,
            timestamp: record.date_raw.unwrap_or_default(),
            duration: record.duration_seconds,
            phone_number: record.phone_number.clone(),
            sender: record.sender.clone(),
            filename: None,
            file_path: record.source_media_path.clone(),
            file_size: None,
            hash: None,
            transcript_path: record.transcript_path.clone(),
            diarization_path: None,
            confidence: None,
            speakers: None,
            is_transcribed: record.is_transcribed,
            is_diarized: false,
        })
        .collect::<Vec<_>>();

    fs::write(
        ctx.evidence_dir.join("voicemail_records.json"),
        serde_json::to_vec_pretty(&sidecar)?,
    )?;
    Ok(())
}

fn unix_to_rfc3339(value: i64) -> Option<String> {
    Utc.timestamp_opt(value, 0)
        .single()
        .map(|value| DateTime::<Utc>::to_rfc3339(&value))
}

fn unique_count<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    values
        .filter(|value| !value.trim().is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn count_by<'a>(values: impl Iterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed.to_string()).or_insert(0) += 1;
    }
    counts
}
