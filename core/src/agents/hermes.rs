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
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agents::{Agent, AgentCtx};
use crate::agents::cerberus_models::{Attachment, VoicemailRecord};
use crate::agents::charon_models::AssetRecord;
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
    pub transcribed_items: usize,
    pub skipped_items: usize,
    pub investigation_contacts: Vec<String>,
    pub transcription_enabled: bool,
}

pub struct HermesAgent;

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

        ctx.emit_progress("Hermes", 4, 4, "Catalog complete");
        ctx.log(&format!("Hermes: {} total media items cataloged", catalog.len()));

        // 3. Optionally transcribe media linked to investigation contacts
        let mut transcribed_count = 0;
        let mut skipped_count = 0;

        if transcription_enabled {
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
                ctx.emit_progress("Hermes", i + 1, total_to_transcribe, &format!("Transcribing {}", item.id));
                match transcribe_media_item(ctx, item) {
                    Ok(_) => {
                        transcribed_count += 1;
                        ctx.log(&format!(
                            "Hermes transcribed: {} -> {}",
                            item.id,
                            item.merged_transcript_path.as_ref().unwrap_or(&"?".to_string())
                        ));
                    }
                    Err(e) => {
                        ctx.log(&format!("Hermes transcription failed for {}: {}", item.id, e));
                        skipped_count += 1;
                    }
                }
            }
        } else {
            skipped_count = catalog.len();
        }

        // 4. Generate catalog JSON
        let catalog_path = ctx.evidence_dir.join("media_catalog.json");
        fs::write(&catalog_path, serde_json::to_string_pretty(&catalog)?)
            .with_context(|| format!("writing {}", catalog_path.display()))?;

        // 5. Generate PDF report
        let pdf_path = ctx.evidence_dir.join("media_catalog.pdf");
        match generate_pdf_catalog(ctx, &catalog, &pdf_path) {
            Ok(_) => ctx.log(&format!("Hermes PDF catalog: {}", pdf_path.display())),
            Err(e) => ctx.log(&format!("Hermes PDF generation failed: {}", e)),
        }

        // 6. Report record
        let report = HermesReport {
            catalog_items: catalog.len(),
            transcribed_items: transcribed_count,
            skipped_items: skipped_count,
            investigation_contacts: investigation_phones.into_iter().collect(),
            transcription_enabled,
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
                timestamp: item.added_date.clone().unwrap_or_else(|| Utc::now().to_rfc3339()),
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

// ---------------------------------------------------------------------------
// Media collection
// ---------------------------------------------------------------------------

fn collect_charon_media(ctx: &AgentCtx, catalog: &mut Vec<MediaCatalogItem>) -> Result<()> {
    let assets_path = ctx.case.evidence_path("charon").join("assets.json");
    if !assets_path.exists() {
        return Ok(());
    }

    let raw = fs::read(&assets_path)
        .with_context(|| format!("reading {}", assets_path.display()))?;
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
        let rec_type = rec.get("record_type").and_then(|v| v.as_str()).unwrap_or("");
        if rec_type != "audio" {
            continue;
        }
        let payload = rec.get("payload").cloned().unwrap_or_default();
        let record_id = payload.get("record_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let copied_media = payload.get("copied_media_path").and_then(|v| v.as_str());
        let source_media = payload.get("source_media_path").and_then(|v| v.as_str());
        let file_path = copied_media.or(source_media).map(|s| s.to_string());
        let duration = payload.get("duration_seconds").and_then(|v| v.as_f64());
        let mime = payload.get("mime_type").and_then(|v| v.as_str()).unwrap_or("audio/unknown").to_string();
        let sha256 = payload.get("sha256").and_then(|v| v.as_str()).map(|s| s.to_string());

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
            filename: file_path.as_ref().map(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_default(),
            file_path,
            thumbnail_path: None,
            file_size: None,
            duration_seconds: duration,
            created_date: rec.get("timestamp").and_then(|v| v.as_str()).map(|s| s.to_string()),
            added_date: rec.get("timestamp").and_then(|v| v.as_str()).map(|s| s.to_string()),
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
            let Ok(vms): Result<Vec<VoicemailRecord>, _> = serde_json::from_slice(&raw) else { continue };
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
    if !attachments_path.exists() {
        return Ok(());
    }

    let raw = fs::read(&attachments_path)?;
    let attachments: Vec<Attachment> = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing {}", attachments_path.display()))?;

    for (idx, att) in attachments.iter().enumerate() {
        let mime = att.mime_type.as_deref().unwrap_or("application/octet-stream");
        let is_media = mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("image/");

        if !is_media {
            continue;
        }

        let media_type = if mime.starts_with("video/") {
            "Video"
        } else if mime.starts_with("audio/") {
            "Audio"
        } else {
            "Image"
        };

        catalog.push(MediaCatalogItem {
            id: format!("cerberus:attachment:{}", idx),
            source: "Cerberus".to_string(),
            media_type: media_type.to_string(),
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

    Ok(())
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
    let source_path = item.file_path.as_ref()
        .and_then(|p| {
            let path = Path::new(p);
            if path.exists() { Some(path.to_path_buf()) } else { None }
        })
        .or_else(|| {
            // Try to find in evidence directories
            find_media_in_evidence(ctx, &item.filename)
        })
        .context("no resolvable media path for transcription")?;

    let out_dir = ctx.evidence_dir.join("transcripts").join(&item.id.replace(":", "_"));
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
        for entry in walkdir::WalkDir::new(dir).max_depth(4).into_iter().flatten() {
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
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
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

fn generate_pdf_catalog(ctx: &AgentCtx, catalog: &[MediaCatalogItem], pdf_path: &Path) -> Result<()> {
    let html_path = pdf_path.with_extension("html");

    let mut rows = String::new();
    for item in catalog {
        let thumb = if item.media_type.eq_ignore_ascii_case("audio") || item.mime_type.starts_with("audio/") {
            "🎵".to_string()
        } else if item.media_type.eq_ignore_ascii_case("video") || item.mime_type.starts_with("video/") {
            "🎬".to_string()
        } else {
            "🖼️".to_string()
        };

        let phone = item.phone_number.as_deref().unwrap_or("—");
        let contact = item.contact_name.as_deref().unwrap_or("—");
        let transcribed = if item.is_transcribed { "✅" } else { "⬜" };
        let duration = item.duration_seconds.map(|d| format!("{:.1}s", d)).unwrap_or_else(|| "—".to_string());

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
