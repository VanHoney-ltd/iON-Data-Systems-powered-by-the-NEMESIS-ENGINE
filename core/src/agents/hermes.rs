//! Hermes — Media catalog, transcription & diarization agent.
//!
//! Hermes runs after Charon to collect metadata for all media files
//! (photos, videos, voicemail, voice notes, MMS attachments, screen recordings).
//! It builds a unified catalog with thumbnails, dates, times, and sources.
//!
//! Transcription and speaker diarization are performed ONLY when a case has
//! contacts assigned for investigation. Only media linked to those contacts
//! (voicemail, MMS attachments) is transcribed. Camera/screen recordings
//! are cataloged but not auto-transcribed unless explicitly matched.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agents::cerberus_models::{Attachment, VoicemailRecord};
use crate::agents::charon_models::AssetRecord;
use crate::agents::intake::IntakeRecord;
use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

const HERMES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MediaCatalogItem {
    pub id: String,
    pub source: String,
    pub media_type: String,
    pub mime_type: String,
    pub filename: String,
    pub file_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub file_size: Option<u64>,
    pub duration_seconds: Option<f64>,
    pub created_date: Option<String>,
    pub added_date: Option<String>,
    pub phone_number: Option<String>,
    pub contact_name: Option<String>,
    pub direction: Option<String>,
    pub is_transcribed: bool,
    pub transcript_path: Option<String>,
    pub merged_transcript_path: Option<String>,
    pub pdf_report_path: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HermesReport {
    pub catalog_items: usize,
    pub queue_items: usize,
    pub transcribed_items: usize,
    pub skipped_items: usize,
    pub investigation_contacts: Vec<String>,
    pub transcription_enabled: bool,
    pub source_breakdown: BTreeMap<String, usize>,
    pub media_type_breakdown: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TranscriptionQueueItem {
    pub media_id: String,
    pub source: String,
    pub filename: String,
    pub file_path: Option<String>,
    pub media_type: String,
    pub mime_type: String,
    pub status: String,
    pub reason: String,
    pub duration_seconds: Option<String>,
    pub phone_number: Option<String>,
    pub contact_name: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptionRunRecord {
    pub media_id: String,
    pub source: String,
    pub filename: String,
    pub file_path: Option<String>,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub transcript_path: Option<String>,
    pub merged_transcript_path: Option<String>,
    pub pdf_report_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
struct HermesTranscribeOptions {
    source: Option<String>,
    media_id: Option<String>,
    limit: Option<usize>,
    dry_run: bool,
}

pub struct HermesAgent;

pub fn run_transcription_command(case_name: &str, args: &[String]) -> Result<()> {
    let options = parse_transcribe_options(args)?;
    let ctx = AgentCtx::new(case_name, HermesAgent::SLUG, "Hermes Transcribe")?;
    let queue_path = ctx.evidence_dir.join("transcription_queue.json");
    if !queue_path.exists() {
        anyhow::bail!(
            "Hermes transcription queue not found at {}. Run `minios hermes {}` first.",
            queue_path.display(),
            case_name
        );
    }

    let raw = fs::read(&queue_path).with_context(|| format!("reading {}", queue_path.display()))?;
    let mut queue: Vec<TranscriptionQueueItem> = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing {}", queue_path.display()))?;
    let mut selected = select_transcription_items(&queue, &options);
    if let Some(limit) = options.limit {
        selected.truncate(limit);
    }

    if selected.is_empty() {
        println!("Hermes transcribe: no queued items matched the requested filters");
        return Ok(());
    }

    println!(
        "Hermes transcribe: {} full media item(s) selected",
        selected.len()
    );
    for item in &selected {
        println!(
            "  {} | {} | {}",
            item.media_id,
            item.source,
            item.file_path.as_deref().unwrap_or("<no source path>")
        );
    }

    if options.dry_run {
        println!("Hermes transcribe dry-run complete; no media processed");
        return Ok(());
    }

    let mut manifest = read_transcription_manifest(&ctx.evidence_dir)?;
    for queue_item in selected {
        let mut item = media_item_from_queue(&queue_item);
        let started_at = Utc::now().to_rfc3339();
        let mut record = TranscriptionRunRecord {
            media_id: queue_item.media_id.clone(),
            source: queue_item.source.clone(),
            filename: queue_item.filename.clone(),
            file_path: queue_item.file_path.clone(),
            status: "running".to_string(),
            started_at,
            completed_at: None,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            error: None,
        };

        match transcribe_media_item(&ctx, &mut item) {
            Ok(()) => {
                record.status = "complete".to_string();
                record.completed_at = Some(Utc::now().to_rfc3339());
                record.transcript_path = item.transcript_path.clone();
                record.merged_transcript_path = item.merged_transcript_path.clone();
                record.pdf_report_path = item.pdf_report_path.clone();
                println!("Hermes transcribed {}", queue_item.media_id);
            }
            Err(err) => {
                record.status = "error".to_string();
                record.completed_at = Some(Utc::now().to_rfc3339());
                record.error = Some(format!("{:#}", err));
                println!(
                    "Hermes transcription failed for {}: {:#}",
                    queue_item.media_id, err
                );
            }
        }
        manifest.push(record);
        write_transcription_manifest(&ctx.evidence_dir, &manifest)?;
        apply_transcription_runs_to_queue(&mut queue, &manifest);
        write_transcription_queue_outputs(&ctx.evidence_dir, &queue)?;
    }

    Ok(())
}

impl Agent for HermesAgent {
    const NAME: &'static str = "Hermes";
    const SLUG: &'static str = "hermes";
    const SCHEMA_VERSION: u32 = HERMES_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();

        // 1. Load investigation contacts from case registry
        let (investigation_phones, transcription_enabled) = load_investigation_contacts(&ctx.case);
        ctx.emit_progress("Hermes", 0, 4, "Loading investigation contacts");
        ctx.log(&format!(
            "Hermes: {} investigation contacts loaded. Transcription enabled: {}",
            investigation_phones.len(),
            transcription_enabled
        ));

        // 2. Collect media from all sources
        let mut catalog: Vec<MediaCatalogItem> = Vec::new();

        ctx.emit_progress("Hermes", 1, 4, "Collecting Charon media");
        collect_charon_media(ctx, &mut catalog)?;
        ctx.emit_progress("Hermes", 2, 4, "Collecting Echo media");
        collect_echo_media(ctx, &mut catalog)?;
        ctx.emit_progress("Hermes", 3, 4, "Collecting Cerberus attachments");
        collect_cerberus_attachments(ctx, &mut catalog)?;
        collect_intake_media(ctx, &mut catalog)?;
        dedupe_catalog(&mut catalog);

        ctx.emit_progress("Hermes", 4, 4, "Catalog complete");
        ctx.log(&format!(
            "Hermes: {} total media items cataloged",
            catalog.len()
        ));

        // 3. Queue audio/video for dashboard-approved transcription.
        let transcription_queue = build_transcription_queue(&catalog);
        write_catalog_outputs(ctx, &catalog, &transcription_queue)?;

        // 4. Optionally transcribe media linked to investigation contacts when explicitly enabled.
        let mut transcribed_count = 0;
        let mut skipped_count;

        if transcription_enabled
            && std::env::var("ION_HERMES_AUTO_TRANSCRIBE").as_deref() == Ok("1")
        {
            let transcribe_items: Vec<usize> = catalog
                .iter()
                .enumerate()
                .filter(|(_, item)| should_transcribe(item, &investigation_phones))
                .map(|(idx, _)| idx)
                .collect();
            let total_to_transcribe = transcribe_items.len();
            skipped_count = catalog.len() - total_to_transcribe;

            for (i, idx) in transcribe_items.into_iter().enumerate() {
                let item = &mut catalog[idx];
                ctx.emit_progress(
                    "Hermes",
                    i + 1,
                    total_to_transcribe,
                    &format!("Transcribing {}", item.id),
                );
                match transcribe_media_item(ctx, item) {
                    Ok(_) => {
                        transcribed_count += 1;
                        ctx.log(&format!(
                            "Hermes transcribed: {} -> {}",
                            item.id,
                            item.merged_transcript_path
                                .as_ref()
                                .unwrap_or(&"?".to_string())
                        ));
                    }
                    Err(e) => {
                        ctx.log(&format!(
                            "Hermes transcription failed for {}: {}",
                            item.id, e
                        ));
                        skipped_count += 1;
                    }
                }
            }
        } else {
            skipped_count = catalog.len();
        }

        // 5. Generate PDF report
        if catalog.len() <= 1000 {
            let pdf_path = ctx.evidence_dir.join("media_catalog.pdf");
            match generate_pdf_catalog(ctx, &catalog, &pdf_path) {
                Ok(_) => ctx.log(&format!("Hermes PDF catalog: {}", pdf_path.display())),
                Err(e) => ctx.log(&format!("Hermes PDF generation failed: {}", e)),
            }
        } else {
            ctx.log(&format!(
                "Hermes PDF catalog skipped for {} media items; use index.html/media_catalog.csv",
                catalog.len()
            ));
        }

        // 6. Report record
        let report = HermesReport {
            catalog_items: catalog.len(),
            queue_items: transcription_queue.len(),
            transcribed_items: transcribed_count,
            skipped_items: skipped_count,
            investigation_contacts: investigation_phones.into_iter().collect(),
            transcription_enabled,
            source_breakdown: breakdown_by(&catalog, |item| item.source.clone()),
            media_type_breakdown: breakdown_by(&catalog, |item| item.media_type.clone()),
        };

        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        });

        // 7. Individual media records
        for item in catalog {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "media_item".to_string(),
                timestamp: item
                    .added_date
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(&item)?,
            });
        }

        Ok(records)
    }
}

