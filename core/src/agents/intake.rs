//! Intake - external evidence cataloging for non-phone artifacts.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub const INTAKE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntakeRecord {
    pub record_id: String,
    pub original_path: String,
    pub managed_path: String,
    pub file_name: String,
    pub category: String,
    pub media_type: String,
    pub extension: String,
    pub mime_guess: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub modified_utc: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IntakeReport {
    schema_version: u32,
    case_id: String,
    generated_at: String,
    roots_scanned: Vec<String>,
    files_cataloged: usize,
    added_since_last_run: usize,
    removed_since_last_run: usize,
    category_breakdown: BTreeMap<String, usize>,
    total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IntakeState {
    observed_hashes: BTreeSet<String>,
}

pub struct IntakeAgent;

impl Agent for IntakeAgent {
    const NAME: &'static str = "Intake";
    const SLUG: &'static str = "intake";
    const SCHEMA_VERSION: u32 = INTAKE_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let evidence_dir = ctx.case.evidence_path(Self::SLUG);
        fs::create_dir_all(&evidence_dir)?;

        let roots = intake_roots(ctx);
        let managed_root = evidence_dir.join("files");
        let mut report_roots = roots.clone();
        if managed_root.exists() {
            report_roots.push(managed_root.clone());
        }
        let previous_state = read_state(&evidence_dir)?;
        let records = catalog_external_evidence(&roots, &evidence_dir)?;
        let records = merge_managed_evidence(records, &managed_root)?;
        let current_state = IntakeState {
            observed_hashes: records.iter().map(|record| record.sha256.clone()).collect(),
        };
        let added_since_last_run = current_state
            .observed_hashes
            .difference(&previous_state.observed_hashes)
            .count();
        let removed_since_last_run = previous_state
            .observed_hashes
            .difference(&current_state.observed_hashes)
            .count();

        write_intake_outputs(
            ctx,
            &report_roots,
            &records,
            added_since_last_run,
            removed_since_last_run,
        )?;
        write_state(&evidence_dir, &current_state)?;

        let mut evidence_records = Vec::new();
        let report = build_report(
            ctx,
            &report_roots,
            &records,
            added_since_last_run,
            removed_since_last_run,
        );
        evidence_records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "intake_report".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        });

        for record in records {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "external_evidence".to_string(),
                timestamp: record
                    .modified_utc
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(&record)?,
            });
        }

        Ok(evidence_records)
    }
}

pub fn watch_case(case_name: &str) -> Result<()> {
    let ctx = AgentCtx::new(case_name, IntakeAgent::SLUG, "Intake Watch")?;
    let roots = intake_roots(&ctx);
    let mut last_snapshot = snapshot_roots(&roots)?;

    println!("Intake watch started for case {}", case_name);
    println!("Watching:");
    for root in &roots {
        println!("  {}", root.display());
    }
    println!("Press Ctrl-C to stop.");

    let _ = IntakeAgent::run_with_case(case_name);

    loop {
        thread::sleep(Duration::from_secs(2));
        let snapshot = snapshot_roots(&roots)?;
        if snapshot == last_snapshot {
            continue;
        }

        let added = snapshot.difference(&last_snapshot).count();
        let removed = last_snapshot.difference(&snapshot).count();
        println!(
            "Intake change detected: {} added/changed, {} removed",
            added, removed
        );
        IntakeAgent::run_with_case(case_name)?;
        last_snapshot = snapshot;
    }
}

