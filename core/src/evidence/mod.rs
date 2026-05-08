//! Unified evidence output pipeline.
//!
//! Every agent produces a Vec<EvidenceRecord> and calls write_evidence().
//! This module handles JSON, CSV, JSONL, summary.json, and report.pdf generation.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

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
///   evidence/<slug>/report.pdf        — human-readable PDF report
pub fn write_evidence(
    evidence_dir: &Path,
    agent_name: &str,
    slug: &str,
    schema_version: u32,
    case_name: &str,
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

    // report.pdf — human-readable PDF (best-effort)
    if let Err(e) = write_pdf_report(evidence_dir, agent_name, case_name, records) {
        log::warn!("PDF report generation failed for {}: {}", slug, e);
    }

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

// ---------------------------------------------------------------------------
// PDF Report Generation
// ---------------------------------------------------------------------------

/// Threshold for splitting into per-type PDFs to avoid memory issues.
const PDF_SPLIT_THRESHOLD: usize = 3000;

fn write_pdf_report(
    evidence_dir: &Path,
    agent_name: &str,
    case_name: &str,
    records: &[EvidenceRecord],
) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }

    // Group records by type
    let mut by_type: HashMap<String, Vec<&EvidenceRecord>> = HashMap::new();
    for record in records {
        by_type.entry(record.record_type.clone()).or_default().push(record);
    }

    if records.len() <= PDF_SPLIT_THRESHOLD {
        // Small dataset — single comprehensive PDF
        let html_path = evidence_dir.join("report.html");
        let pdf_path = evidence_dir.join("report.pdf");
        let html = build_html_report(agent_name, case_name, records, false);
        fs::write(&html_path, html)
            .with_context(|| format!("writing {}", html_path.display()))?;
        convert_html_to_pdf(&html_path, &pdf_path)?;
        let _ = fs::remove_file(&html_path);
    } else {
        // Large dataset — summary PDF + one PDF per record type (capped at 3000 per type)
        let summary_html = evidence_dir.join("report_summary.html");
        let summary_pdf = evidence_dir.join("report_summary.pdf");
        let html = build_html_report(agent_name, case_name, records, true);
        fs::write(&summary_html, html)
            .with_context(|| format!("writing {}", summary_html.display()))?;
        convert_html_to_pdf(&summary_html, &summary_pdf)?;
        let _ = fs::remove_file(&summary_html);

        for (record_type, type_records) in &by_type {
            if type_records.len() > PDF_SPLIT_THRESHOLD {
                // Too large for a table PDF — skip to avoid memory issues
                log::info!(
                    "Skipping full-table PDF for {} {} ({} records exceeds limit)",
                    agent_name,
                    record_type,
                    type_records.len()
                );
                continue;
            }
            let safe_type = record_type.replace(|c: char| !c.is_alphanumeric(), "_");
            let type_html = evidence_dir.join(format!("report_{}.html", safe_type));
            let type_pdf = evidence_dir.join(format!("report_{}.pdf", safe_type));
            let html = build_record_type_full_pdf(agent_name, case_name, record_type, type_records);
            fs::write(&type_html, html)
                .with_context(|| format!("writing {}", type_html.display()))?;
            if let Err(e) = convert_html_to_pdf(&type_html, &type_pdf) {
                log::warn!("PDF conversion failed for {} {}: {}", agent_name, record_type, e);
            }
            let _ = fs::remove_file(&type_html);
        }
    }

    Ok(())
}

fn convert_html_to_pdf(html_path: &Path, pdf_path: &Path) -> Result<()> {
    // Try weasyprint first (better image support), fallback to libreoffice
    if try_weasyprint(html_path, pdf_path) {
        return Ok(());
    }

    let evidence_dir = pdf_path.parent().unwrap();
    let output = Command::new("libreoffice")
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            evidence_dir.to_str().unwrap(),
            html_path.to_str().unwrap(),
        ])
        .output()
        .with_context(|| "running libreoffice for PDF conversion")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("libreoffice PDF conversion failed: {}", stderr);
    }

    let lo_pdf = html_path.with_extension("pdf");
    if lo_pdf.exists() && lo_pdf != pdf_path {
        fs::rename(&lo_pdf, pdf_path)
            .with_context(|| format!("renaming {} to {}", lo_pdf.display(), pdf_path.display()))?;
    }

    Ok(())
}

fn try_weasyprint(html_path: &Path, pdf_path: &Path) -> bool {
    let output = Command::new("weasyprint")
        .args([html_path.to_str().unwrap(), pdf_path.to_str().unwrap()])
        .output();

    match output {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log::warn!("weasyprint failed: {}", stderr);
            false
        }
        Err(e) => {
            log::warn!("weasyprint not available: {}", e);
            false
        }
    }
}

