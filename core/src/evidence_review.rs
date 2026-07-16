use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::case::Case;

#[derive(Debug, Serialize)]
struct EvidenceReviewReport {
    case_id: String,
    generated_at: String,
    evidence_dir: String,
    agents: Vec<AgentReview>,
    findings: Vec<ReviewFinding>,
}

#[derive(Debug, Serialize)]
struct AgentReview {
    agent: String,
    path: String,
    record_count: usize,
    record_types: BTreeMap<String, usize>,
    files: Vec<FileReview>,
    findings: Vec<ReviewFinding>,
}

#[derive(Debug, Serialize)]
struct FileReview {
    name: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize, Clone)]
struct ReviewFinding {
    severity: &'static str,
    agent: String,
    item: String,
    message: String,
}

pub fn run(case_name: &str) -> Result<()> {
    let case = Case::new(case_name)?;
    let evidence_dir = case.root_path().join("evidence");
    let review_dir = evidence_dir.join("_review");
    fs::create_dir_all(&review_dir)?;

    let mut agents = Vec::new();
    let mut findings = Vec::new();
    for entry in fs::read_dir(&evidence_dir)
        .with_context(|| format!("reading evidence dir {}", evidence_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let agent = entry.file_name().to_string_lossy().to_string();
        if agent.starts_with('_') {
            continue;
        }
        let review = review_agent(&agent, &entry.path())?;
        findings.extend(review.findings.clone());
        agents.push(review);
    }
    agents.sort_by(|left, right| left.agent.cmp(&right.agent));
    findings.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| left.agent.cmp(&right.agent))
            .then_with(|| left.item.cmp(&right.item))
    });

    let report = EvidenceReviewReport {
        case_id: case_name.to_string(),
        generated_at: Utc::now().to_rfc3339(),
        evidence_dir: evidence_dir.display().to_string(),
        agents,
        findings,
    };

    let json_path = review_dir.join("evidence_review.json");
    let md_path = review_dir.join("evidence_review.md");
    fs::write(&json_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("writing {}", json_path.display()))?;
    fs::write(&md_path, render_markdown(&report))
        .with_context(|| format!("writing {}", md_path.display()))?;

    let high = report
        .findings
        .iter()
        .filter(|finding| finding.severity == "high")
        .count();
    let medium = report
        .findings
        .iter()
        .filter(|finding| finding.severity == "medium")
        .count();
    println!(
        "Evidence review complete: {} agents, {} findings ({} high, {} medium) -> {}",
        report.agents.len(),
        report.findings.len(),
        high,
        medium,
        review_dir.display()
    );
    Ok(())
}

fn review_agent(agent: &str, path: &Path) -> Result<AgentReview> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            files.push(FileReview {
                name: entry.file_name().to_string_lossy().to_string(),
                size_bytes: entry.metadata()?.len(),
            });
        }
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));

    let records_path = path.join("records.json");
    let mut findings = Vec::new();
    let mut record_count = 0usize;
    let mut record_types = BTreeMap::new();
    if records_path.exists() {
        let records: Vec<Value> = serde_json::from_slice(
            &fs::read(&records_path)
                .with_context(|| format!("reading {}", records_path.display()))?,
        )
        .with_context(|| format!("parsing {}", records_path.display()))?;
        record_count = records.len();
        for record in &records {
            let record_type = record
                .get("record_type")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            *record_types.entry(record_type).or_insert(0) += 1;
        }
        findings.extend(review_records(agent, &records));
    } else {
        findings.push(finding(
            "high",
            agent,
            "records.json",
            "Missing standard records.json output",
        ));
    }

    if record_count <= 1 {
        findings.push(finding(
            "high",
            agent,
            "record_count",
            "Agent output is empty or placeholder-sized",
        ));
    }
    if !path.join("summary.json").exists() {
        findings.push(finding(
            "medium",
            agent,
            "summary.json",
            "Missing standard summary.json output",
        ));
    }

    agent_specific_checks(agent, path, &mut findings);

    Ok(AgentReview {
        agent: agent.to_string(),
        path: path.display().to_string(),
        record_count,
        record_types,
        files,
        findings,
    })
}