// ---------------------------------------------------------------------------
// Contact loading
// ---------------------------------------------------------------------------

fn load_investigation_contacts(case: &crate::case::Case) -> (HashSet<String>, bool) {
    match case.registry_entry() {
        Ok(Some(entry)) => {
            let phones: HashSet<String> = entry.phone_numbers.into_iter().collect();
            let enabled = !phones.is_empty();
            (phones, enabled)
        }
        _ => (HashSet::new(), false),
    }
}

fn parse_transcribe_options(args: &[String]) -> Result<HermesTranscribeOptions> {
    let mut options = HermesTranscribeOptions::default();
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--source" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow::anyhow!("--source requires a value"))?;
                options.source = Some(value.to_string());
            }
            "--media-id" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow::anyhow!("--media-id requires a value"))?;
                options.media_id = Some(value.to_string());
            }
            "--limit" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow::anyhow!("--limit requires a value"))?;
                options.limit = Some(
                    value
                        .parse()
                        .with_context(|| format!("invalid --limit value `{}`", value))?,
                );
            }
            "--dry-run" => {
                options.dry_run = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: minios hermes-transcribe <case> [--source NAME] [--media-id ID] [--limit N] [--dry-run]"
                );
                println!("Transcribes full selected media files from evidence/hermes/transcription_queue.json.");
                std::process::exit(0);
            }
            other => anyhow::bail!("Unknown hermes-transcribe option `{}`", other),
        }
        idx += 1;
    }
    Ok(options)
}

fn select_transcription_items(
    queue: &[TranscriptionQueueItem],
    options: &HermesTranscribeOptions,
) -> Vec<TranscriptionQueueItem> {
    queue
        .iter()
        .filter(|item| item.status == "needs_review")
        .filter(|item| {
            options
                .source
                .as_ref()
                .map(|source| item.source.eq_ignore_ascii_case(source))
                .unwrap_or(true)
        })
        .filter(|item| {
            options
                .media_id
                .as_ref()
                .map(|media_id| item.media_id == *media_id)
                .unwrap_or(true)
        })
        .filter(|item| {
            item.file_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file())
        })
        .cloned()
        .collect()
}

fn media_item_from_queue(queue_item: &TranscriptionQueueItem) -> MediaCatalogItem {
    MediaCatalogItem {
        id: queue_item.media_id.clone(),
        source: queue_item.source.clone(),
        media_type: queue_item.media_type.clone(),
        mime_type: queue_item.mime_type.clone(),
        filename: queue_item.filename.clone(),
        file_path: queue_item.file_path.clone(),
        thumbnail_path: None,
        file_size: queue_item
            .file_path
            .as_deref()
            .and_then(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len()),
        duration_seconds: queue_item
            .duration_seconds
            .as_deref()
            .and_then(|value| value.parse().ok()),
        created_date: None,
        added_date: None,
        phone_number: queue_item.phone_number.clone(),
        contact_name: queue_item.contact_name.clone(),
        direction: None,
        is_transcribed: false,
        transcript_path: None,
        merged_transcript_path: None,
        pdf_report_path: None,
        sha256: queue_item.sha256.clone(),
    }
}

fn transcription_manifest_path(evidence_dir: &Path) -> PathBuf {
    evidence_dir.join("transcription_runs.json")
}