fn intake_roots(ctx: &AgentCtx) -> Vec<PathBuf> {
    let case_root = ctx.case.root_path();
    let primary = case_root.join("intake");
    let _ = fs::create_dir_all(&primary);
    [
        primary,
        case_root.join("external"),
        case_root.join("evidence").join("external"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

fn snapshot_roots(roots: &[PathBuf]) -> Result<BTreeSet<String>> {
    let mut snapshot = BTreeSet::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if is_hidden_file(path) {
                continue;
            }
            let metadata = entry
                .metadata()
                .with_context(|| format!("reading metadata {}", path.display()))?;
            let modified = metadata
                .modified()
                .ok()
                .map(DateTime::<Utc>::from)
                .map(|value| value.timestamp_nanos_opt().unwrap_or_default())
                .unwrap_or_default();
            snapshot.insert(format!(
                "{}|{}|{}",
                path.display(),
                metadata.len(),
                modified
            ));
        }
    }
    Ok(snapshot)
}

fn catalog_external_evidence(roots: &[PathBuf], evidence_dir: &Path) -> Result<Vec<IntakeRecord>> {
    let managed_root = evidence_dir.join("files");
    fs::create_dir_all(&managed_root)?;

    let mut records = Vec::new();
    let mut seen_hashes = BTreeSet::new();

    for root in roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            if is_hidden_file(path) {
                continue;
            }

            let sha256 = sha256_file(path)?;
            if !seen_hashes.insert(sha256.clone()) {
                continue;
            }

            let metadata = entry
                .metadata()
                .with_context(|| format!("reading metadata {}", path.display()))?;
            let file_name = file_name_for_path(path);
            let category = category_for_path(path);
            let managed_path = copy_managed(path, &managed_root, &category, &sha256, &file_name)?;

            records.push(build_intake_record(
                path,
                &managed_path,
                &metadata,
                file_name,
                category,
                sha256,
            ));
        }
    }

    sort_intake_records(&mut records);
    Ok(records)
}

fn merge_managed_evidence(
    mut records: Vec<IntakeRecord>,
    managed_root: &Path,
) -> Result<Vec<IntakeRecord>> {
    let mut seen_hashes = records
        .iter()
        .map(|record| record.sha256.clone())
        .collect::<BTreeSet<_>>();

    if !managed_root.exists() {
        sort_intake_records(&mut records);
        return Ok(records);
    }

    for entry in WalkDir::new(managed_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if is_hidden_file(path) {
            continue;
        }

        let sha256 = sha256_file(path)?;
        if !seen_hashes.insert(sha256.clone()) {
            continue;
        }

        let metadata = entry
            .metadata()
            .with_context(|| format!("reading metadata {}", path.display()))?;
        let file_name = file_name_for_path(path);
        let category = category_for_path(path);
        records.push(build_intake_record(
            path, path, &metadata, file_name, category, sha256,
        ));
    }

    sort_intake_records(&mut records);
    Ok(records)
}

fn build_intake_record(
    original_path: &Path,
    managed_path: &Path,
    metadata: &fs::Metadata,
    file_name: String,
    category: String,
    sha256: String,
) -> IntakeRecord {
    let extension = original_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let media_type = media_type_for_category(&category).to_string();
    let mime_guess = mime_guess::from_path(original_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let modified_utc = metadata
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .map(|value| value.to_rfc3339());

    IntakeRecord {
        record_id: format!("intake_{}", &sha256[..16]),
        original_path: original_path.display().to_string(),
        managed_path: managed_path.display().to_string(),
        file_name,
        category: category.clone(),
        media_type,
        extension,
        mime_guess,
        size_bytes: metadata.len(),
        sha256,
        modified_utc,
        tags: tags_for_category(&category),
    }
}

fn file_name_for_path(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("external-evidence")
        .to_string()
}

fn category_for_path(path: &Path) -> String {
    classify_extension(
        &path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase(),
    )
    .to_string()
}

fn sort_intake_records(records: &mut [IntakeRecord]) {
    records.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.file_name.cmp(&right.file_name))
            .then_with(|| left.sha256.cmp(&right.sha256))
    });
}

fn write_intake_outputs(
    ctx: &AgentCtx,
    roots: &[PathBuf],
    records: &[IntakeRecord],
    added_since_last_run: usize,
    removed_since_last_run: usize,
) -> Result<()> {
    let evidence_dir = ctx.case.evidence_path(IntakeAgent::SLUG);
    fs::create_dir_all(&evidence_dir)?;

    fs::write(
        evidence_dir.join("external_evidence.json"),
        serde_json::to_vec_pretty(records)?,
    )?;
    export_intake_csv(records, &evidence_dir.join("external_evidence.csv"))?;
    export_intake_html(
        ctx.case.name(),
        records,
        &build_report(
            ctx,
            roots,
            records,
            added_since_last_run,
            removed_since_last_run,
        ),
        &evidence_dir.join("index.html"),
    )?;
    fs::write(
        evidence_dir.join("intake_report.json"),
        serde_json::to_vec_pretty(&build_report(
            ctx,
            roots,
            records,
            added_since_last_run,
            removed_since_last_run,
        ))?,
    )?;

    Ok(())
}