fn review_records(agent: &str, records: &[Value]) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    let mut null_heavy = 0usize;
    for record in records.iter().take(5000) {
        let Some(object) = record.as_object() else {
            continue;
        };
        let total = object.len().max(1);
        let nulls = object.values().filter(|value| value.is_null()).count();
        if nulls * 100 / total >= 60 {
            null_heavy += 1;
        }
    }
    if null_heavy > 0 {
        findings.push(finding(
            "low",
            agent,
            "null_density",
            &format!("{null_heavy} sampled records are 60%+ null fields"),
        ));
    }
    findings
}

fn agent_specific_checks(agent: &str, path: &Path, findings: &mut Vec<ReviewFinding>) {
    match agent {
        "hermes" => {
            check_json_array_len(agent, path, "media_catalog.json", 1, "high", findings);
            check_json_array_len(
                agent,
                path,
                "transcription_queue.json",
                1,
                "medium",
                findings,
            );
        }
        "obolus" => {
            check_json_array_len(agent, path, "notes.json", 1, "high", findings);
            check_file(agent, path, "notes.csv", "high", findings);
            check_json_array_len(agent, path, "money_index.json", 1, "medium", findings);
        }
        "psyche" => {
            check_json_array_len(agent, path, "contact_dossiers.json", 1, "high", findings);
        }
        "atlas" => {
            check_json_array_len(agent, path, "location_activity.json", 1, "medium", findings);
            if let Ok(coords) = json_array_len(&path.join("coordinates.json")) {
                if coords == 0 {
                    findings.push(finding(
                        "low",
                        agent,
                        "coordinates.json",
                        "No true coordinate records found; document Atlas as app activity only",
                    ));
                }
            }
        }
        "plutus" => {
            check_json_array_len(agent, path, "transactions.json", 1, "high", findings);
        }
        "cerberus" => {
            check_json_array_len(agent, path, "contact_identities.json", 1, "high", findings);
            check_json_array_len(agent, path, "contacts_full.json", 1, "high", findings);
        }
        _ => {}
    }
}

fn check_file(
    agent: &str,
    path: &Path,
    file_name: &str,
    severity: &'static str,
    findings: &mut Vec<ReviewFinding>,
) {
    let file = path.join(file_name);
    if !file.exists() {
        findings.push(finding(
            severity,
            agent,
            file_name,
            &format!("Missing expected {file_name}"),
        ));
    }
}

fn check_json_array_len(
    agent: &str,
    path: &Path,
    file_name: &str,
    minimum: usize,
    severity: &'static str,
    findings: &mut Vec<ReviewFinding>,
) {
    let file = path.join(file_name);
    match json_array_len(&file) {
        Ok(count) if count >= minimum => {}
        Ok(count) => findings.push(finding(
            severity,
            agent,
            file_name,
            &format!("Expected at least {minimum} records, found {count}"),
        )),
        Err(error) => findings.push(finding(
            severity,
            agent,
            file_name,
            &format!("Could not read expected JSON array: {error:#}"),
        )),
    }
}

fn json_array_len(path: &PathBuf) -> Result<usize> {
    let values: Vec<Value> = serde_json::from_slice(&fs::read(path)?)?;
    Ok(values.len())
}

fn finding(severity: &'static str, agent: &str, item: &str, message: &str) -> ReviewFinding {
    ReviewFinding {
        severity,
        agent: agent.to_string(),
        item: item.to_string(),
        message: message.to_string(),
    }
}

fn severity_rank(value: &str) -> u8 {
    match value {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

fn render_markdown(report: &EvidenceReviewReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Evidence Review - {}\n\n", report.case_id));
    out.push_str(&format!("Generated: {}\n\n", report.generated_at));
    out.push_str("## Findings\n\n");
    if report.findings.is_empty() {
        out.push_str("No findings.\n\n");
    } else {
        for finding in &report.findings {
            out.push_str(&format!(
                "- **{}** `{}` `{}`: {}\n",
                finding.severity, finding.agent, finding.item, finding.message
            ));
        }
        out.push('\n');
    }
    out.push_str("## Agent Inventory\n\n");
    for agent in &report.agents {
        out.push_str(&format!(
            "### {}\n\nRecords: {}\n\n",
            agent.agent, agent.record_count
        ));
        if !agent.record_types.is_empty() {
            out.push_str("Record types:\n");
            for (record_type, count) in &agent.record_types {
                out.push_str(&format!("- `{record_type}`: {count}\n"));
            }
            out.push('\n');
        }
        out.push_str("Files:\n");
        for file in &agent.files {
            out.push_str(&format!("- `{}`: {} bytes\n", file.name, file.size_bytes));
        }
        out.push('\n');
    }
    out
}