fn read_transcription_manifest(evidence_dir: &Path) -> Result<Vec<TranscriptionRunRecord>> {
    let path = transcription_manifest_path(evidence_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&raw).with_context(|| format!("parsing {}", path.display()))
}

fn write_transcription_manifest(
    evidence_dir: &Path,
    records: &[TranscriptionRunRecord],
) -> Result<()> {
    let path = transcription_manifest_path(evidence_dir);
    fs::write(&path, serde_json::to_vec_pretty(records)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn apply_transcription_runs_to_queue(
    queue: &mut [TranscriptionQueueItem],
    runs: &[TranscriptionRunRecord],
) {
    let mut latest_by_media_id: BTreeMap<&str, &TranscriptionRunRecord> = BTreeMap::new();
    for run in runs {
        latest_by_media_id.insert(run.media_id.as_str(), run);
    }

    for item in queue {
        let Some(run) = latest_by_media_id.get(item.media_id.as_str()) else {
            continue;
        };
        match run.status.as_str() {
            "complete" => {
                item.status = "complete".to_string();
                item.reason = "transcription complete; see transcription_runs.json".to_string();
            }
            "error" => {
                item.status = "error".to_string();
                item.reason = run
                    .error
                    .clone()
                    .unwrap_or_else(|| "transcription failed".to_string());
            }
            other => {
                item.status = other.to_string();
                item.reason = "transcription in progress; see transcription_runs.json".to_string();
            }
        }
    }
}

fn write_transcription_queue_outputs(
    evidence_dir: &Path,
    queue: &[TranscriptionQueueItem],
) -> Result<()> {
    fs::write(
        evidence_dir.join("transcription_queue.json"),
        serde_json::to_vec_pretty(queue)?,
    )?;
    export_transcription_queue_csv(queue, &evidence_dir.join("transcription_queue.csv"))
}

// ---------------------------------------------------------------------------
// Media collection
// ---------------------------------------------------------------------------

fn collect_charon_media(ctx: &AgentCtx, catalog: &mut Vec<MediaCatalogItem>) -> Result<()> {
    let assets_path = ctx.case.evidence_path("charon").join("assets.json");
    if !assets_path.exists() {
        return Ok(());
    }

    let raw =
        fs::read(&assets_path).with_context(|| format!("reading {}", assets_path.display()))?;
    let assets: Vec<AssetRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing {}", assets_path.display()))?;

    for asset in assets {
        let media_type_str = format!("{:?}", asset.media_type);
        let _is_audio_video = asset.mime_type.starts_with("video/")
            || asset.mime_type.starts_with("audio/")
            || media_type_str.eq_ignore_ascii_case("video")
            || media_type_str.eq_ignore_ascii_case("livephoto");

        catalog.push(MediaCatalogItem {
            id: format!("charon:{}", asset.uuid),
            source: "Charon".to_string(),
            media_type: format!("{:?}", asset.media_type),
            mime_type: asset.mime_type.clone(),
            filename: asset.filename.clone(),
            file_path: asset.resolved_source_path.clone(),
            thumbnail_path: asset.thumbnail_path.clone(),
            file_size: asset.file_size,
            duration_seconds: asset.duration,
            created_date: Some(asset.created_date.to_rfc3339()),
            added_date: Some(asset.added_date.to_rfc3339()),
            phone_number: None,
            contact_name: None,
            direction: None,
            is_transcribed: false,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            sha256: asset.hash.clone(),
        });
    }

    Ok(())
}

fn collect_echo_media(ctx: &AgentCtx, catalog: &mut Vec<MediaCatalogItem>) -> Result<()> {
    let echo_dir = ctx.case.evidence_path("echo");
    if !echo_dir.exists() {
        return Ok(());
    }

    // Load echo records.jsonl or records.json
    let records_path = echo_dir.join("records.json");
    if !records_path.exists() {
        return Ok(());
    }

    let raw = fs::read(&records_path)?;
    let echo_records: Vec<serde_json::Value> = serde_json::from_slice(&raw)?;

    for rec in echo_records {
        let rec_type = rec
            .get("record_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if rec_type != "audio" {
            continue;
        }
        let payload = rec.get("payload").cloned().unwrap_or_default();
        let record_id = payload
            .get("record_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let copied_media = payload.get("copied_media_path").and_then(|v| v.as_str());
        let source_media = payload.get("source_media_path").and_then(|v| v.as_str());
        let file_path = copied_media.or(source_media).map(|s| s.to_string());
        let duration = payload.get("duration_seconds").and_then(|v| v.as_f64());
        let mime = payload
            .get("mime_type")
            .and_then(|v| v.as_str())
            .unwrap_or("audio/unknown")
            .to_string();
        let sha256 = payload
            .get("sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Try to resolve voicemail phone number from voicemail evidence
        let (phone, contact) = if record_id.starts_with("voicemail:") {
            resolve_voicemail_contact(ctx, &record_id)
        } else {
            (None, None)
        };

        catalog.push(MediaCatalogItem {
            id: format!("echo:{}", record_id),
            source: "Echo".to_string(),
            media_type: "Audio".to_string(),
            mime_type: mime,
            filename: file_path
                .as_ref()
                .map(|p| {
                    Path::new(p)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
            file_path,
            thumbnail_path: None,
            file_size: None,
            duration_seconds: duration,
            created_date: rec
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            added_date: rec
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            phone_number: phone,
            contact_name: contact,
            direction: None,
            is_transcribed: false,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            sha256,
        });
    }

    Ok(())
}

fn resolve_voicemail_contact(ctx: &AgentCtx, record_id: &str) -> (Option<String>, Option<String>) {
    let voicemail_dir = ctx.case.evidence_path("voicemail");
    if !voicemail_dir.exists() {
        return (None, None);
    }

    let vm_id_str = record_id.strip_prefix("voicemail:").unwrap_or(record_id);
    let vm_id: i64 = vm_id_str.parse().unwrap_or(-1);
    if vm_id < 0 {
        return (None, None);
    }

    if let Ok(dir_iter) = fs::read_dir(&voicemail_dir) {
        for entry in dir_iter.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read(&path) else { continue };
            let Ok(vms): Result<Vec<VoicemailRecord>, _> = serde_json::from_slice(&raw) else {
                continue;
            };
            for vm in vms {
                if vm.id == vm_id {
                    let phone = vm.phone_number.clone().filter(|s| !s.is_empty());
                    let contact = vm.sender.clone().filter(|s| !s.is_empty());
                    return (phone, contact);
                }
            }
        }
    }
    (None, None)
}

fn collect_cerberus_attachments(ctx: &AgentCtx, catalog: &mut Vec<MediaCatalogItem>) -> Result<()> {
    let attachments_path = ctx.case.evidence_path("cerberus").join("attachments.json");
    if attachments_path.exists() {
        let raw = fs::read(&attachments_path)?;
        let attachments: Vec<Attachment> = serde_json::from_slice(&raw)
            .with_context(|| format!("parsing {}", attachments_path.display()))?;
        for (idx, att) in attachments.iter().enumerate() {
            let mime = att
                .mime_type
                .as_deref()
                .unwrap_or("application/octet-stream");
            if !is_media_mime(mime) {
                continue;
            }

            catalog.push(MediaCatalogItem {
                id: format!("cerberus:attachment:{}", idx),
                source: "Cerberus".to_string(),
                media_type: media_type_from_mime(mime).to_string(),
                mime_type: mime.to_string(),
                filename: att.transfer_name.clone().unwrap_or_default(),
                file_path: att.filename.clone(),
                thumbnail_path: None,
                file_size: None,
                duration_seconds: None,
                created_date: Some(att.date.to_string()),
                added_date: Some(att.date.to_string()),
                phone_number: att.phone_number.clone().filter(|s| !s.is_empty()),
                contact_name: None,
                direction: Some(att.direction.clone()),
                is_transcribed: false,
                transcript_path: None,
                merged_transcript_path: None,
                pdf_report_path: None,
                sha256: None,
            });
        }
        return Ok(());
    }

    collect_cerberus_attachment_records(ctx, catalog)
}

fn collect_cerberus_attachment_records(
    ctx: &AgentCtx,
    catalog: &mut Vec<MediaCatalogItem>,
) -> Result<()> {
    let records_path = ctx.case.evidence_path("cerberus").join("records.json");
    if !records_path.exists() {
        return Ok(());
    }

    let raw =
        fs::read(&records_path).with_context(|| format!("reading {}", records_path.display()))?;
    let records: Vec<serde_json::Value> = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing {}", records_path.display()))?;

    for record in records {
        if record.get("record_type").and_then(|value| value.as_str()) != Some("attachment") {
            continue;
        }
        let mime = record
            .get("mime_type")
            .and_then(|value| value.as_str())
            .unwrap_or("application/octet-stream");
        if !is_media_mime(mime) {
            continue;
        }
        let attachment_id = record
            .get("attachment_id")
            .and_then(|value| value.as_i64())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let file_path = first_string_field(
            &record,
            &["export_path", "source_path", "original_filename"],
        )
        .map(|value| resolve_media_reference(ctx, value));
        let sha256 = record
            .get("sha256")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .or_else(|| file_path.as_deref().and_then(hash_existing_file));

        catalog.push(MediaCatalogItem {
            id: format!("cerberus:attachment:{}", attachment_id),
            source: "Cerberus".to_string(),
            media_type: media_type_from_mime(mime).to_string(),
            mime_type: mime.to_string(),
            filename: record
                .get("transfer_name")
                .or_else(|| record.get("original_filename"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            file_path,
            thumbnail_path: None,
            file_size: record.get("total_bytes").and_then(|value| value.as_u64()),
            duration_seconds: None,
            created_date: record
                .get("created_utc")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            added_date: record
                .get("timestamp")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            phone_number: None,
            contact_name: None,
            direction: None,
            is_transcribed: false,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            sha256,
        });
    }
    Ok(())
}

fn first_string_field<'a>(record: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| record.get(*key).and_then(|value| value.as_str()))
        .find(|value| !value.trim().is_empty())
}

fn resolve_media_reference(ctx: &AgentCtx, value: &str) -> String {
    let candidate = Path::new(value);
    if candidate.exists() {
        return candidate.display().to_string();
    }

    let mut roots = vec![ctx.backup_root.clone()];
    if let Ok(discovered) = ctx.case.discovered_helios_roots() {
        for root in discovered {
            if !roots.iter().any(|existing| existing == &root) {
                roots.push(root);
            }
        }
    }

    for root in roots {
        for candidate in sms_attachment_candidates(&root, value) {
            if candidate.exists() {
                return candidate.display().to_string();
            }
        }
    }

    value.to_string()
}

fn sms_attachment_candidates(backup_root: &Path, value: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(rest) = value.strip_prefix("~/") {
        candidates.push(backup_root.join("HomeDomain").join(rest));
        candidates.push(backup_root.join("MediaDomain").join(rest));
    }
    if let Some(rest) = value.strip_prefix("/var/mobile/") {
        candidates.push(backup_root.join("HomeDomain").join(rest));
        candidates.push(backup_root.join("MediaDomain").join(rest));
    }
    if let Some(rest) = value.strip_prefix("/var/tmp/com.apple.messages/com.apple.MobileSMS/") {
        candidates.push(
            backup_root
                .join("MediaDomain")
                .join("Library/SMS")
                .join(rest),
        );
    }
    candidates.push(backup_root.join(value.trim_start_matches('/')));
    candidates
}

fn hash_existing_file(path: &str) -> Option<String> {
    let path = Path::new(path);
    if path.is_file() {
        crate::common::prepared::compute_sha256(path).ok()
    } else {
        None
    }
}

fn is_media_mime(mime: &str) -> bool {
    mime.starts_with("video/") || mime.starts_with("audio/") || mime.starts_with("image/")
}

fn media_type_from_mime(mime: &str) -> &'static str {
    if mime.starts_with("video/") {
        "Video"
    } else if mime.starts_with("audio/") {
        "Audio"
    } else if mime.starts_with("image/") {
        "Image"
    } else {
        "File"
    }
}

fn collect_intake_media(ctx: &AgentCtx, catalog: &mut Vec<MediaCatalogItem>) -> Result<()> {
    let intake_dir = ctx.case.evidence_path("intake");
    let intake_path = ctx
        .case
        .evidence_path("intake")
        .join("external_evidence.json");
    if !intake_path.exists() {
        return collect_intake_managed_files(&intake_dir, catalog);
    }

    let raw =
        fs::read(&intake_path).with_context(|| format!("reading {}", intake_path.display()))?;
    let records: Vec<IntakeRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing {}", intake_path.display()))?;
    if records.is_empty() {
        return collect_intake_managed_files(&intake_dir, catalog);
    }

    for record in records {
        if !matches!(record.media_type.as_str(), "audio" | "video" | "image") {
            continue;
        }

        let media_type = match record.media_type.as_str() {
            "audio" => "Audio",
            "video" => "Video",
            "image" => "Image",
            _ => "File",
        };

        catalog.push(MediaCatalogItem {
            id: format!("intake:{}", record.record_id),
            source: "Intake".to_string(),
            media_type: media_type.to_string(),
            mime_type: record.mime_guess,
            filename: record.file_name,
            file_path: Some(record.managed_path),
            thumbnail_path: None,
            file_size: Some(record.size_bytes),
            duration_seconds: None,
            created_date: record.modified_utc.clone(),
            added_date: record.modified_utc,
            phone_number: None,
            contact_name: None,
            direction: None,
            is_transcribed: false,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            sha256: Some(record.sha256),
        });
    }

    Ok(())
}

fn collect_intake_managed_files(
    intake_dir: &Path,
    catalog: &mut Vec<MediaCatalogItem>,
) -> Result<()> {
    let files_dir = intake_dir.join("files");
    if !files_dir.exists() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(&files_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        if !is_media_mime(&mime) {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("reading metadata {}", path.display()))?;
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("intake-media")
            .to_string();
        let sha256 = crate::common::prepared::compute_sha256(path).ok();

        catalog.push(MediaCatalogItem {
            id: sha256
                .as_ref()
                .map(|hash| format!("intake:{}", &hash[..16]))
                .unwrap_or_else(|| format!("intake:{}", filename)),
            source: "Intake".to_string(),
            media_type: media_type_from_mime(&mime).to_string(),
            mime_type: mime,
            filename,
            file_path: Some(path.display().to_string()),
            thumbnail_path: None,
            file_size: Some(metadata.len()),
            duration_seconds: None,
            created_date: metadata
                .modified()
                .ok()
                .map(chrono::DateTime::<Utc>::from)
                .map(|value| value.to_rfc3339()),
            added_date: metadata
                .modified()
                .ok()
                .map(chrono::DateTime::<Utc>::from)
                .map(|value| value.to_rfc3339()),
            phone_number: None,
            contact_name: None,
            direction: None,
            is_transcribed: false,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            sha256,
        });
    }

    Ok(())
}

fn dedupe_catalog(catalog: &mut Vec<MediaCatalogItem>) {
    let mut seen = BTreeSet::new();
    catalog.retain(|item| {
        let key = item
            .sha256
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}:{}:{}",
                    item.source,
                    item.file_path.as_deref().unwrap_or_default(),
                    item.filename
                )
            });
        seen.insert(key)
    });
}

fn build_transcription_queue(catalog: &[MediaCatalogItem]) -> Vec<TranscriptionQueueItem> {
    let mut queue = Vec::new();
    for item in catalog {
        if !is_transcribable(item) {
            continue;
        }
        queue.push(TranscriptionQueueItem {
            media_id: item.id.clone(),
            source: item.source.clone(),
            filename: item.filename.clone(),
            file_path: item.file_path.clone(),
            media_type: item.media_type.clone(),
            mime_type: item.mime_type.clone(),
            status: transcription_status(item),
            reason: transcription_reason(item),
            duration_seconds: item.duration_seconds.map(|value| format!("{:.3}", value)),
            phone_number: item.phone_number.clone(),
            contact_name: item.contact_name.clone(),
            sha256: item.sha256.clone(),
        });
    }
    queue.sort_by(|left, right| {
        left.status
            .cmp(&right.status)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.filename.cmp(&right.filename))
    });
    queue
}