/// Maximum characters for any single cell value.
const PDF_CELL_TRUNCATE: usize = 300;

fn build_html_report(agent_name: &str, case_name: &str, records: &[EvidenceRecord], summary_only: bool) -> String {
    let now = Utc::now().to_rfc3339();

    let mut type_counts: HashMap<String, usize> = HashMap::new();
    for record in records {
        *type_counts.entry(record.record_type.clone()).or_insert(0) += 1;
    }

    let mut sorted_types: Vec<(String, usize)> = type_counts.into_iter().collect();
    sorted_types.sort_by(|a, b| b.1.cmp(&a.1));

    let summary_rows: String = sorted_types
        .iter()
        .map(|(rt, count)| {
            format!(
                "<tr><td style=\"padding:5px 8px;border-bottom:1px solid #e0e0e0;\">{}</td><td style=\"padding:5px 8px;border-bottom:1px solid #e0e0e0;text-align:right;\">{}</td></tr>",
                escape_html(rt),
                count
            )
        })
        .collect();

    let mut sections = String::new();
    if !summary_only {
        for (record_type, total_count) in &sorted_types {
            let type_records: Vec<&EvidenceRecord> = records
                .iter()
                .filter(|r| &r.record_type == record_type)
                .collect();
            sections.push_str(&build_record_type_section(record_type, *total_count, &type_records));
        }
    } else {
        sections.push_str(r#"<div class="note" style="font-size:9pt;color:#444;margin:12px 0;padding:10px;background:#fffbe6;border-left:4px solid #f0a000;">
<strong>Large Dataset:</strong> This agent produced more than 3,000 records. To keep PDFs manageable, each record type has been saved as a separate PDF file in this directory.
</div>"#);
    }

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{agent} — {case}</title>
<style>
  @page {{ size: A4 landscape; margin: 10mm; @bottom-center {{ content: "Page " counter(page) " of " counter(pages); font-size: 7pt; color: #888; }} }}
  body {{ font-family: "Segoe UI", Roboto, Helvetica, Arial, sans-serif; font-size: 8pt; color: #222; line-height: 1.3; margin: 0; padding: 12px; }}
  h1 {{ font-size: 15pt; color: #1a1a2e; margin-bottom: 3px; }}
  h2 {{ font-size: 11pt; color: #16213e; margin-top: 16px; margin-bottom: 5px; border-bottom: 2px solid #0f3460; padding-bottom: 3px; page-break-after: avoid; }}
  h3 {{ font-size: 9pt; color: #0f3460; margin-top: 10px; margin-bottom: 3px; }}
  .meta {{ color: #666; font-size: 8pt; margin-bottom: 10px; }}
  table {{ border-collapse: collapse; width: 100%; margin-bottom: 10px; font-size: 6.5pt; page-break-inside: auto; }}
  tr {{ page-break-inside: avoid; }}
  th {{ background: #0f3460; color: #fff; padding: 4px 5px; text-align: left; font-weight: 600; }}
  td {{ padding: 3px 5px; border-bottom: 1px solid #e8e8e8; vertical-align: top; max-width: 350px; overflow: hidden; text-overflow: ellipsis; }}
  tr:nth-child(even) {{ background: #f8f9fa; }}
  .summary-box {{ background: #f0f4f8; border-left: 4px solid #0f3460; padding: 8px 12px; margin-bottom: 12px; }}
  .badge {{ display: inline-block; background: #0f3460; color: #fff; padding: 1px 5px; border-radius: 2px; font-size: 6.5pt; margin-right: 2px; }}
  .null {{ color: #aaa; font-style: italic; }}
  pre {{ margin: 0; white-space: pre-wrap; word-break: break-word; font-family: "SF Mono", Consolas, monospace; font-size: 5.5pt; }}
</style>
</head>
<body>
  <h1>{agent} Evidence Report</h1>
  <div class="meta">Case: <strong>{case}</strong> &nbsp;|&nbsp; Generated: {now} &nbsp;|&nbsp; Total Records: {total}</div>

  <div class="summary-box">
    <h3>Record Summary</h3>
    <table>
      <thead><tr><th>Record Type</th><th style="text-align:right">Count</th></tr></thead>
      <tbody>{summary_rows}</tbody>
    </table>
  </div>

  {sections}

  <div style="margin-top:20px; padding-top:6px; border-top:1px solid #ccc; font-size:7pt; color:#888; text-align:center;">
    iON Data Security Systems &mdash; Evidence Report
  </div>
</body>
</html>"#,
        agent = escape_html(agent_name),
        case = escape_html(case_name),
        now = escape_html(&now),
        total = records.len(),
        summary_rows = summary_rows,
        sections = sections,
    )
}

fn build_record_type_full_pdf(agent_name: &str, case_name: &str, record_type: &str, records: &[&EvidenceRecord]) -> String {
    let now = Utc::now().to_rfc3339();
    let sections = build_record_type_section(record_type, records.len(), records);

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{agent} — {rt} — {case}</title>
<style>
  @page {{ size: A4 landscape; margin: 10mm; @bottom-center {{ content: "Page " counter(page) " of " counter(pages); font-size: 7pt; color: #888; }} }}
  body {{ font-family: "Segoe UI", Roboto, Helvetica, Arial, sans-serif; font-size: 8pt; color: #222; line-height: 1.3; margin: 0; padding: 12px; }}
  h1 {{ font-size: 15pt; color: #1a1a2e; margin-bottom: 3px; }}
  h2 {{ font-size: 11pt; color: #16213e; margin-top: 16px; margin-bottom: 5px; border-bottom: 2px solid #0f3460; padding-bottom: 3px; page-break-after: avoid; }}
  .meta {{ color: #666; font-size: 8pt; margin-bottom: 10px; }}
  table {{ border-collapse: collapse; width: 100%; margin-bottom: 10px; font-size: 6.5pt; page-break-inside: auto; }}
  tr {{ page-break-inside: avoid; }}
  th {{ background: #0f3460; color: #fff; padding: 4px 5px; text-align: left; font-weight: 600; }}
  td {{ padding: 3px 5px; border-bottom: 1px solid #e8e8e8; vertical-align: top; max-width: 350px; overflow: hidden; text-overflow: ellipsis; }}
  tr:nth-child(even) {{ background: #f8f9fa; }}
  .badge {{ display: inline-block; background: #0f3460; color: #fff; padding: 1px 5px; border-radius: 2px; font-size: 6.5pt; margin-right: 2px; }}
  .null {{ color: #aaa; font-style: italic; }}
  pre {{ margin: 0; white-space: pre-wrap; word-break: break-word; font-family: "SF Mono", Consolas, monospace; font-size: 5.5pt; }}
</style>
</head>
<body>
  <h1>{agent} — {rt}</h1>
  <div class="meta">Case: <strong>{case}</strong> &nbsp;|&nbsp; Generated: {now} &nbsp;|&nbsp; Records: {total}</div>

  {sections}

  <div style="margin-top:20px; padding-top:6px; border-top:1px solid #ccc; font-size:7pt; color:#888; text-align:center;">
    iON Data Security Systems &mdash; Evidence Report
  </div>
</body>
</html>"#,
        agent = escape_html(agent_name),
        rt = escape_html(record_type),
        case = escape_html(case_name),
        now = escape_html(&now),
        total = records.len(),
        sections = sections,
    )
}

fn build_record_type_section(record_type: &str, total_count: usize, records: &[&EvidenceRecord]) -> String {
    if records.is_empty() {
        return String::new();
    }

    // Collect ALL keys across ALL records of this type
    let mut keys: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for record in records {
        if let Some(obj) = record.payload.as_object() {
            for key in obj.keys() {
                if seen.insert(key.clone()) {
                    keys.push(key.clone());
                }
            }
        }
    }
    keys.sort();

    let header: String = keys
        .iter()
        .map(|k| format!("<th>{}</th>", escape_html(k)))
        .collect();

    let rows: String = records
        .iter()
        .map(|record| {
            let cells: String = keys
                .iter()
                .map(|key| {
                    let val = record
                        .payload
                        .get(key)
                        .map(format_json_value_compact)
                        .unwrap_or_else(|| "<span class=\"null\">null</span>".to_string());
                    format!("<td>{}</td>", val)
                })
                .collect();
            format!(
                "<tr><td><span class=\"badge\">{}</span></td>{}</tr>",
                escape_html(&record.timestamp[..record.timestamp.len().min(16)]),
                cells
            )
        })
        .collect();

    format!(
        r#"<h2>{rt} <span style="font-size:9pt;font-weight:normal;color:#666;">({count} records)</span></h2>
<table>
  <thead>
    <tr><th>Timestamp</th>{header}</tr>
  </thead>
  <tbody>
    {rows}
  </tbody>
</table>"#,
        rt = escape_html(record_type),
        count = total_count,
        header = header,
        rows = rows,
    )
}

fn format_json_value_compact(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            let truncated = if s.len() > PDF_CELL_TRUNCATE {
                format!("{}…", &s[..PDF_CELL_TRUNCATE])
            } else {
                s.clone()
            };
            escape_html(&truncated)
        }
        serde_json::Value::Null => "<span class=\"null\">null</span>".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() == 1 {
                format!("[{}]", format_json_value_compact(&arr[0]))
            } else {
                format!("[{} items]", arr.len())
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                format!("{{{} fields}}", obj.len())
            }
        }
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
