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