fn is_transcribable(item: &MediaCatalogItem) -> bool {
    item.mime_type.starts_with("audio/")
        || item.mime_type.starts_with("video/")
        || item.media_type.eq_ignore_ascii_case("audio")
        || item.media_type.eq_ignore_ascii_case("video")
        || item.media_type.eq_ignore_ascii_case("livephoto")
}

fn transcription_status(item: &MediaCatalogItem) -> String {
    if item.is_transcribed {
        "complete".to_string()
    } else if !item
        .file_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file())
    {
        "missing_source".to_string()
    } else {
        "needs_review".to_string()
    }
}

fn transcription_reason(item: &MediaCatalogItem) -> String {
    if !item
        .file_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file())
    {
        "source media file was not available in resolved evidence paths".to_string()
    } else if item.source == "Intake" {
        "external evidence audio/video; dashboard approval required".to_string()
    } else if item.phone_number.is_some() {
        "message or voicemail media linked to a phone number".to_string()
    } else {
        "audio/video media; dashboard approval required".to_string()
    }
}

fn write_catalog_outputs(
    ctx: &AgentCtx,
    catalog: &[MediaCatalogItem],
    queue: &[TranscriptionQueueItem],
) -> Result<()> {
    fs::create_dir_all(&ctx.evidence_dir)?;
    fs::write(
        ctx.evidence_dir.join("media_catalog.json"),
        serde_json::to_vec_pretty(catalog)?,
    )?;
    fs::write(
        ctx.evidence_dir.join("transcription_queue.json"),
        serde_json::to_vec_pretty(queue)?,
    )?;
    export_media_catalog_csv(catalog, &ctx.evidence_dir.join("media_catalog.csv"))?;
    export_transcription_queue_csv(queue, &ctx.evidence_dir.join("transcription_queue.csv"))?;
    export_media_catalog_html(ctx, catalog, queue, &ctx.evidence_dir.join("index.html"))?;
    Ok(())
}

