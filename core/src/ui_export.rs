//! MINiOS unified evidence export.
//!
//! Reads evidence records from each agent's evidence directory and writes a
//! consolidated JSON export plus copies per-agent CSV files when present.

use anyhow::{Context, Result};
use ion::case::Case;
use std::env;

fn main() {
    if let Err(error) = run() {
        eprintln!("Export error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        anyhow::bail!("No case specified");
    }

    let case = Case::new(&args[1])?;
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
            let agent = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");
            println!("Found {} records from {}", records.len(), agent);
            all_records.extend(records);
        }
    }

    let output_dir = case.root_path().join("output").join("export");
    std::fs::create_dir_all(&output_dir)?;

    let consolidated_path = output_dir.join("consolidated_evidence.json");
    std::fs::write(&consolidated_path, serde_json::to_vec_pretty(&all_records)?)?;
    println!(
        "Exported {} total records to {}",
        all_records.len(),
        consolidated_path.display()
    );

    if let Ok(entries) = std::fs::read_dir(&evidence_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let csv_src = path.join("records.csv");
            if !csv_src.exists() {
                continue;
            }
            let agent = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");
            let csv_dst = output_dir.join(format!("{agent}_evidence.csv"));
            std::fs::copy(&csv_src, &csv_dst)?;
            println!("  CSV: {}", csv_dst.display());
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!(
        r#"
MINiOS Evidence Export

USAGE:
    minios-export <case_name>

Exports consolidated evidence from all agent runs to:
  output/export/consolidated_evidence.json
  output/export/<agent>_evidence.csv
"#
    );
}