fn export_intake_csv(records: &[IntakeRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Record ID",
        "File Name",
        "Category",
        "Media Type",
        "Extension",
        "MIME Guess",
        "Size Bytes",
        "SHA256",
        "Modified UTC",
        "Original Path",
        "Managed Path",
        "Tags",
    ])?;

    for record in records {
        wtr.write_record([
            record.record_id.clone(),
            record.file_name.clone(),
            record.category.clone(),
            record.media_type.clone(),
            record.extension.clone(),
            record.mime_guess.clone(),
            record.size_bytes.to_string(),
            record.sha256.clone(),
            record.modified_utc.clone().unwrap_or_default(),
            record.original_path.clone(),
            record.managed_path.clone(),
            record.tags.join(";"),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn export_intake_html(
    case_name: &str,
    records: &[IntakeRecord],
    report: &IntakeReport,
    path: &Path,
) -> Result<()> {
    let rows = records
        .iter()
        .map(|record| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&record.category),
                escape_html(&record.file_name),
                escape_html(&record.mime_guess),
                record.size_bytes,
                escape_html(&record.sha256),
                escape_html(&record.managed_path)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let roots = report
        .roots_scanned
        .iter()
        .map(|root| format!("<li>{}</li>", escape_html(root)))
        .collect::<Vec<_>>()
        .join("\n");
    let html = format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Intake - {case}</title>
<style>
body {{ font-family: Arial, sans-serif; margin: 32px; color: #1f2933; }}
h1 {{ font-size: 22px; margin-bottom: 4px; }}
.meta {{ color: #5c6873; margin-bottom: 18px; }}
table {{ border-collapse: collapse; width: 100%; font-size: 12px; }}
th, td {{ border: 1px solid #d8dee4; padding: 6px 8px; text-align: left; vertical-align: top; }}
th {{ background: #eef2f6; }}
td:nth-child(5), td:nth-child(6) {{ overflow-wrap: anywhere; }}
@media print {{ body {{ margin: 16px; }} }}
</style>
</head>
<body>
<h1>External Evidence Intake</h1>
<div class="meta">Case: {case} | Files: {count} | Bytes: {bytes}</div>
<ul>{roots}</ul>
<table>
<thead><tr><th>Category</th><th>File</th><th>MIME</th><th>Bytes</th><th>SHA256</th><th>Managed Copy</th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
</body>
</html>
"#,
        case = escape_html(case_name),
        count = records.len(),
        bytes = report.total_bytes,
        roots = roots,
        rows = rows
    );
    fs::write(path, html)?;
    Ok(())
}

fn build_report(
    ctx: &AgentCtx,
    roots: &[PathBuf],
    records: &[IntakeRecord],
    added_since_last_run: usize,
    removed_since_last_run: usize,
) -> IntakeReport {
    let mut category_breakdown = BTreeMap::new();
    let mut total_bytes = 0;

    for record in records {
        *category_breakdown
            .entry(record.category.clone())
            .or_insert(0) += 1;
        total_bytes += record.size_bytes;
    }

    IntakeReport {
        schema_version: INTAKE_SCHEMA_VERSION,
        case_id: ctx.case.name().to_string(),
        generated_at: Utc::now().to_rfc3339(),
        roots_scanned: roots
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        files_cataloged: records.len(),
        added_since_last_run,
        removed_since_last_run,
        category_breakdown,
        total_bytes,
    }
}

fn read_state(evidence_dir: &Path) -> Result<IntakeState> {
    let path = evidence_dir.join("watch_state.json");
    if !path.exists() {
        return Ok(IntakeState::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn write_state(evidence_dir: &Path, state: &IntakeState) -> Result<()> {
    fs::write(
        evidence_dir.join("watch_state.json"),
        serde_json::to_vec_pretty(state)?,
    )?;
    Ok(())
}

fn copy_managed(
    source: &Path,
    managed_root: &Path,
    category: &str,
    sha256: &str,
    file_name: &str,
) -> Result<PathBuf> {
    let safe_name = sanitize_file_name(file_name);
    let dest_dir = managed_root.join(category);
    fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(format!("{}_{}", &sha256[..16], safe_name));
    fs::copy(source, &dest)
        .with_context(|| format!("copying {} to {}", source.display(), dest.display()))?;
    Ok(dest)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn classify_extension(extension: &str) -> &'static str {
    match extension {
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" => "documents",
        "xls" | "xlsx" | "csv" | "tsv" | "ods" => "spreadsheets",
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "webp" | "tif" | "tiff" | "bmp" => "images",
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "3gp" => "video",
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "amr" => "audio",
        "zip" | "7z" | "rar" | "tar" | "gz" => "archives",
        "json" | "xml" | "plist" | "sqlite" | "db" => "data",
        _ => "other",
    }
}

fn media_type_for_category(category: &str) -> &'static str {
    match category {
        "images" => "image",
        "video" => "video",
        "audio" => "audio",
        "documents" | "spreadsheets" => "document",
        "archives" => "archive",
        "data" => "data",
        _ => "file",
    }
}

fn tags_for_category(category: &str) -> Vec<String> {
    match category {
        "documents" => vec!["external".into(), "document".into(), "printable".into()],
        "spreadsheets" => vec!["external".into(), "spreadsheet".into(), "report".into()],
        "images" => vec!["external".into(), "image".into(), "media".into()],
        "video" => vec!["external".into(), "video".into(), "media".into()],
        "audio" => vec!["external".into(), "audio".into(), "recording".into()],
        "archives" => vec!["external".into(), "archive".into()],
        "data" => vec!["external".into(), "data".into()],
        _ => vec!["external".into(), "file".into()],
    }
}

fn sanitize_file_name(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if cleaned.is_empty() {
        "evidence.bin".to_string()
    } else {
        cleaned
    }
}

fn is_hidden_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_expected_external_evidence_types() {
        assert_eq!(classify_extension("pdf"), "documents");
        assert_eq!(classify_extension("xlsx"), "spreadsheets");
        assert_eq!(classify_extension("mp4"), "video");
        assert_eq!(classify_extension("m4a"), "audio");
        assert_eq!(classify_extension("heic"), "images");
    }

    #[test]
    fn catalogs_and_copies_external_evidence_once_by_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("external");
        let evidence_dir = tmp.path().join("evidence").join("intake");
        fs::create_dir_all(root.join("reports")).unwrap();
        fs::write(root.join("reports").join("bank report.xlsx"), b"same").unwrap();
        fs::write(root.join("reports").join("copy.xlsx"), b"same").unwrap();
        fs::write(root.join("camera.mp4"), b"video").unwrap();

        let records = catalog_external_evidence(&[root], &evidence_dir).unwrap();

        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .any(|record| record.category == "spreadsheets"));
        assert!(records.iter().any(|record| record.category == "video"));
        for record in records {
            assert!(Path::new(&record.managed_path).exists());
            assert!(record.tags.iter().any(|tag| tag == "external"));
        }
    }

    #[test]
    fn state_diff_detects_added_and_removed_files() {
        let previous = IntakeState {
            observed_hashes: ["old".to_string(), "keep".to_string()]
                .into_iter()
                .collect(),
        };
        let current = IntakeState {
            observed_hashes: ["new".to_string(), "keep".to_string()]
                .into_iter()
                .collect(),
        };

        assert_eq!(
            current
                .observed_hashes
                .difference(&previous.observed_hashes)
                .count(),
            1
        );
        assert_eq!(
            previous
                .observed_hashes
                .difference(&current.observed_hashes)
                .count(),
            1
        );
    }

    #[test]
    fn snapshot_changes_when_file_is_added() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("intake");
        fs::create_dir_all(&root).unwrap();

        let before = snapshot_roots(&[root.clone()]).unwrap();
        fs::write(root.join("report.pdf"), b"pdf").unwrap();
        let after = snapshot_roots(&[root]).unwrap();

        assert!(before.is_empty());
        assert_eq!(after.len(), 1);
    }
}