fn export_media_catalog_csv(catalog: &[MediaCatalogItem], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "ID",
        "Source",
        "Media Type",
        "MIME Type",
        "Filename",
        "File Path",
        "File Size",
        "Duration Seconds",
        "Created Date",
        "Phone Number",
        "Contact Name",
        "Direction",
        "Transcribed",
        "SHA256",
    ])?;
    for item in catalog {
        wtr.write_record([
            item.id.clone(),
            item.source.clone(),
            item.media_type.clone(),
            item.mime_type.clone(),
            item.filename.clone(),
            item.file_path.clone().unwrap_or_default(),
            item.file_size
                .map(|value| value.to_string())
                .unwrap_or_default(),
            item.duration_seconds
                .map(|value| format!("{:.3}", value))
                .unwrap_or_default(),
            item.created_date.clone().unwrap_or_default(),
            item.phone_number.clone().unwrap_or_default(),
            item.contact_name.clone().unwrap_or_default(),
            item.direction.clone().unwrap_or_default(),
            item.is_transcribed.to_string(),
            item.sha256.clone().unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn export_transcription_queue_csv(queue: &[TranscriptionQueueItem], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Media ID",
        "Source",
        "Filename",
        "File Path",
        "Media Type",
        "MIME Type",
        "Status",
        "Reason",
        "Duration Seconds",
        "Phone Number",
        "Contact Name",
        "SHA256",
    ])?;
    for item in queue {
        wtr.write_record([
            item.media_id.clone(),
            item.source.clone(),
            item.filename.clone(),
            item.file_path.clone().unwrap_or_default(),
            item.media_type.clone(),
            item.mime_type.clone(),
            item.status.clone(),
            item.reason.clone(),
            item.duration_seconds.clone().unwrap_or_default(),
            item.phone_number.clone().unwrap_or_default(),
            item.contact_name.clone().unwrap_or_default(),
            item.sha256.clone().unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn export_media_catalog_html(
    ctx: &AgentCtx,
    catalog: &[MediaCatalogItem],
    queue: &[TranscriptionQueueItem],
    path: &Path,
) -> Result<()> {
    let status_rows = breakdown_queue_by_status(queue)
        .into_iter()
        .map(|(status, count)| {
            format!(
                "<tr><td>{}</td><td class=\"num\">{}</td></tr>",
                html_escape(&status),
                count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let source_rows = breakdown_by(catalog, |item| item.source.clone())
        .into_iter()
        .map(|(source, count)| {
            format!(
                "<tr><td>{}</td><td class=\"num\">{}</td></tr>",
                html_escape(&source),
                count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let queue_rows = queue
        .iter()
        .map(|item| {
            format!(
                "<tr><td><span class=\"status {status_class}\">{status}</span></td><td>{source}</td><td>{media_type}</td><td>{filename}</td><td>{reason}</td><td>{path}</td></tr>",
                status_class = html_escape(&item.status.replace('_', "-")),
                status = html_escape(&item.status),
                source = html_escape(&item.source),
                media_type = html_escape(&item.media_type),
                filename = html_escape(&item.filename),
                reason = html_escape(&item.reason),
                path = html_escape(item.file_path.as_deref().unwrap_or("")),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let rows = catalog
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&item.source),
                html_escape(&item.media_type),
                html_escape(&item.filename),
                html_escape(&item.mime_type),
                html_escape(item.file_path.as_deref().unwrap_or("")),
                if is_transcribable(item) { "yes" } else { "no" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let html = format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Hermes Media Catalog - {case}</title>
<style>
body {{ font-family: Arial, sans-serif; margin: 32px; color: #1f2933; background: #f7f9fb; }}
h1 {{ font-size: 22px; margin: 0 0 4px; }}
h2 {{ font-size: 16px; margin: 24px 0 10px; }}
.meta {{ color: #5c6873; margin-bottom: 20px; }}
.summary {{ display: grid; grid-template-columns: repeat(4, minmax(140px, 1fr)); gap: 10px; margin: 18px 0; }}
.metric {{ background: #fff; border: 1px solid #d8dee4; padding: 12px; }}
.metric .label {{ color: #5c6873; font-size: 11px; text-transform: uppercase; }}
.metric .value {{ font-size: 22px; margin-top: 4px; font-weight: 700; }}
.panel {{ background: #fff; border: 1px solid #d8dee4; padding: 14px; margin-bottom: 16px; }}
.split {{ display: grid; grid-template-columns: minmax(220px, 320px) minmax(220px, 320px); gap: 16px; }}
table {{ border-collapse: collapse; width: 100%; font-size: 12px; background: #fff; }}
th, td {{ border: 1px solid #d8dee4; padding: 6px 8px; text-align: left; vertical-align: top; }}
th {{ background: #eef2f6; }}
td {{ overflow-wrap: anywhere; }}
.num {{ text-align: right; font-variant-numeric: tabular-nums; }}
.status {{ display: inline-block; padding: 2px 6px; border-radius: 4px; font-size: 11px; font-weight: 700; }}
.status.needs-review {{ background: #e8f3ff; color: #0b5cad; }}
.status.missing-source {{ background: #fff1d6; color: #8a5600; }}
.status.complete {{ background: #e5f7ed; color: #166534; }}
.status.error {{ background: #fde7e7; color: #991b1b; }}
</style>
</head>
<body>
<h1>Hermes Media Catalog</h1>
<div class="meta">Case: {case} | Media items: {items} | Transcription queue: {queue}</div>
<div class="summary">
  <div class="metric"><div class="label">Media Items</div><div class="value">{items}</div></div>
  <div class="metric"><div class="label">Queue Items</div><div class="value">{queue}</div></div>
  <div class="metric"><div class="label">Needs Review</div><div class="value">{needs_review}</div></div>
  <div class="metric"><div class="label">Missing Source</div><div class="value">{missing_source}</div></div>
</div>
<div class="split">
  <div class="panel">
    <h2>Queue Status</h2>
    <table><thead><tr><th>Status</th><th>Count</th></tr></thead><tbody>{status_rows}</tbody></table>
  </div>
  <div class="panel">
    <h2>Catalog Sources</h2>
    <table><thead><tr><th>Source</th><th>Count</th></tr></thead><tbody>{source_rows}</tbody></table>
  </div>
</div>
<h2>Transcription Queue</h2>
<table>
<thead><tr><th>Status</th><th>Source</th><th>Type</th><th>File</th><th>Reason</th><th>Path</th></tr></thead>
<tbody>
{queue_rows}
</tbody>
</table>
<h2>Media Catalog</h2>
<table>
<thead><tr><th>Source</th><th>Type</th><th>File</th><th>MIME</th><th>Path</th><th>Transcribable</th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
</body>
</html>
"#,
        case = html_escape(ctx.case.name()),
        items = catalog.len(),
        queue = queue.len(),
        needs_review = queue
            .iter()
            .filter(|item| item.status == "needs_review")
            .count(),
        missing_source = queue
            .iter()
            .filter(|item| item.status == "missing_source")
            .count(),
        status_rows = status_rows,
        source_rows = source_rows,
        queue_rows = queue_rows,
        rows = rows
    );
    fs::write(path, html)?;
    Ok(())
}

fn breakdown_queue_by_status(queue: &[TranscriptionQueueItem]) -> BTreeMap<String, usize> {
    let mut breakdown = BTreeMap::new();
    for item in queue {
        *breakdown.entry(item.status.clone()).or_insert(0) += 1;
    }
    breakdown
}

fn breakdown_by<F>(catalog: &[MediaCatalogItem], mut key_fn: F) -> BTreeMap<String, usize>
where
    F: FnMut(&MediaCatalogItem) -> String,
{
    let mut breakdown = BTreeMap::new();
    for item in catalog {
        *breakdown.entry(key_fn(item)).or_insert(0) += 1;
    }
    breakdown
}

// ---------------------------------------------------------------------------
// Transcription decision & execution
// ---------------------------------------------------------------------------

fn should_transcribe(item: &MediaCatalogItem, investigation_phones: &HashSet<String>) -> bool {
    // Only audio/video with actual audio tracks
    let is_av = item.mime_type.starts_with("audio/")
        || item.mime_type.starts_with("video/")
        || item.media_type.eq_ignore_ascii_case("audio")
        || item.media_type.eq_ignore_ascii_case("video")
        || item.media_type.eq_ignore_ascii_case("livephoto");

    if !is_av {
        return false;
    }

    // If it has a phone number, check against investigation contacts
    if let Some(ref phone) = item.phone_number {
        let normalized: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
        for inv in investigation_phones {
            if normalized == *inv || phone.contains(inv) || inv.contains(&normalized) {
                return true;
            }
        }
    }

    false
}

fn transcribe_media_item(ctx: &AgentCtx, item: &mut MediaCatalogItem) -> Result<()> {
    let source_path = item
        .file_path
        .as_ref()
        .and_then(|p| {
            let path = Path::new(p);
            if path.exists() {
                Some(path.to_path_buf())
            } else {
                None
            }
        })
        .or_else(|| {
            // Try to find in evidence directories
            find_media_in_evidence(ctx, &item.filename)
        })
        .context("no resolvable media path for transcription")?;

    let out_dir = ctx
        .evidence_dir
        .join("transcripts")
        .join(&item.id.replace(":", "_"));
    fs::create_dir_all(&out_dir)?;

    let base_name = sanitize_filename(&item.filename);
    let transcript_json = out_dir.join(format!("{}_transcript.json", base_name));
    let _diarization_txt = out_dir.join(format!("{}_diarization.txt", base_name));
    let merged_txt = out_dir.join(format!("{}_merged.txt", base_name));
    let report_pdf = out_dir.join(format!("{}_report.pdf", base_name));

    // Shell out to Python transcription script
    let ion_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/home/ghost/iON"));
    let venv_python = ion_root.join("transcribe-env").join("bin").join("python");
    let script_path = ion_root.join("hermes_transcribe.py");

    let output = Command::new(&venv_python)
        .arg(&script_path)
        .arg(&source_path)
        .arg(&out_dir)
        .arg(&base_name)
        .output()
        .with_context(|| format!("running hermes_transcribe.py on {}", source_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Transcription script failed: {}", stderr);
    }

    item.is_transcribed = true;
    item.transcript_path = Some(transcript_json.to_string_lossy().to_string());
    item.merged_transcript_path = Some(merged_txt.to_string_lossy().to_string());
    item.pdf_report_path = Some(report_pdf.to_string_lossy().to_string());

    Ok(())
}

fn find_media_in_evidence(ctx: &AgentCtx, filename: &str) -> Option<PathBuf> {
    let candidates = [
        ctx.case.evidence_path("voicemail"),
        ctx.case.evidence_path("audio"),
        ctx.case.evidence_path("mms"),
        ctx.case.evidence_path("charon"),
    ];

    for dir in &candidates {
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(dir)
            .max_depth(4)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() {
                if let Some(name) = entry.path().file_name().and_then(|n| n.to_str()) {
                    if name.eq_ignore_ascii_case(filename) {
                        return Some(entry.path().to_path_buf());
                    }
                }
            }
        }
    }
    None
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_end_matches(".mp4")
        .trim_end_matches(".m4a")
        .trim_end_matches(".wav")
        .trim_end_matches(".mp3")
        .trim_end_matches(".3gp")
        .trim_end_matches(".mov")
        .to_string()
}

// ---------------------------------------------------------------------------
// PDF catalog generation
// ---------------------------------------------------------------------------

fn generate_pdf_catalog(
    ctx: &AgentCtx,
    catalog: &[MediaCatalogItem],
    pdf_path: &Path,
) -> Result<()> {
    let html_path = pdf_path.with_extension("html");

    let mut rows = String::new();
    for item in catalog {
        let thumb = if item.media_type.eq_ignore_ascii_case("audio")
            || item.mime_type.starts_with("audio/")
        {
            "🎵".to_string()
        } else if item.media_type.eq_ignore_ascii_case("video")
            || item.mime_type.starts_with("video/")
        {
            "🎬".to_string()
        } else {
            "🖼️".to_string()
        };

        let phone = item.phone_number.as_deref().unwrap_or("—");
        let contact = item.contact_name.as_deref().unwrap_or("—");
        let transcribed = if item.is_transcribed { "✅" } else { "⬜" };
        let duration = item
            .duration_seconds
            .map(|d| format!("{:.1}s", d))
            .unwrap_or_else(|| "—".to_string());

        rows.push_str(&format!(
            r#"<tr>
                <td style="text-align:center;font-size:18px;">{}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
            </tr>"#,
            thumb,
            html_escape(&item.id),
            html_escape(&item.source),
            html_escape(&item.media_type),
            html_escape(&item.filename),
            html_escape(phone),
            html_escape(contact),
            html_escape(&duration),
            transcribed
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
body {{ font-family: "Segoe UI", Roboto, sans-serif; font-size: 10pt; color: #1a1a1a; margin: 0.5in; }}
h1 {{ font-size: 16pt; color: #2c3e50; border-bottom: 2px solid #2c3e50; padding-bottom: 8px; }}
.meta {{ font-size: 9pt; color: #555; margin-bottom: 16px; }}
table {{ width: 100%; border-collapse: collapse; font-size: 9pt; }}
th {{ background: #2c3e50; color: #fff; padding: 6px; text-align: left; }}
td {{ padding: 5px; border-bottom: 1px solid #ddd; vertical-align: middle; }}
tr:nth-child(even) {{ background: #f8f9fa; }}
.footer {{ margin-top: 20px; font-size: 8pt; color: #777; text-align: center; border-top: 1px solid #ccc; padding-top: 8px; }}
</style>
</head>
<body>
<h1>Media Catalog Report</h1>
<div class="meta">
  <strong>Case:</strong> {} &nbsp;|&nbsp;
  <strong>Generated:</strong> {} &nbsp;|&nbsp;
  <strong>Items:</strong> {} &nbsp;|&nbsp;
  <strong>Transcribed:</strong> {}
</div>
<table>
<thead>
<tr><th></th><th>ID</th><th>Source</th><th>Type</th><th>Filename</th><th>Phone</th><th>Contact</th><th>Duration</th><th>Transcribed</th></tr>
</thead>
<tbody>
{}
</tbody>
</table>
<div class="footer">iON Data Security Systems — Hermes Media Catalog</div>
</body>
</html>"#,
        html_escape(ctx.case.name()),
        Utc::now().format("%Y-%m-%d %H:%M UTC"),
        catalog.len(),
        catalog.iter().filter(|i| i.is_transcribed).count(),
        rows
    );

    fs::write(&html_path, html)?;

    let _output = Command::new("libreoffice")
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            pdf_path.parent().unwrap().to_str().unwrap(),
            html_path.to_str().unwrap(),
        ])
        .output()
        .with_context(|| "running libreoffice for PDF conversion")?;

    Ok(())
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, source: &str, mime: &str, media_type: &str) -> MediaCatalogItem {
        MediaCatalogItem {
            id: id.to_string(),
            source: source.to_string(),
            media_type: media_type.to_string(),
            mime_type: mime.to_string(),
            filename: format!("{}.bin", id),
            file_path: Some(format!("/tmp/{}.bin", id)),
            thumbnail_path: None,
            file_size: Some(4),
            duration_seconds: Some(1.25),
            created_date: None,
            added_date: None,
            phone_number: None,
            contact_name: None,
            direction: None,
            is_transcribed: false,
            transcript_path: None,
            merged_transcript_path: None,
            pdf_report_path: None,
            sha256: None,
        }
    }

    #[test]
    fn transcription_queue_includes_audio_and_video_only() {
        let catalog = vec![
            item("audio", "Intake", "audio/mpeg", "Audio"),
            item("video", "Cerberus", "video/mp4", "Video"),
            item("image", "Charon", "image/jpeg", "Photo"),
        ];

        let queue = build_transcription_queue(&catalog);

        assert_eq!(queue.len(), 2);
        assert!(queue.iter().any(|entry| entry.media_id == "audio"));
        assert!(queue.iter().any(|entry| entry.media_id == "video"));
        assert!(!queue.iter().any(|entry| entry.media_id == "image"));
    }

    #[test]
    fn catalog_dedupes_by_sha256() {
        let mut first = item("one", "Intake", "audio/mpeg", "Audio");
        first.sha256 = Some("same".to_string());
        let mut second = item("two", "Cerberus", "audio/mpeg", "Audio");
        second.sha256 = Some("same".to_string());
        let mut catalog = vec![first, second];

        dedupe_catalog(&mut catalog);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, "one");
    }

    #[test]
    fn queue_csv_writes_dashboard_status() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("queue.csv");
        let queue = vec![TranscriptionQueueItem {
            media_id: "intake:1".to_string(),
            source: "Intake".to_string(),
            filename: "recording.m4a".to_string(),
            file_path: Some("/tmp/recording.m4a".to_string()),
            media_type: "Audio".to_string(),
            mime_type: "audio/mp4".to_string(),
            status: "needs_review".to_string(),
            reason: "dashboard approval required".to_string(),
            duration_seconds: None,
            phone_number: None,
            contact_name: None,
            sha256: Some("abc".to_string()),
        }];

        export_transcription_queue_csv(&queue, &path).unwrap();
        let csv = fs::read_to_string(path).unwrap();

        assert!(csv.contains("needs_review"));
        assert!(csv.contains("dashboard approval required"));
    }

    #[test]
    fn transcription_queue_marks_missing_source() {
        let catalog = vec![item("missing", "Cerberus", "video/mp4", "Video")];

        let queue = build_transcription_queue(&catalog);

        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].status, "missing_source");
        assert!(queue[0].reason.contains("not available"));
    }

    #[test]
    fn transcription_runs_update_queue_status() {
        let mut queue = vec![TranscriptionQueueItem {
            media_id: "intake:abc".to_string(),
            source: "Intake".to_string(),
            filename: "bodycam.mp4".to_string(),
            file_path: Some("/tmp/bodycam.mp4".to_string()),
            media_type: "Video".to_string(),
            mime_type: "video/mp4".to_string(),
            status: "needs_review".to_string(),
            reason: "external evidence audio/video; dashboard approval required".to_string(),
            duration_seconds: None,
            phone_number: None,
            contact_name: None,
            sha256: Some("abc".to_string()),
        }];
        let runs = vec![TranscriptionRunRecord {
            media_id: "intake:abc".to_string(),
            source: "Intake".to_string(),
            filename: "bodycam.mp4".to_string(),
            file_path: Some("/tmp/bodycam.mp4".to_string()),
            status: "complete".to_string(),
            started_at: "2026-05-19T00:00:00Z".to_string(),
            completed_at: Some("2026-05-19T00:01:00Z".to_string()),
            transcript_path: Some("transcript.json".to_string()),
            merged_transcript_path: Some("merged.txt".to_string()),
            pdf_report_path: Some("report.pdf".to_string()),
            error: None,
        }];

        apply_transcription_runs_to_queue(&mut queue, &runs);

        assert_eq!(queue[0].status, "complete");
        assert!(queue[0].reason.contains("transcription complete"));
    }

    #[test]
    fn queue_status_breakdown_counts_statuses() {
        let queue = vec![
            TranscriptionQueueItem {
                media_id: "one".to_string(),
                source: "Intake".to_string(),
                filename: "one.mp4".to_string(),
                file_path: Some("/tmp/one.mp4".to_string()),
                media_type: "Video".to_string(),
                mime_type: "video/mp4".to_string(),
                status: "needs_review".to_string(),
                reason: String::new(),
                duration_seconds: None,
                phone_number: None,
                contact_name: None,
                sha256: None,
            },
            TranscriptionQueueItem {
                media_id: "two".to_string(),
                source: "Cerberus".to_string(),
                filename: "two.mp4".to_string(),
                file_path: None,
                media_type: "Video".to_string(),
                mime_type: "video/mp4".to_string(),
                status: "missing_source".to_string(),
                reason: String::new(),
                duration_seconds: None,
                phone_number: None,
                contact_name: None,
                sha256: None,
            },
        ];

        let breakdown = breakdown_queue_by_status(&queue);

        assert_eq!(breakdown.get("needs_review"), Some(&1));
        assert_eq!(breakdown.get("missing_source"), Some(&1));
    }

    #[test]
    fn recognizes_media_mime_classes() {
        assert!(is_media_mime("audio/mp4"));
        assert!(is_media_mime("video/quicktime"));
        assert!(is_media_mime("image/jpeg"));
        assert!(!is_media_mime("application/pdf"));
        assert_eq!(media_type_from_mime("video/mp4"), "Video");
    }
}
