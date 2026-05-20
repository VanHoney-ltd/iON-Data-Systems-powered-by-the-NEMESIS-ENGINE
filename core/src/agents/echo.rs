//! Echo — Deterministic audio evidence inventory.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::agents::cerberus_models::VoicemailRecord;
use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::compute_sha256;
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
                payload: strip_nulls(json!({
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
                })),
            });
        }

        Ok(records)
    }
}

fn strip_nulls(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter_map(|(key, value)| {
                    let cleaned = strip_nulls(value);
                    (!cleaned.is_null()).then_some((key, cleaned))
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(strip_nulls).collect())
        }
        other => other,
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

    // Read unified voicemail_records.json written by Cerberus
    let unified_path = voicemail_dir.join("voicemail_records.json");
    if unified_path.exists() {
        let raw = fs::read(&unified_path)
            .with_context(|| format!("Failed to read {}", unified_path.display()))?;
        let parsed: Vec<VoicemailRecord> = serde_json::from_slice(&raw)
            .with_context(|| format!("Failed to parse {}", unified_path.display()))?;
        for record in parsed {
            records_by_id.entry(record.id).or_insert(record);
        }
    }

    // Also read legacy voicemail_*.json files if present
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
        if file_name == "voicemail_records.json" {
            continue; // Already handled above
        }
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

fn collect_audio_metadata(path: &Path, db_duration: Option<i64>) -> Result<AudioMetadata> {
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

fn normalize_db_duration(value: Option<i64>) -> Option<f64> {
    value.and_then(|v| (v > 0).then_some(v as f64))
}

fn unix_to_rfc3339(timestamp: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(timestamp, 0).map(|dt| dt.to_rfc3339())
}
