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
