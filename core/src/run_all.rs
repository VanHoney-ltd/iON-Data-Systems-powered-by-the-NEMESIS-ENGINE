use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::case::Case;

#[derive(Debug, Clone, Copy)]
struct RunStep {
    slug: &'static str,
    name: &'static str,
    category: &'static str,
    ui_route: &'static str,
}

#[derive(Debug, Serialize)]
struct RunAllReport {
    case_id: String,
    generated_at: String,
    case_root: String,
    evidence_dir: String,
    ui_dir: String,
    status: String,
    agents_total: usize,
    agents_succeeded: usize,
    agents_failed: usize,
    elapsed_ms: u128,
    agents: Vec<RunAgentStatus>,
    review: Option<ReviewSummary>,
    ui_artifacts: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
struct RunAgentStatus {
    slug: String,
    name: String,
    category: String,
    ui_route: String,
    status: String,
    started_at: String,
    finished_at: String,
    elapsed_ms: u128,
    record_count: Option<usize>,
    summary_path: Option<String>,
    records_path: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct UiCaseManifest {
    schema_version: u32,
    case_id: String,
    generated_at: String,
    case_root: String,
    evidence_dir: String,
    review_path: String,
    run_report_path: String,
    agents: Vec<UiAgentEntry>,
    navigation: Vec<UiNavigationEntry>,
    review: Option<ReviewSummary>,
}

#[derive(Debug, Serialize)]
struct UiAgentEntry {
    slug: String,
    name: String,
    category: String,
    ui_route: String,
    status: String,
    record_count: usize,
    record_types: BTreeMap<String, usize>,
    records_path: Option<String>,
    summary_path: Option<String>,
    primary_outputs: Vec<UiOutputFile>,
}

#[derive(Debug, Serialize)]
struct UiNavigationEntry {
    label: String,
    slug: String,
    route: String,
    category: String,
    enabled: bool,
    record_count: usize,
}

#[derive(Debug, Serialize)]
struct UiOutputFile {
    name: String,
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize, Clone)]
struct ReviewSummary {
    path: String,
    findings_total: usize,
    high: usize,
    medium: usize,
    low: usize,
}

const RUN_STEPS: &[RunStep] = &[
    RunStep {
        slug: "cerberus",
        name: "Cerberus",
        category: "Communications",
        ui_route: "/communications",
    },
    RunStep {
        slug: "voicemail",
        name: "Voicemail",
        category: "Communications",
        ui_route: "/voicemail",
    },
    RunStep {
        slug: "intake",
        name: "Intake",
        category: "Case Evidence",
        ui_route: "/intake",
    },
    RunStep {
        slug: "hermes",
        name: "Hermes",
        category: "Media",
        ui_route: "/media",
    },
    RunStep {
        slug: "charon",
        name: "Charon",
        category: "Media",
        ui_route: "/photos",
    },
    RunStep {
        slug: "vigil",
        name: "Vigil",
        category: "Media",
        ui_route: "/video",
    },
    RunStep {
        slug: "echo",
        name: "Echo",
        category: "Media",
        ui_route: "/audio",
    },
    RunStep {
        slug: "aether",
        name: "Aether",
        category: "Location",
        ui_route: "/location/exif",
    },
    RunStep {
        slug: "atlas",
        name: "Atlas",
        category: "Location",
        ui_route: "/location",
    },
    RunStep {
        slug: "nyx",
        name: "Nyx",
        category: "Browser",
        ui_route: "/browser",
    },
    RunStep {
        slug: "obolus",
        name: "Obolus",
        category: "Notes",
        ui_route: "/notes",
    },
    RunStep {
        slug: "plutus",
        name: "Plutus",
        category: "Financial",
        ui_route: "/financial",
    },
    RunStep {
        slug: "orpheus",
        name: "Orpheus",
        category: "Databases",
        ui_route: "/databases",
    },
    RunStep {
        slug: "psyche",
        name: "Psyche",
        category: "Analysis",
        ui_route: "/psyche",
    },
];

pub fn run(case_name: &str) -> Result<()> {
    let started = Instant::now();
    let case = Case::new(case_name)?;
    case.workspace().ensure_layout()?;

    let evidence_dir = case.root_path().join("evidence");
    let ui_dir = evidence_dir.join("_ui");
    fs::create_dir_all(&ui_dir)?;

    println!("Run-all started for case {}", case.name());
    println!("Evidence dir: {}", evidence_dir.display());

    let mut agents = Vec::new();
    for step in RUN_STEPS {
        agents.push(run_step(&case, step));
    }

    crate::evidence_review::run(case.name()).context("running final evidence review")?;
    let review = read_review_summary(&evidence_dir);
    let ui_artifacts = stage_ui(&case, &agents, review.clone())?;

    let succeeded = agents
        .iter()
        .filter(|agent| agent.status == "succeeded")
        .count();
    let failed = agents.len().saturating_sub(succeeded);
    let status = if failed == 0 { "complete" } else { "partial" };

    let report = RunAllReport {
        case_id: case.name().to_string(),
        generated_at: Utc::now().to_rfc3339(),
        case_root: case.root_path().display().to_string(),
        evidence_dir: evidence_dir.display().to_string(),
        ui_dir: ui_dir.display().to_string(),
        status: status.to_string(),
        agents_total: agents.len(),
        agents_succeeded: succeeded,
        agents_failed: failed,
        elapsed_ms: started.elapsed().as_millis(),
        agents,
        review,
        ui_artifacts,
    };

    let report_path = ui_dir.join("run_all_report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("writing {}", report_path.display()))?;

    println!(
        "Run-all {}: {}/{} agents succeeded -> {}",
        status,
        report.agents_succeeded,
        report.agents_total,
        ui_dir.display()
    );
    Ok(())
}

fn run_step(case: &Case, step: &RunStep) -> RunAgentStatus {
    let started_at = Utc::now().to_rfc3339();
    let started = Instant::now();
    println!("→ {} ({})", step.name, step.slug);

    let result = crate::agents::dispatch(step.slug, case.name());
    let finished_at = Utc::now().to_rfc3339();
    let evidence_dir = case.evidence_path(step.slug);
    let summary_path = evidence_dir.join("summary.json");
    let records_path = evidence_dir.join("records.json");
    let record_count = read_record_count(&records_path);

    match result {
        Ok(()) => RunAgentStatus {
            slug: step.slug.to_string(),
            name: step.name.to_string(),
            category: step.category.to_string(),
            ui_route: step.ui_route.to_string(),
            status: "succeeded".to_string(),
            started_at,
            finished_at,
            elapsed_ms: started.elapsed().as_millis(),
            record_count,
            summary_path: summary_path
                .exists()
                .then(|| summary_path.display().to_string()),
            records_path: records_path
                .exists()
                .then(|| records_path.display().to_string()),
            error: None,
        },
        Err(error) => {
            eprintln!("  {} failed: {error:#}", step.name);
            RunAgentStatus {
                slug: step.slug.to_string(),
                name: step.name.to_string(),
                category: step.category.to_string(),
                ui_route: step.ui_route.to_string(),
                status: "failed".to_string(),
                started_at,
                finished_at,
                elapsed_ms: started.elapsed().as_millis(),
                record_count,
                summary_path: summary_path
                    .exists()
                    .then(|| summary_path.display().to_string()),
                records_path: records_path
                    .exists()
                    .then(|| records_path.display().to_string()),
                error: Some(format!("{error:#}")),
            }
        }
    }
}

fn stage_ui(
    case: &Case,
    run_agents: &[RunAgentStatus],
    review: Option<ReviewSummary>,
) -> Result<Vec<String>> {
    let evidence_dir = case.root_path().join("evidence");
    let ui_dir = evidence_dir.join("_ui");
    fs::create_dir_all(&ui_dir)?;

    let mut agents = Vec::new();
    let mut navigation = Vec::new();
    for status in run_agents {
        let agent_dir = evidence_dir.join(&status.slug);
        let records_path = agent_dir.join("records.json");
        let summary_path = agent_dir.join("summary.json");
        let record_types = read_record_types(&records_path);
        let record_count = status.record_count.unwrap_or_default();
        let primary_outputs = list_primary_outputs(&agent_dir)?;

        agents.push(UiAgentEntry {
            slug: status.slug.clone(),
            name: status.name.clone(),
            category: status.category.clone(),
            ui_route: status.ui_route.clone(),
            status: status.status.clone(),
            record_count,
            record_types,
            records_path: records_path
                .exists()
                .then(|| records_path.display().to_string()),
            summary_path: summary_path
                .exists()
                .then(|| summary_path.display().to_string()),
            primary_outputs,
        });
        navigation.push(UiNavigationEntry {
            label: status.name.clone(),
            slug: status.slug.clone(),
            route: status.ui_route.clone(),
            category: status.category.clone(),
            enabled: status.status == "succeeded" && record_count > 0,
            record_count,
        });
    }

    let manifest = UiCaseManifest {
        schema_version: 1,
        case_id: case.name().to_string(),
        generated_at: Utc::now().to_rfc3339(),
        case_root: case.root_path().display().to_string(),
        evidence_dir: evidence_dir.display().to_string(),
        review_path: evidence_dir
            .join("_review")
            .join("evidence_review.json")
            .display()
            .to_string(),
        run_report_path: ui_dir.join("run_all_report.json").display().to_string(),
        agents,
        navigation,
        review,
    };

    let manifest_path = ui_dir.join("case_manifest.json");
    let agent_status_path = ui_dir.join("agent_status.json");
    let navigation_path = ui_dir.join("navigation.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    fs::write(
        &agent_status_path,
        serde_json::to_vec_pretty(&manifest.agents)?,
    )?;
    fs::write(
        &navigation_path,
        serde_json::to_vec_pretty(&manifest.navigation)?,
    )?;

    Ok(vec![
        manifest_path.display().to_string(),
        agent_status_path.display().to_string(),
        navigation_path.display().to_string(),
    ])
}

fn list_primary_outputs(agent_dir: &Path) -> Result<Vec<UiOutputFile>> {
    if !agent_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(agent_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_primary_output(name) {
            continue;
        }
        files.push(UiOutputFile {
            name: name.to_string(),
            path: path.display().to_string(),
            size_bytes: entry.metadata()?.len(),
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

fn is_primary_output(name: &str) -> bool {
    matches!(
        name,
        "records.json"
            | "summary.json"
            | "index.html"
            | "report.json"
            | "media_catalog.json"
            | "transcription_queue.json"
            | "contact_dossiers.json"
            | "notes.json"
            | "transactions.json"
            | "coordinates.json"
            | "location_activity.json"
            | "evidence_review.json"
    )
}

fn read_record_count(path: &Path) -> Option<usize> {
    let bytes = fs::read(path).ok()?;
    let values: Vec<Value> = serde_json::from_slice(&bytes).ok()?;
    Some(values.len())
}

fn read_record_types(path: &Path) -> BTreeMap<String, usize> {
    let Ok(bytes) = fs::read(path) else {
        return BTreeMap::new();
    };
    let Ok(values) = serde_json::from_slice::<Vec<Value>>(&bytes) else {
        return BTreeMap::new();
    };
    let mut counts = BTreeMap::new();
    for value in values {
        let record_type = value
            .get("record_type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        *counts.entry(record_type).or_insert(0) += 1;
    }
    counts
}

fn read_review_summary(evidence_dir: &Path) -> Option<ReviewSummary> {
    let path = evidence_dir.join("_review").join("evidence_review.json");
    let bytes = fs::read(&path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let findings = value.get("findings")?.as_array()?;
    let mut high = 0usize;
    let mut medium = 0usize;
    let mut low = 0usize;
    for finding in findings {
        match finding.get("severity").and_then(|value| value.as_str()) {
            Some("high") => high += 1,
            Some("medium") => medium += 1,
            Some("low") => low += 1,
            _ => {}
        }
    }
    Some(ReviewSummary {
        path: path.display().to_string(),
        findings_total: findings.len(),
        high,
        medium,
        low,
    })
}
