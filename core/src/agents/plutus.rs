//! Plutus — Deterministic financial artifact extraction.

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use regex::Regex;
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};

use crate::common::prepared::{prepare_artifact, PrepareContext, PreparedArtifact};
use crate::common::resolver::{
    write_resolver_audit, ArtifactResolver, BackupResolver, ResolveMethod, ResolvedPath,
    ResolverAuditRecord,
};
use crate::evidence::EvidenceRecord;

pub const PLUTUS_FINANCIAL_SCHEMA_VERSION: u32 = 1;

const SOURCE_AGENT: &str = "plutus";
const CASH_APP_DOMAIN: &str = "AppDomain-com.squareup.cash";
const CASH_APP_DOCUMENTS: &str = "Documents";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FinancialRecord {
    pub schema_version: u32,
    pub record_id: String,
    pub source_agent: String,
    pub source_path: Option<std::path::PathBuf>,
    pub app: Option<String>,
    pub account_label: Option<String>,
    pub observed_date: Option<String>,
    pub timestamp_utc: Option<String>,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub counterparty: Option<String>,
    pub memo: Option<String>,
    pub transaction_type: Option<String>,
    pub raw_reference: Option<String>,
    pub sha256: Option<String>,
    pub fee_amount: Option<f64>,
    pub statement_period: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FinancialSourceRecord {
    pub label: String,
    pub app: String,
    pub source_type: String,
    pub path: String,
    pub size_bytes: u64,
    pub parsed: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CashAppArtifactRecord {
    pub record_id: String,
    pub source_path: String,
    pub source_kind: String,
    pub app: String,
    pub table_name: Option<String>,
    pub row_index: Option<usize>,
    pub file_size: u64,
    pub keyword_hits: Vec<String>,
    pub request_like: bool,
    pub payment_like: bool,
    pub preview: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CashAppActivityRecord {
    pub record_id: String,
    pub source_path: String,
    pub message_uuid: String,
    pub recorded_at_ms: i64,
    pub recorded_at_utc: Option<String>,
    pub entity_id: Option<String>,
    pub counterparty_token: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_cashtag: Option<String>,
    pub amount: Option<f64>,
    pub amount_text: Option<String>,
    pub payment_role: Option<String>,
    pub payment_state: Option<String>,
    pub payment_orientation: Option<String>,
    pub activity_item_type: Option<String>,
    pub is_outstanding: Option<String>,
    pub is_recurring: Option<String>,
    pub row_index: Option<String>,
    pub origin: Option<String>,
    pub activity_flow_token: Option<String>,
    pub raw_payload: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CashAppIndexedTransactionRecord {
    pub record_id: String,
    pub source_path: String,
    pub entity_id: String,
    pub counterparty_token: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_cashtag: Option<String>,
    pub entity_type: Option<i64>,
    pub observed_at_utc: Option<String>,
    pub amount: Option<f64>,
    pub amount_text: Option<String>,
    pub direction_hint: Option<String>,
    pub display_text: String,
}

#[derive(Debug, Clone, Default)]
struct CashAppPerson {
    name: Option<String>,
    cashtag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlutusReport {
    pub schema_version: u32,
    pub case_id: String,
    pub generated_at: String,
    pub state: String,
    pub files_examined: usize,
    pub files_parsed: usize,
    pub records_emitted: usize,
    pub money_requests_emitted: usize,
    pub cash_app_artifacts_found: usize,
    pub cash_app_request_artifacts_found: usize,
    pub cash_app_activity_events_found: usize,
    pub cash_app_indexed_transactions_found: usize,
    pub financial_sources_found: usize,
    pub financial_source_breakdown: std::collections::BTreeMap<String, usize>,
    pub unsupported_sources_skipped: usize,
    pub supported_source_breakdown: std::collections::BTreeMap<String, usize>,
    pub unsupported_source_breakdown: std::collections::BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct StatementContext {
    statement_period: Option<String>,
    statement_year: Option<i32>,
    account_label: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct StatementTotals {
    money_in: f64,
    money_out: f64,
    fees: f64,
}

#[derive(Debug)]
struct StatementParseResult {
    records: Vec<FinancialRecord>,
    warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct UnsupportedCandidate {
    label: &'static str,
    domain: &'static str,
    relative_path: &'static str,
}

pub struct PlutusAgent;

impl Agent for PlutusAgent {
    const NAME: &'static str = "Plutus";
    const SLUG: &'static str = "plutus";
    const SCHEMA_VERSION: u32 = PLUTUS_FINANCIAL_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let backup_root = if ctx.backup_root.exists() {
            ctx.backup_root.clone()
        } else {
            ctx.case.backup_path()
        };

        let manifest_db_path = backup_root
            .join("Manifest.db")
            .exists()
            .then(|| backup_root.join("Manifest.db"));

        let resolver = BackupResolver::new(crate::common::resolver::ResolverContext {
            backup_root: backup_root.clone(),
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            manifest_db_path,
        });

        let evidence_dir = ctx.case.evidence_path("plutus");
        fs::create_dir_all(&evidence_dir)?;

        let prepare_ctx = PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean").join("plutus"),
            temp_root: ctx.case.root_path().join("temp").join("plutus"),
        };

        let financial_sources = discover_financial_sources(&backup_root)?;
        export_financial_sources_json(
            &financial_sources,
            &evidence_dir.join("financial_sources.json"),
        )?;
        export_financial_sources_csv(
            &financial_sources,
            &evidence_dir.join("financial_sources.csv"),
        )?;
        export_financial_sources_html(
            ctx.case.name(),
            &financial_sources,
            &evidence_dir.join("financial_sources.html"),
        )?;

        println!("\n💸 Hunting Cash App payment/request payloads...");
        let cash_app_artifacts = discover_cash_app_artifacts(&backup_root)?;
        let cash_app_request_artifacts: Vec<CashAppArtifactRecord> = cash_app_artifacts
            .iter()
            .filter(|artifact| is_likely_money_request_artifact(artifact))
            .cloned()
            .collect();
        export_cash_app_artifacts_json(
            &cash_app_artifacts,
            &evidence_dir.join("cash_app_artifacts.json"),
        )?;
        export_cash_app_artifacts_csv(
            &cash_app_artifacts,
            &evidence_dir.join("cash_app_artifacts.csv"),
        )?;
        export_cash_app_artifacts_json(
            &cash_app_request_artifacts,
            &evidence_dir.join("money_request_artifacts.json"),
        )?;
        export_cash_app_artifacts_csv(
            &cash_app_request_artifacts,
            &evidence_dir.join("money_request_artifacts.csv"),
        )?;
        let cash_app_activity = discover_cash_app_activity(&backup_root)?;
        let cash_app_indexed_transactions = discover_cash_app_indexed_transactions(&backup_root)?;
        export_cash_app_activity_json(
            &cash_app_activity,
            &evidence_dir.join("cash_app_activity_history.json"),
        )?;
        export_cash_app_activity_csv(
            &cash_app_activity,
            &evidence_dir.join("cash_app_activity_history.csv"),
        )?;
        export_cash_app_indexed_transactions_json(
            &cash_app_indexed_transactions,
            &evidence_dir.join("cash_app_indexed_transactions.json"),
        )?;
        export_cash_app_indexed_transactions_csv(
            &cash_app_indexed_transactions,
            &evidence_dir.join("cash_app_indexed_transactions.csv"),
        )?;

        let statements = discover_cash_app_statements(&resolver, &prepare_ctx)?;
        let mut supported_breakdown = BTreeMap::new();
        supported_breakdown.insert("cash_app_statement_pdf".to_string(), statements.len());
        let financial_source_breakdown = source_breakdown(&financial_sources);

        let mut records = Vec::new();
        let mut warnings = Vec::new();
        let mut files_parsed = 0usize;

        for statement in &statements {
            let parsed = parse_cash_app_statement(statement)?;
            files_parsed += 1;
            records.extend(parsed.records);
            warnings.extend(parsed.warnings);
        }

        // SQLite extraction from payment apps
        let mut sqlite_records = Vec::new();
        let mut sqlite_files_examined = 0usize;
        let mut sqlite_files_parsed = 0usize;

        println!("\n💳 Scanning payment app SQLite databases...");
        extract_payment_app_sqlite(
            &backup_root,
            &mut sqlite_records,
            &mut sqlite_files_examined,
            &mut sqlite_files_parsed,
            &mut warnings,
        );

        if sqlite_files_parsed > 0 {
            supported_breakdown.insert("payment_app_sqlite".to_string(), sqlite_files_parsed);
        }
        records.extend(sqlite_records);

        records.sort_by(|left, right| {
            right
                .observed_date
                .cmp(&left.observed_date)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });

        let transactions_json_path = evidence_dir.join("transactions.json");
        let transactions_csv_path = evidence_dir.join("transactions.csv");
        let report_path = evidence_dir.join("report.json");
        let money_requests: Vec<FinancialRecord> = records
            .iter()
            .filter(|record| is_money_request_record(record))
            .cloned()
            .collect();

        export_transactions_json(&records, &transactions_json_path)?;
        export_transactions_csv(&records, &transactions_csv_path)?;
        export_transactions_json(&money_requests, &evidence_dir.join("money_requests.json"))?;
        export_transactions_csv(&money_requests, &evidence_dir.join("money_requests.csv"))?;

        // Export records.jsonl for UI integration
        let records_jsonl_path = evidence_dir.join("records.jsonl");
        let mut jsonl_file = fs::File::create(&records_jsonl_path)?;
        for record in &records {
            use std::io::Write;
            writeln!(jsonl_file, "{}", serde_json::to_string(record)?)?;
        }

        // Export summary.json for UI metrics
        let summary_json_path = evidence_dir.join("summary.json");
        let total_money_in: f64 = records.iter().filter_map(|r| r.amount).sum();
        let total_money_out: f64 = records.iter().filter_map(|r| r.amount).map(|a| -a).sum();
        let total_fees: f64 = records.iter().filter_map(|r| r.fee_amount).sum();
        let apps_found: Vec<String> = records
            .iter()
            .filter_map(|r| r.app.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        let summary = serde_json::json!({
            "metrics": [
                {"key": "records", "value": records.len().to_string()},
                {"key": "total_money_in", "value": format!("${:.2}", total_money_in)},
                {"key": "total_money_out", "value": format!("${:.2}", total_money_out)},
                {"key": "total_fees", "value": format!("${:.2}", total_fees)},
                {"key": "apps", "value": apps_found.join(", ")},
                {"key": "financial_sources", "value": financial_sources.len().to_string()},
                {"key": "money_requests", "value": money_requests.len().to_string()},
                {"key": "cash_app_artifacts", "value": cash_app_artifacts.len().to_string()},
                {"key": "cash_app_request_artifacts", "value": cash_app_request_artifacts.len().to_string()},
                {"key": "cash_app_activity_events", "value": cash_app_activity.len().to_string()},
                {"key": "cash_app_indexed_transactions", "value": cash_app_indexed_transactions.len().to_string()},
                {"key": "files_examined", "value": (statements.len() + sqlite_files_examined).to_string()},
                {"key": "files_parsed", "value": (files_parsed + sqlite_files_parsed).to_string()},
            ]
        });
        fs::write(&summary_json_path, serde_json::to_string_pretty(&summary)?)?;

        let total_files_examined = statements.len() + sqlite_files_examined;
        let total_files_parsed = files_parsed + sqlite_files_parsed;

        let report = PlutusReport {
            schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
            case_id: ctx.case.name().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            state: "complete".to_string(),
            files_examined: total_files_examined,
            files_parsed: total_files_parsed,
            records_emitted: records.len(),
            money_requests_emitted: money_requests.len(),
            cash_app_artifacts_found: cash_app_artifacts.len(),
            cash_app_request_artifacts_found: cash_app_request_artifacts.len(),
            cash_app_activity_events_found: cash_app_activity.len(),
            cash_app_indexed_transactions_found: cash_app_indexed_transactions.len(),
            financial_sources_found: financial_sources.len(),
            financial_source_breakdown,
            unsupported_sources_skipped: 0,
            supported_source_breakdown: supported_breakdown,
            unsupported_source_breakdown: BTreeMap::new(),
            warnings: warnings.clone(),
        };
        fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;

        println!("✓ Files examined: {}", report.files_examined);
        println!("✓ Files parsed: {}", report.files_parsed);
        println!("✓ Records emitted: {}", report.records_emitted);
        println!("✓ Output: {}", evidence_dir.display());

        // Build EvidenceRecords
        let mut evidence_records = Vec::new();

        // Report record
        evidence_records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: plutus_payload(&report)?,
        });

        for source in financial_sources {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "financial_source".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: plutus_payload(&source)?,
            });
        }

        for artifact in cash_app_artifacts {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "cash_app_artifact".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: plutus_payload(&artifact)?,
            });
        }

        for activity in cash_app_activity {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "cash_app_activity".to_string(),
                timestamp: activity
                    .recorded_at_utc
                    .clone()
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                payload: plutus_payload(&activity)?,
            });
        }

        for indexed in cash_app_indexed_transactions {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "cash_app_indexed_transaction".to_string(),
                timestamp: indexed
                    .observed_at_utc
                    .clone()
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                payload: plutus_payload(&indexed)?,
            });
        }

        // Transaction records
        for record in records {
            let timestamp = record
                .observed_date
                .clone()
                .or_else(|| record.timestamp_utc.clone())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "transaction".to_string(),
                timestamp,
                payload: plutus_payload(&record)?,
            });
        }

        Ok(evidence_records)
    }
}

fn plutus_payload<T: Serialize>(value: &T) -> Result<serde_json::Value> {
    Ok(strip_nulls(serde_json::to_value(value)?))
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

fn discover_cash_app_statements(
    resolver: &BackupResolver,
    prepare_ctx: &PrepareContext,
) -> Result<Vec<PreparedArtifact>> {
    let documents_root = resolver.direct_tree_path(CASH_APP_DOMAIN, CASH_APP_DOCUMENTS);
    let mut discovered = Vec::new();
    let mut unique = BTreeSet::new();

    if !documents_root.exists() {
        return Ok(Vec::new());
    }

    let backup_domain_root = resolver.backup_root().join(CASH_APP_DOMAIN);
    for entry in WalkDir::new(&documents_root)
        .max_depth(3)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_cash_app_statement_name(file_name) {
            continue;
        }
        if !unique.insert(path.to_path_buf()) {
            continue;
        }

        let relative_path = path
            .strip_prefix(&backup_domain_root)
            .with_context(|| format!("Failed to compute relative path for {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let resolved = ResolvedPath {
            artifact_key: "cash_app_statement".to_string(),
            source_path: path.to_path_buf(),
            method: ResolveMethod::DirectTree,
            domain: CASH_APP_DOMAIN.to_string(),
            relative_path,
            clean_file_name: None,
            attempted: vec![path.to_path_buf()],
        };
        discovered.push(prepare_artifact(prepare_ctx, &resolved, false)?);
    }

    discovered.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    write_resolver_audit(
        resolver.case_root(),
        &ResolverAuditRecord {
            artifact_key: "plutus_cash_app_statements".to_string(),
            candidates: discovered
                .iter()
                .map(|artifact| artifact.source_path.display().to_string())
                .collect(),
            chosen: None,
            method: Some("multi_direct_tree_scan".to_string()),
        },
    )?;
    Ok(discovered)
}

#[allow(dead_code)]
fn discover_unsupported_candidates(resolver: &BackupResolver) -> Result<Vec<UnsupportedCandidate>> {
    let mut discovered = Vec::new();
    for candidate in unsupported_candidates() {
        if resolver
            .resolve_exact(
                "plutus_unsupported",
                candidate.domain,
                candidate.relative_path,
            )?
            .is_some()
        {
            discovered.push(*candidate);
        }
    }
    Ok(discovered)
}

#[allow(dead_code)]
fn unsupported_candidates() -> &'static [UnsupportedCandidate] {
    &[
        UnsupportedCandidate {
            label: "venmo_model_sqlite",
            domain: "AppDomain-net.kortina.labs.Venmo",
            relative_path: "Documents/Model.sqlite",
        },
        UnsupportedCandidate {
            label: "venmo_siri_support_sqlite",
            domain: "AppDomainGroup-group.net.kortina.labs.Venmo",
            relative_path: "SiriSupportData.sqlite",
        },
        UnsupportedCandidate {
            label: "chime_visa_analytics_sqlite",
            domain: "AppDomain-com.1debit.ChimeProdApp",
            relative_path: "Documents/VisaAnalytics_v2.sqlite",
        },
        UnsupportedCandidate {
            label: "cash_app_search_index_sqlite",
            domain: "AppDomainGroup-group.com.squareup.cash",
            relative_path: "SearchIndex-internal.cashappapi.com.sqlite",
        },
        UnsupportedCandidate {
            label: "cash_app_analytics_messages_sqlite",
            domain: "AppDomainGroup-group.com.squareup.cash",
            relative_path: "CDPPersistedAnalyticsMessages.SQLITE",
        },
    ]
}

fn discover_cash_app_activity(root: &Path) -> Result<Vec<CashAppActivityRecord>> {
    let people = discover_cash_app_people(root)?;
    let db_path = root
        .join("AppDomainGroup-group.com.squareup.cash")
        .join("CDPPersistedAnalyticsMessages.SQLITE");
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = open_db_ro(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT message_uuid, recorded_at, payload FROM analytics_message ORDER BY recorded_at",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|row| row.ok()) {
        let (message_uuid, recorded_at_ms, payload) = row;
        let payload_text = String::from_utf8_lossy(&payload).to_string();
        for raw_object in extract_json_objects(&payload_text) {
            let Ok(object) = serde_json::from_str::<serde_json::Value>(&raw_object) else {
                continue;
            };
            if object.get("cdf_entity").and_then(|value| value.as_str()) != Some("ActivityHistory")
            {
                continue;
            }

            let counterparty_token = json_string(&object, "counterparty_token");
            let person = counterparty_token
                .as_ref()
                .and_then(|token| people.get(token))
                .cloned()
                .unwrap_or_default();
            let (amount, amount_text) = extract_amount_from_text(&raw_object);
            let recorded_at_utc =
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(recorded_at_ms)
                    .map(|value| value.to_rfc3339());
            let row_index = records.len();
            records.push(CashAppActivityRecord {
                record_id: format!("cashapp_activity_{}", row_index),
                source_path: db_path.display().to_string(),
                message_uuid: message_uuid.clone(),
                recorded_at_ms,
                recorded_at_utc,
                entity_id: json_string(&object, "entity_id"),
                counterparty_token,
                counterparty_name: person.name,
                counterparty_cashtag: person.cashtag,
                amount,
                amount_text,
                payment_role: json_string(&object, "payment_role"),
                payment_state: json_string(&object, "payment_state"),
                payment_orientation: json_string(&object, "payment_orientation"),
                activity_item_type: json_string(&object, "activity_item_type"),
                is_outstanding: json_string(&object, "is_outstanding"),
                is_recurring: json_string(&object, "is_recurring"),
                row_index: json_string(&object, "row_index"),
                origin: json_string(&object, "origin"),
                activity_flow_token: json_string(&object, "activity_flow_token"),
                raw_payload: raw_object,
            });
        }
    }

    Ok(records)
}

fn discover_cash_app_people(root: &Path) -> Result<HashMap<String, CashAppPerson>> {
    let mut people = HashMap::new();
    let db_path = root
        .join("AppDomainGroup-group.com.squareup.cash")
        .join("SearchIndex-internal.cashappapi.com.sqlite");
    if db_path.exists() {
        let conn = open_db_ro(&db_path)?;
        let mut stmt = conn.prepare(
            "SELECT l.entity_id, c.c0text_content \
             FROM entity_lookup l \
             LEFT JOIN entity_fts_content c ON c.docid = l.fts_docid \
             WHERE l.entity_id LIKE 'C_%'",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        for row in rows.filter_map(|row| row.ok()) {
            let (token, text) = row;
            let person = parse_search_index_person(text.as_deref().unwrap_or_default());
            people.entry(token).or_insert(person);
        }
    }

    let people_path = root
        .join("AppDomainGroup-group.com.squareup.cash")
        .join("CCPersonManagerPersistedPeople");
    if people_path.exists() {
        let text = String::from_utf8_lossy(&fs::read(people_path)?).to_string();
        let Ok(regex) = Regex::new(r#"\{"id":"(C_[^"]+)".*?\}"#) else {
            return Ok(people);
        };
        for capture in regex.captures_iter(&text) {
            let Some(raw) = capture.get(0).map(|value| value.as_str()) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
                continue;
            };
            let Some(token) = value.get("id").and_then(|value| value.as_str()) else {
                continue;
            };
            let person = CashAppPerson {
                name: json_string(&value, "full_name"),
                cashtag: json_string(&value, "cashtag"),
            };
            people.insert(token.to_string(), person);
        }
    }

    Ok(people)
}

fn discover_cash_app_indexed_transactions(
    root: &Path,
) -> Result<Vec<CashAppIndexedTransactionRecord>> {
    let people = discover_cash_app_people(root)?;
    let db_path = root
        .join("AppDomainGroup-group.com.squareup.cash")
        .join("SearchIndex-internal.cashappapi.com.sqlite");
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = open_db_ro(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT l.entity_id, l.customer_id, l.entity_type, c.c0text_content \
         FROM entity_lookup l \
         LEFT JOIN entity_fts_content c ON c.docid = l.fts_docid \
         WHERE c.c0text_content IS NOT NULL \
         ORDER BY l.fts_docid",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|row| row.ok()) {
        let (entity_id, counterparty_token, entity_type, display_text) = row;
        if !is_cash_app_indexed_transaction(&entity_id, entity_type, &display_text) {
            continue;
        }

        let person = counterparty_token
            .as_ref()
            .and_then(|token| people.get(token))
            .cloned()
            .unwrap_or_default();
        let (amount, amount_text) = extract_amount_from_text(&display_text);
        records.push(CashAppIndexedTransactionRecord {
            record_id: format!("cashapp_indexed_{}", records.len()),
            source_path: db_path.display().to_string(),
            entity_id: entity_id.clone(),
            counterparty_token,
            counterparty_name: person.name,
            counterparty_cashtag: person.cashtag,
            entity_type,
            observed_at_utc: observed_at_from_cash_app_entity_id(&entity_id),
            amount,
            amount_text,
            direction_hint: cash_app_direction_hint(&entity_id, &display_text),
            display_text: display_text.trim().to_string(),
        });
    }

    Ok(records)
}

fn is_cash_app_indexed_transaction(
    entity_id: &str,
    entity_type: Option<i64>,
    display_text: &str,
) -> bool {
    let lower = display_text.to_ascii_lowercase();
    let token_match = entity_id.starts_with("BMIT_")
        || entity_id.starts_with("MSI_")
        || entity_id.starts_with("OSI_")
        || entity_id.starts_with("PCD$_")
        || entity_id.starts_with("P2PE_")
        || entity_id.starts_with("ST$_");
    let type_match = matches!(entity_type, Some(3 | 4));
    (token_match || type_match)
        && (has_money_signal(&lower)
            || lower.contains("payment")
            || lower.contains("cash")
            || lower.contains("transfer")
            || lower.contains("added")
            || lower.contains("cashing"))
}

fn observed_at_from_cash_app_entity_id(entity_id: &str) -> Option<String> {
    let regex = Regex::new(r"^[A-Z]+_([0-9]{10})").ok()?;
    let captures = regex.captures(entity_id)?;
    let seconds = captures.get(1)?.as_str().parse::<i64>().ok()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0).map(|value| value.to_rfc3339())
}

fn cash_app_direction_hint(entity_id: &str, display_text: &str) -> Option<String> {
    let lower = display_text.to_ascii_lowercase();
    if entity_id.starts_with("MSI_")
        || entity_id.starts_with("PCD$_")
        || lower.contains("adding added cash")
    {
        Some("incoming_or_add_cash".to_string())
    } else if entity_id.starts_with("OSI_")
        || entity_id.starts_with("BMIT_")
        || lower.contains("cashing cashed out")
    {
        Some("outgoing_or_cash_out".to_string())
    } else if entity_id.starts_with("ST$_") {
        Some("card_or_merchant".to_string())
    } else {
        None
    }
}

fn extract_amount_from_text(text: &str) -> (Option<f64>, Option<String>) {
    let Ok(regex) = Regex::new(r"(?:\$?\b)(\d{1,6}(?:,\d{3})*(?:\.\d{2}))\b") else {
        return (None, None);
    };
    let Some(capture) = regex.captures_iter(text).last() else {
        return (None, None);
    };
    let Some(raw) = capture.get(1).map(|value| value.as_str().replace(',', "")) else {
        return (None, None);
    };
    (raw.parse::<f64>().ok(), Some(raw))
}

fn parse_search_index_person(text: &str) -> CashAppPerson {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.is_empty() {
        return CashAppPerson::default();
    }
    CashAppPerson {
        name: Some(parts[0].to_string()),
        cashtag: parts.get(1).map(|value| value.to_string()),
    }
}

fn extract_json_objects(text: &str) -> Vec<String> {
    let mut objects = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(start_idx) = start.take() {
                            objects.push(text[start_idx..=idx].to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    objects
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn discover_cash_app_artifacts(root: &Path) -> Result<Vec<CashAppArtifactRecord>> {
    let mut artifacts = Vec::new();
    let mut seen = BTreeSet::new();

    for domain in [
        "AppDomain-com.squareup.cash",
        "AppDomainGroup-group.com.squareup.cash",
        "AppDomain-com.squareup.invoices",
    ] {
        let domain_root = root.join(domain);
        if !domain_root.exists() {
            continue;
        }

        for entry in WalkDir::new(&domain_root)
            .max_depth(10)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if !seen.insert(rel.clone()) {
                continue;
            }

            let metadata = entry.metadata().ok();
            let file_size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            if is_sqlite_path(path) {
                match scan_cash_app_sqlite_artifact(path, &rel, file_size) {
                    Ok(mut found) => artifacts.append(&mut found),
                    Err(err) => artifacts.push(CashAppArtifactRecord {
                        record_id: artifact_record_id("cashapp_sqlite_error", &rel, 0),
                        source_path: path.display().to_string(),
                        source_kind: "sqlite_error".to_string(),
                        app: cash_app_artifact_app(&rel).to_string(),
                        table_name: None,
                        row_index: None,
                        file_size,
                        keyword_hits: vec!["sqlite_error".to_string()],
                        request_like: false,
                        payment_like: false,
                        preview: err.to_string(),
                    }),
                }
            } else if is_cash_app_text_candidate(&rel, file_size) {
                if let Some(record) = scan_cash_app_text_artifact(path, &rel, file_size) {
                    artifacts.push(record);
                }
            }
        }
    }

    artifacts.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then_with(|| left.table_name.cmp(&right.table_name))
            .then_with(|| left.row_index.cmp(&right.row_index))
    });
    Ok(artifacts)
}

fn is_sqlite_path(path: &Path) -> bool {
    let extension_match = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            matches!(lower.as_str(), "sqlite" | "db" | "sqlitedb" | "storedata")
        })
        .unwrap_or(false);
    if extension_match {
        return true;
    }

    fs::read(path)
        .map(|bytes| bytes.starts_with(b"SQLite format 3\0"))
        .unwrap_or(false)
}

fn is_cash_app_text_candidate(rel: &str, file_size: u64) -> bool {
    if file_size == 0 || file_size > 8 * 1024 * 1024 {
        return false;
    }
    let lower_rel = rel.to_ascii_lowercase();
    if lower_rel.ends_with(".pdf")
        || lower_rel.ends_with(".png")
        || lower_rel.ends_with(".jpg")
        || lower_rel.ends_with(".jpeg")
        || lower_rel.ends_with(".heic")
        || lower_rel.ends_with(".mp4")
        || lower_rel.ends_with(".mov")
    {
        return false;
    }

    lower_rel.contains("payment")
        || lower_rel.contains("request")
        || lower_rel.contains("recipient")
        || lower_rel.contains("invoice")
        || lower_rel.contains("money")
        || lower_rel.contains("activity")
        || lower_rel.ends_with(".json")
        || lower_rel.ends_with(".js")
        || lower_rel.ends_with(".plist")
}

fn scan_cash_app_text_artifact(
    path: &Path,
    rel: &str,
    file_size: u64,
) -> Option<CashAppArtifactRecord> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let hits = cash_app_keyword_hits(&text);
    if hits.is_empty() {
        return None;
    }
    let request_like = is_cash_app_request_like(&text);
    let payment_like = is_cash_app_payment_like(&text);
    if !request_like && !payment_like {
        return None;
    }

    Some(CashAppArtifactRecord {
        record_id: artifact_record_id("cashapp_text", rel, 0),
        source_path: path.display().to_string(),
        source_kind: cash_app_text_kind(rel).to_string(),
        app: cash_app_artifact_app(rel).to_string(),
        table_name: None,
        row_index: None,
        file_size,
        keyword_hits: hits,
        request_like,
        payment_like,
        preview: compact_preview(&text, 1200),
    })
}

fn scan_cash_app_sqlite_artifact(
    path: &Path,
    rel: &str,
    file_size: u64,
) -> Result<Vec<CashAppArtifactRecord>> {
    let conn = open_db_ro(path)?;
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let table_names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|row| row.ok())
        .collect();

    let mut artifacts = Vec::new();
    for table in table_names {
        if !is_safe_sql_identifier(&table) {
            continue;
        }
        let query = sqlite_select_all_query(&table);
        let mut stmt = match conn.prepare(&query) {
            Ok(stmt) => stmt,
            Err(_) => continue,
        };
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|idx| stmt.column_name(idx).unwrap_or("unknown").to_string())
            .collect();
        let rows = match stmt.query_map([], |row| {
            let mut parts = Vec::new();
            for idx in 0..col_count {
                let value = row.get::<_, SqlValue>(idx).ok();
                parts.push(format!(
                    "{}={}",
                    col_names[idx],
                    sqlite_value_artifact_preview(value)
                ));
            }
            Ok(parts.join(" | "))
        }) {
            Ok(rows) => rows,
            Err(_) => continue,
        };

        for (row_index, row_text) in rows.filter_map(|row| row.ok()).enumerate() {
            let hits = cash_app_keyword_hits(&row_text);
            if hits.is_empty() {
                continue;
            }
            let request_like = is_cash_app_request_like(&row_text);
            let payment_like = is_cash_app_payment_like(&row_text);
            if !request_like && !payment_like {
                continue;
            }

            artifacts.push(CashAppArtifactRecord {
                record_id: artifact_record_id(&format!("cashapp_sqlite_{}", table), rel, row_index),
                source_path: path.display().to_string(),
                source_kind: "sqlite_row_hit".to_string(),
                app: cash_app_artifact_app(rel).to_string(),
                table_name: Some(table.clone()),
                row_index: Some(row_index),
                file_size,
                keyword_hits: hits,
                request_like,
                payment_like,
                preview: compact_preview(&row_text, 1200),
            });
        }
    }

    Ok(artifacts)
}

fn sqlite_value_artifact_preview(value: Option<SqlValue>) -> String {
    match value {
        Some(SqlValue::Null) | None => String::new(),
        Some(SqlValue::Integer(value)) => value.to_string(),
        Some(SqlValue::Real(value)) => value.to_string(),
        Some(SqlValue::Text(value)) => value,
        Some(SqlValue::Blob(bytes)) => match String::from_utf8(bytes.clone()) {
            Ok(text) => text,
            Err(_) => format!("<blob {} bytes>", bytes.len()),
        },
    }
}

fn cash_app_keyword_hits(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    [
        "paymenthistory",
        "payment_history",
        "payment",
        "request",
        "requested",
        "recipient",
        "sender",
        "amount",
        "transaction",
        "transfer",
        "activity",
        "receipt",
        "invoice",
        "payment_type",
        "transaction_type",
        "sender_id",
        "recipient_id",
    ]
    .iter()
    .filter(|keyword| lower.contains(**keyword))
    .map(|keyword| keyword.to_string())
    .collect()
}

fn is_cash_app_request_like(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("request")
        || lower.contains("requested")
        || lower.contains("invoice")
        || lower.contains("amount_owed"))
        && (lower.contains("amount")
            || lower.contains("payment")
            || lower.contains("recipient")
            || lower.contains("sender")
            || lower.contains("money")
            || lower.contains("owed")
            || has_money_signal(&lower))
}

fn is_cash_app_payment_like(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("payment")
        || lower.contains("transaction")
        || lower.contains("transfer")
        || lower.contains("receipt")
        || lower.contains("amount"))
        && (lower.contains("recipient")
            || lower.contains("sender")
            || lower.contains("cash")
            || lower.contains("payment_type")
            || lower.contains("transaction_type")
            || has_money_signal(&lower))
}

fn has_money_signal(text: &str) -> bool {
    let Ok(regex) = Regex::new(r"(?:\$|\b)\d{1,6}(?:,\d{3})*(?:\.\d{2})\b") else {
        return false;
    };
    regex.is_match(text)
}

fn cash_app_text_kind(rel: &str) -> &'static str {
    let lower = rel.to_ascii_lowercase();
    if lower.ends_with(".js") {
        "javascript_bundle"
    } else if lower.ends_with(".json") {
        "json_file"
    } else if lower.ends_with(".plist") {
        "plist_file"
    } else {
        "text_file"
    }
}

fn cash_app_artifact_app(rel: &str) -> &'static str {
    let lower = rel.to_ascii_lowercase();
    if lower.contains("squareup.invoices") {
        "Square Invoices"
    } else {
        "Cash App"
    }
}

fn is_likely_money_request_artifact(record: &CashAppArtifactRecord) -> bool {
    if !record.request_like {
        return false;
    }
    if matches!(
        record.source_kind.as_str(),
        "javascript_bundle" | "plist_file" | "sqlite_error"
    ) {
        return false;
    }

    let lower_path = record.source_path.to_ascii_lowercase();
    if lower_path.contains("bugsnag")
        || lower_path.contains("featureflags")
        || lower_path.contains("feature_flags")
        || lower_path.contains("unleash")
        || lower_path.contains("fillr")
        || lower_path.contains("bitdrift")
        || lower_path.contains("onetrust")
        || lower_path.contains("webmonitoring")
        || lower_path.contains("preferences")
        || lower_path.contains("logging")
    {
        return false;
    }

    record.source_kind == "sqlite_row_hit" || record.source_kind == "json_file"
}

fn artifact_record_id(prefix: &str, rel: &str, index: usize) -> String {
    let cleaned = rel
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    format!("{}_{}_{}", prefix, cleaned, index)
}

fn compact_preview(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn is_cash_app_statement_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.starts_with("cash_app_") && lower.ends_with("statement.pdf")
}

fn parse_cash_app_statement(statement: &PreparedArtifact) -> Result<StatementParseResult> {
    let text = pdftotext_layout(&statement.working_path)?;
    let context = statement_context(&text, &statement.source_path);
    let expected = extract_statement_totals(&text);
    let rows = parse_statement_rows(&text);
    let computed = summarize_rows(&rows);

    let mut warnings = Vec::new();
    if let Some(expected) = expected {
        if !totals_match(expected, computed) {
            warnings.push(format!(
                "Statement totals mismatch for {}: expected in {:.2}/out {:.2}/fees {:.2}, computed in {:.2}/out {:.2}/fees {:.2}",
                statement.source_path.display(),
                expected.money_in,
                expected.money_out,
                expected.fees,
                computed.money_in,
                computed.money_out,
                computed.fees
            ));
        }
    } else if !rows.is_empty() {
        warnings.push(format!(
            "Statement summary totals were not found in {}",
            statement.source_path.display()
        ));
    }

    let statement_slug = statement
        .source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("cash_app_statement");
    let records = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let observed_date = context
                .statement_year
                .and_then(|year| normalize_statement_date(&row.raw_date, year));
            FinancialRecord {
                schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
                record_id: format!("cashapp:{}:{}", statement_slug, index + 1),
                source_agent: SOURCE_AGENT.to_string(),
                source_path: Some(statement.source_path.clone()),
                app: Some("Cash App".to_string()),
                account_label: context.account_label.clone(),
                observed_date,
                timestamp_utc: None,
                amount: Some(row.signed_amount),
                currency: Some("USD".to_string()),
                counterparty: (!row.description.trim().is_empty()).then(|| row.description.clone()),
                memo: None,
                transaction_type: Some(row.details.clone()),
                raw_reference: Some(row.raw_line),
                sha256: statement.sha256.clone(),
                fee_amount: Some(row.fee_amount),
                statement_period: context.statement_period.clone(),
            }
        })
        .collect();

    Ok(StatementParseResult { records, warnings })
}

fn pdftotext_layout(path: &Path) -> Result<String> {
    let output = Command::new("pdftotext")
        .args(["-layout"])
        .arg(path)
        .arg("-")
        .output()
        .with_context(|| format!("Failed to run pdftotext on {}", path.display()))?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "pdftotext failed for {} with status {}",
            path.display(),
            output.status
        ));
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("pdftotext output was not UTF-8 for {}", path.display()))
}

fn statement_context(text: &str, source_path: &Path) -> StatementContext {
    let period_re = Regex::new(
        r"^(January|February|March|April|May|June|July|August|September|October|November|December)\s+(\d{4})$",
    )
    .expect("valid period regex");
    let account_re = Regex::new(r"^Cash App\s{2,}(?P<label>.+?)\s*$").expect("valid account regex");

    let mut statement_period = None;
    let mut statement_year = None;
    let mut account_label = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if statement_period.is_none() {
            if let Some(captures) = period_re.captures(trimmed) {
                statement_period = Some(trimmed.to_string());
                statement_year = captures
                    .get(2)
                    .and_then(|value| value.as_str().parse::<i32>().ok());
            }
        }
        if account_label.is_none() {
            if let Some(captures) = account_re.captures(trimmed) {
                account_label = captures
                    .name("label")
                    .map(|value| value.as_str().trim().to_string());
            }
        }
        if statement_period.is_some() && account_label.is_some() {
            break;
        }
    }

    if statement_period.is_none() {
        statement_period = statement_period_from_filename(source_path);
        statement_year = statement_period
            .as_deref()
            .and_then(|value| value.split_whitespace().last())
            .and_then(|value| value.parse::<i32>().ok());
    }

    StatementContext {
        statement_period,
        statement_year,
        account_label,
    }
}

fn statement_period_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let cleaned = stem
        .replace("Cash_App_", "")
        .replace("_Account_Statement", "")
        .replace("_Statement", "");
    let month = cleaned.replace('_', " ");
    Some(month)
}

fn extract_statement_totals(text: &str) -> Option<StatementTotals> {
    let summary_re = Regex::new(
        r"^(?P<label>Money In|Money Out|Fees)\s+(?P<sign>[+-])?\s*\$(?P<amount>[\d,]+\.\d{2})\s*$",
    )
    .expect("valid summary regex");
    let mut totals = StatementTotals::default();
    let mut found_any = false;

    for line in text.lines() {
        let trimmed = line.trim();
        let Some(captures) = summary_re.captures(trimmed) else {
            continue;
        };
        let amount = parse_money(
            captures
                .name("amount")
                .map(|value| value.as_str())
                .unwrap_or_default(),
        );
        match captures.name("label").map(|value| value.as_str()) {
            Some("Money In") => totals.money_in = amount,
            Some("Money Out") => totals.money_out = amount,
            Some("Fees") => totals.fees = amount,
            _ => {}
        }
        found_any = true;
    }

    found_any.then_some(totals)
}

#[derive(Debug)]
struct StatementRow {
    raw_date: String,
    description: String,
    details: String,
    fee_amount: f64,
    signed_amount: f64,
    raw_line: String,
}

fn parse_statement_rows(text: &str) -> Vec<StatementRow> {
    let row_re = Regex::new(
        r"^(?P<date>[A-Z][a-z]{2}\s+\d{1,2})\s{2,}(?P<desc>.*?)\s{2,}(?P<details>.*?)\s+\$(?P<fee>[\d,]+\.\d{2})\s+(?P<sign>\+)?\s*\$(?P<amount>[\d,]+\.\d{2})\s*$",
    )
    .expect("valid transaction regex");

    text.lines()
        .filter_map(|line| {
            let captures = row_re.captures(line.trim_end())?;
            let fee_amount = parse_money(captures.name("fee")?.as_str());
            let amount = parse_money(captures.name("amount")?.as_str());
            let signed_amount = if captures.name("sign").is_some() {
                amount
            } else {
                -amount
            };
            let mut description = captures.name("desc")?.as_str().trim().to_string();
            let mut details = captures.name("details")?.as_str().trim().to_string();
            if details.is_empty() && !description.is_empty() {
                details = description;
                description = String::new();
            }

            Some(StatementRow {
                raw_date: captures.name("date")?.as_str().trim().to_string(),
                description,
                details,
                fee_amount,
                signed_amount,
                raw_line: line.trim_end().to_string(),
            })
        })
        .collect()
}

fn summarize_rows(rows: &[StatementRow]) -> StatementTotals {
    let mut totals = StatementTotals::default();
    for row in rows {
        if row.signed_amount >= 0.0 {
            totals.money_in += row.signed_amount;
        } else {
            totals.money_out += row.signed_amount.abs();
        }
        totals.fees += row.fee_amount;
    }
    totals
}

fn totals_match(expected: StatementTotals, computed: StatementTotals) -> bool {
    (expected.money_in - computed.money_in).abs() < 0.02
        && (expected.money_out - computed.money_out).abs() < 0.02
        && (expected.fees - computed.fees).abs() < 0.02
}

fn parse_money(raw: &str) -> f64 {
    raw.replace(',', "").parse::<f64>().unwrap_or(0.0)
}

fn normalize_statement_date(raw_date: &str, year: i32) -> Option<String> {
    let mut parts = raw_date.split_whitespace();
    let month = month_number(parts.next()?)?;
    let day = parts.next()?.parse::<u32>().ok()?;
    NaiveDate::from_ymd_opt(year, month, day).map(|value| value.format("%Y-%m-%d").to_string())
}

fn month_number(raw: &str) -> Option<u32> {
    match raw {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

fn export_transactions_json(records: &[FinancialRecord], path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(records)?)?;
    Ok(())
}

fn export_transactions_csv(records: &[FinancialRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Schema Version",
        "Record ID",
        "Source Agent",
        "Source Path",
        "App",
        "Account Label",
        "Observed Date",
        "Timestamp UTC",
        "Amount",
        "Currency",
        "Counterparty",
        "Memo",
        "Transaction Type",
        "Raw Reference",
        "SHA256",
        "Fee Amount",
        "Statement Period",
    ])?;

    for record in records {
        let source_path = record
            .source_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let amount = record
            .amount
            .map(|value| value.to_string())
            .unwrap_or_default();
        let fee_amount = record
            .fee_amount
            .map(|value| value.to_string())
            .unwrap_or_default();

        wtr.write_record([
            record.schema_version.to_string(),
            record.record_id.clone(),
            record.source_agent.clone(),
            source_path,
            record.app.clone().unwrap_or_default(),
            record.account_label.clone().unwrap_or_default(),
            record.observed_date.clone().unwrap_or_default(),
            record.timestamp_utc.clone().unwrap_or_default(),
            amount,
            record.currency.clone().unwrap_or_default(),
            record.counterparty.clone().unwrap_or_default(),
            record.memo.clone().unwrap_or_default(),
            record.transaction_type.clone().unwrap_or_default(),
            record.raw_reference.clone().unwrap_or_default(),
            record.sha256.clone().unwrap_or_default(),
            fee_amount,
            record.statement_period.clone().unwrap_or_default(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn discover_financial_sources(root: &Path) -> Result<Vec<FinancialSourceRecord>> {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();

    if !root.exists() {
        return Ok(sources);
    }

    for entry in WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let lower_rel = rel.to_ascii_lowercase();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let lower_name = file_name.to_ascii_lowercase();

        let Some((label, app, source_type, parsed, tags)) =
            classify_financial_source(&rel, &lower_rel, &lower_name)
        else {
            continue;
        };

        if !seen.insert(rel) {
            continue;
        }

        let size_bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        sources.push(FinancialSourceRecord {
            label: label.to_string(),
            app: app.to_string(),
            source_type: source_type.to_string(),
            path: path.display().to_string(),
            size_bytes,
            parsed,
            tags: tags.into_iter().map(str::to_string).collect(),
        });
    }

    sources.sort_by(|left, right| {
        left.app
            .cmp(&right.app)
            .then_with(|| left.source_type.cmp(&right.source_type))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(sources)
}

fn classify_financial_source<'a>(
    rel: &str,
    lower_rel: &str,
    lower_name: &str,
) -> Option<(&'a str, &'a str, &'a str, bool, Vec<&'a str>)> {
    let is_sqlite = matches!(
        Path::new(lower_name)
            .extension()
            .and_then(|value| value.to_str()),
        Some("sqlite" | "db" | "sqlitedb" | "storedata")
    );
    let is_plist = lower_name.ends_with(".plist");

    if lower_rel.contains("appdomain-com.squareup.cash/")
        && is_cash_app_statement_name(Path::new(rel).file_name()?.to_str()?)
    {
        return Some((
            "Cash App statement",
            "Cash App",
            "statement_pdf",
            true,
            vec!["finance", "cash_app", "statement"],
        ));
    }

    if lower_rel.contains("appdomain-com.squareup.cash/") && is_sqlite {
        return Some((
            "Cash App database",
            "Cash App",
            "sqlite",
            true,
            vec!["finance", "cash_app", "database"],
        ));
    }

    if lower_rel.contains("appdomaingroup-group.com.squareup.cash/") && is_sqlite {
        return Some((
            "Cash App app group database",
            "Cash App",
            "sqlite",
            true,
            vec!["finance", "cash_app", "app_group", "database"],
        ));
    }

    if lower_rel.contains("appdomain-com.1debit.chimeprodapp/") && is_sqlite {
        return Some((
            "Chime database",
            "Chime",
            "sqlite",
            true,
            vec!["finance", "chime", "database"],
        ));
    }

    if lower_rel.contains("appdomain-net.kortina.labs.venmo/") && is_sqlite {
        return Some((
            "Venmo database",
            "Venmo",
            "sqlite",
            true,
            vec!["finance", "venmo", "database"],
        ));
    }

    if lower_rel.contains("appdomaingroup-group.net.kortina.labs.venmo/") && is_sqlite {
        return Some((
            "Venmo app group database",
            "Venmo",
            "sqlite",
            true,
            vec!["finance", "venmo", "app_group", "database"],
        ));
    }

    if lower_rel.contains("appdomain-com.empower.finance/") && is_sqlite {
        return Some((
            "Empower database",
            "Empower",
            "sqlite",
            true,
            vec!["finance", "empower", "database"],
        ));
    }

    if lower_rel.contains("appdomain-com.squareup.invoices/") && is_sqlite {
        return Some((
            "Square Invoices database",
            "Square Invoices",
            "sqlite",
            true,
            vec!["finance", "square_invoices", "database"],
        ));
    }

    if lower_rel.contains("homedomain/library/passes/catalogofrecord.plist") {
        return Some((
            "Apple Wallet pass catalog",
            "Apple Wallet",
            "wallet_pass_catalog",
            false,
            vec!["finance", "apple_wallet", "passes", "plist"],
        ));
    }

    if lower_rel.contains("homedomain/library/passes/nonubiquitouscatalogofrecord.plist") {
        return Some((
            "Apple Wallet local pass catalog",
            "Apple Wallet",
            "wallet_pass_catalog",
            false,
            vec!["finance", "apple_wallet", "passes", "plist"],
        ));
    }

    if lower_rel.contains("homedomain/library/preferences/com.apple.wallet.plist") && is_plist {
        return Some((
            "Apple Wallet preferences",
            "Apple Wallet",
            "wallet_preferences",
            false,
            vec!["finance", "apple_wallet", "preferences", "plist"],
        ));
    }

    None
}

fn export_financial_sources_json(sources: &[FinancialSourceRecord], path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(sources)?)?;
    Ok(())
}

fn export_financial_sources_csv(sources: &[FinancialSourceRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Label",
        "App",
        "Source Type",
        "Path",
        "Size Bytes",
        "Parsed",
        "Tags",
    ])?;

    for source in sources {
        wtr.write_record([
            source.label.clone(),
            source.app.clone(),
            source.source_type.clone(),
            source.path.clone(),
            source.size_bytes.to_string(),
            source.parsed.to_string(),
            source.tags.join(";"),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn export_cash_app_artifacts_json(records: &[CashAppArtifactRecord], path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(records)?)?;
    Ok(())
}

fn export_cash_app_artifacts_csv(records: &[CashAppArtifactRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Record ID",
        "Source Path",
        "Source Kind",
        "App",
        "Table Name",
        "Row Index",
        "File Size",
        "Keyword Hits",
        "Request Like",
        "Payment Like",
        "Preview",
    ])?;

    for record in records {
        wtr.write_record([
            record.record_id.clone(),
            record.source_path.clone(),
            record.source_kind.clone(),
            record.app.clone(),
            record.table_name.clone().unwrap_or_default(),
            record
                .row_index
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.file_size.to_string(),
            record.keyword_hits.join(";"),
            record.request_like.to_string(),
            record.payment_like.to_string(),
            record.preview.clone(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn export_cash_app_activity_json(records: &[CashAppActivityRecord], path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(records)?)?;
    Ok(())
}

fn export_cash_app_activity_csv(records: &[CashAppActivityRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Record ID",
        "Recorded At UTC",
        "Recorded At MS",
        "Message UUID",
        "Entity ID",
        "Counterparty Token",
        "Counterparty Name",
        "Counterparty Cashtag",
        "Amount",
        "Amount Text",
        "Payment Role",
        "Payment State",
        "Payment Orientation",
        "Activity Item Type",
        "Is Outstanding",
        "Is Recurring",
        "Row Index",
        "Origin",
        "Activity Flow Token",
        "Source Path",
        "Raw Payload",
    ])?;

    for record in records {
        wtr.write_record([
            record.record_id.clone(),
            record.recorded_at_utc.clone().unwrap_or_default(),
            record.recorded_at_ms.to_string(),
            record.message_uuid.clone(),
            record.entity_id.clone().unwrap_or_default(),
            record.counterparty_token.clone().unwrap_or_default(),
            record.counterparty_name.clone().unwrap_or_default(),
            record.counterparty_cashtag.clone().unwrap_or_default(),
            record
                .amount
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.amount_text.clone().unwrap_or_default(),
            record.payment_role.clone().unwrap_or_default(),
            record.payment_state.clone().unwrap_or_default(),
            record.payment_orientation.clone().unwrap_or_default(),
            record.activity_item_type.clone().unwrap_or_default(),
            record.is_outstanding.clone().unwrap_or_default(),
            record.is_recurring.clone().unwrap_or_default(),
            record.row_index.clone().unwrap_or_default(),
            record.origin.clone().unwrap_or_default(),
            record.activity_flow_token.clone().unwrap_or_default(),
            record.source_path.clone(),
            record.raw_payload.clone(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn export_cash_app_indexed_transactions_json(
    records: &[CashAppIndexedTransactionRecord],
    path: &Path,
) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(records)?)?;
    Ok(())
}

fn export_cash_app_indexed_transactions_csv(
    records: &[CashAppIndexedTransactionRecord],
    path: &Path,
) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Record ID",
        "Observed At UTC",
        "Entity ID",
        "Counterparty Token",
        "Counterparty Name",
        "Counterparty Cashtag",
        "Entity Type",
        "Amount",
        "Amount Text",
        "Direction Hint",
        "Display Text",
        "Source Path",
    ])?;

    for record in records {
        wtr.write_record([
            record.record_id.clone(),
            record.observed_at_utc.clone().unwrap_or_default(),
            record.entity_id.clone(),
            record.counterparty_token.clone().unwrap_or_default(),
            record.counterparty_name.clone().unwrap_or_default(),
            record.counterparty_cashtag.clone().unwrap_or_default(),
            record
                .entity_type
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .amount
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.amount_text.clone().unwrap_or_default(),
            record.direction_hint.clone().unwrap_or_default(),
            record.display_text.clone(),
            record.source_path.clone(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn export_financial_sources_html(
    case_name: &str,
    sources: &[FinancialSourceRecord],
    path: &Path,
) -> Result<()> {
    let rows = sources
        .iter()
        .map(|source| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&source.app),
                escape_html(&source.label),
                escape_html(&source.source_type),
                source.size_bytes,
                source.parsed,
                escape_html(&source.path)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let html = format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Plutus Financial Sources - {case}</title>
<style>
body {{ font-family: Arial, sans-serif; margin: 32px; color: #1f2933; }}
h1 {{ font-size: 22px; margin-bottom: 4px; }}
.meta {{ color: #5c6873; margin-bottom: 20px; }}
table {{ border-collapse: collapse; width: 100%; font-size: 12px; }}
th, td {{ border: 1px solid #d8dee4; padding: 6px 8px; text-align: left; vertical-align: top; }}
th {{ background: #eef2f6; }}
td:last-child {{ overflow-wrap: anywhere; }}
@media print {{ body {{ margin: 16px; }} }}
</style>
</head>
<body>
<h1>Plutus Financial Sources</h1>
<div class="meta">Case: {case} | Sources: {count}</div>
<table>
<thead><tr><th>App</th><th>Label</th><th>Type</th><th>Size</th><th>Parsed</th><th>Path</th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
</body>
</html>
"#,
        case = escape_html(case_name),
        count = sources.len(),
        rows = rows
    );
    fs::write(path, html)?;
    Ok(())
}

fn source_breakdown(sources: &[FinancialSourceRecord]) -> BTreeMap<String, usize> {
    let mut breakdown = BTreeMap::new();
    for source in sources {
        *breakdown
            .entry(format!("{}:{}", source.app, source.source_type))
            .or_insert(0) += 1;
    }
    breakdown
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_safe_sql_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn sanitize_sqlite_value(val: SqlValue) -> serde_json::Value {
    match val {
        SqlValue::Null => serde_json::Value::Null,
        SqlValue::Integer(i) => serde_json::Value::Number(i.into()),
        SqlValue::Real(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        SqlValue::Text(s) => serde_json::Value::String(s),
        SqlValue::Blob(b) => {
            // Try to interpret blob as UTF-8 JSON first
            if let Ok(s) = String::from_utf8(b.clone()) {
                if s.trim_start().starts_with('{') || s.trim_start().starts_with('[') {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&s) {
                        return json;
                    }
                }
                return serde_json::Value::String(s);
            }
            serde_json::Value::String(format!("<bytes {}>", b.len()))
        }
    }
}

fn open_db_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open SQLite database: {}", path.display()))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Ok(conn)
}

fn is_payment_app_domain(domain: &str) -> bool {
    let lower = domain.to_lowercase();
    let keywords = [
        "cash", "venmo", "chime", "paypal", "zelle", "bank", "credit", "debit", "wallet", "pay",
        "transfer", "squareup", "1debit", "invoice",
    ];
    keywords.iter().any(|k| lower.contains(k))
}

fn is_interesting_table(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("transaction")
        || lower.contains("payment")
        || lower.contains("transfer")
        || lower.contains("request")
        || lower.contains("charge")
        || lower.contains("bill")
        || lower.contains("invoice")
        || lower.contains("receipt")
        || lower.contains("purchase")
        || lower.contains("order")
        || lower.contains("card")
        || lower.contains("account")
        || lower.contains("balance")
}

fn extract_payment_app_sqlite(
    backup_root: &Path,
    out_records: &mut Vec<FinancialRecord>,
    files_examined: &mut usize,
    files_parsed: &mut usize,
    warnings: &mut Vec<String>,
) {
    let entries: Vec<_> = match walkdir::WalkDir::new(backup_root)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect()
    {
        e => e,
    };

    for entry in entries {
        if !entry.file_type().is_dir() {
            continue;
        }
        let domain_path = entry.path();
        let domain = domain_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if !domain.starts_with("AppDomain") && !domain.starts_with("AppDomainGroup") {
            continue;
        }
        if !is_payment_app_domain(domain) {
            continue;
        }

        let app_name = if domain.contains("Chime") {
            "Chime"
        } else if domain.contains("Venmo") {
            "Venmo"
        } else if domain.contains("cash") || domain.contains("squareup") {
            "Cash App"
        } else if domain.contains("PayPal") {
            "PayPal"
        } else {
            "Payment App"
        };

        let db_files: Vec<_> = walkdir::WalkDir::new(domain_path)
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        let lower = ext.to_lowercase();
                        lower == "sqlite"
                            || lower == "db"
                            || lower == "sqlitedb"
                            || lower == "storedata"
                    })
                    .unwrap_or(false)
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        for db_path in db_files {
            *files_examined += 1;
            match extract_from_payment_db(&db_path, app_name, domain) {
                Ok(mut recs) => {
                    if !recs.is_empty() {
                        *files_parsed += 1;
                        out_records.append(&mut recs);
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "SQLite extraction failed for {}: {}",
                        db_path.display(),
                        e
                    ));
                }
            }
        }
    }
}

fn extract_from_payment_db(
    db_path: &Path,
    app_name: &str,
    domain: &str,
) -> Result<Vec<FinancialRecord>> {
    let conn = open_db_ro(db_path)?;
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let table_names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut records = Vec::new();
    for table in table_names {
        if !is_interesting_table(&table) {
            continue;
        }
        if !is_safe_sql_identifier(&table) {
            continue;
        }
        let query = sqlite_select_all_query(&table);
        let mut stmt = match conn.prepare(&query) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
            .collect();

        let rows = match stmt.query_map([], |row| {
            let mut map = HashMap::new();
            for idx in 0..col_count {
                let val = match row.get::<_, SqlValue>(idx) {
                    Ok(v) => sanitize_sqlite_value(v),
                    Err(_) => serde_json::Value::Null,
                };
                map.insert(col_names[idx].clone(), val);
            }
            Ok(map)
        }) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for row in rows.filter_map(|r| r.ok()) {
            let counterparty = find_counterparty(&row, &col_names);
            let memo = find_memo(&row, &col_names);
            let timestamp = find_timestamp(&row, &col_names);
            let amount = find_amount(&row, &col_names);
            let tx_type = find_tx_type(&row, &col_names, app_name, &table);

            let record = FinancialRecord {
                schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
                record_id: format!("plutus_sqlite_{}_{}_{}", domain, table, records.len()),
                source_agent: SOURCE_AGENT.to_string(),
                source_path: Some(db_path.to_path_buf()),
                app: Some(app_name.to_string()),
                account_label: None,
                observed_date: timestamp.clone(),
                timestamp_utc: timestamp,
                amount,
                currency: Some("USD".to_string()),
                counterparty,
                memo,
                transaction_type: Some(tx_type),
                raw_reference: Some(serde_json::to_string(&row).unwrap_or_default()),
                sha256: None,
                fee_amount: None,
                statement_period: None,
            };
            records.push(record);
        }
    }
    Ok(records)
}

fn sqlite_select_all_query(table: &str) -> String {
    format!("SELECT * FROM '{}'", table.replace('\'', "''"))
}

fn is_money_request_record(record: &FinancialRecord) -> bool {
    let haystack = [
        record.transaction_type.as_deref().unwrap_or_default(),
        record.memo.as_deref().unwrap_or_default(),
        record.raw_reference.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    haystack.contains("request")
        || haystack.contains("requested")
        || haystack.contains("invoice")
        || haystack.contains("owed")
        || haystack.contains("money request")
        || haystack.contains("request_money")
        || haystack.contains("requested_money")
}

fn find_counterparty(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<String> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("name")
            || lower.contains("display")
            || lower.contains("user")
            || lower.contains("counterparty")
            || lower.contains("recipient")
            || lower.contains("sender")
        {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return Some(s.clone());
                }
            }
        }
    }
    None
}

fn find_memo(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<String> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("memo")
            || lower.contains("note")
            || lower.contains("message")
            || lower.contains("description")
            || lower.contains("status")
            || lower.contains("request")
            || lower.contains("invoice")
            || lower.contains("charge")
            || lower.contains("bill")
        {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return Some(s.clone());
                }
            }
        }
    }
    None
}

fn find_timestamp(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<String> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("time")
            || lower.contains("date")
            || lower.contains("created")
            || lower.contains("modified")
        {
            if let Some(v) = row.get(col) {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                if !s.is_empty() && s != "0" {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn find_amount(row: &HashMap<String, serde_json::Value>, cols: &[String]) -> Option<f64> {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("amount")
            || lower.contains("sum")
            || lower.contains("total")
            || lower.contains("balance")
            || lower.contains("due")
            || lower.contains("owed")
        {
            if let Some(v) = row.get(col) {
                match v {
                    serde_json::Value::Number(n) => return n.as_f64(),
                    serde_json::Value::String(s) => return s.parse().ok(),
                    _ => continue,
                }
            }
        }
    }
    None
}

fn find_tx_type(
    row: &HashMap<String, serde_json::Value>,
    cols: &[String],
    app: &str,
    table: &str,
) -> String {
    for col in cols {
        let lower = col.to_lowercase();
        if lower.contains("type")
            || lower.contains("status")
            || lower.contains("state")
            || lower.contains("request")
            || lower.contains("invoice")
            || lower.contains("charge")
        {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return format!("{}:{}", table, s);
                }
            }
        }
    }
    format!("{}:{}", app, table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_financial_sources_from_helios_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("AppDomain-com.squareup.cash/Documents")).unwrap();
        fs::write(
            root.join("AppDomain-com.squareup.cash/Documents/Cash_App_January_2023_Statement.pdf"),
            b"pdf",
        )
        .unwrap();
        fs::create_dir_all(root.join("AppDomain-com.1debit.ChimeProdApp/Documents")).unwrap();
        fs::write(
            root.join("AppDomain-com.1debit.ChimeProdApp/Documents/VisaAnalytics_v2.sqlite"),
            b"sqlite",
        )
        .unwrap();
        fs::create_dir_all(root.join("HomeDomain/Library/Passes")).unwrap();
        fs::write(
            root.join("HomeDomain/Library/Passes/CatalogOfRecord.plist"),
            b"plist",
        )
        .unwrap();

        let sources = discover_financial_sources(root).unwrap();
        let labels: Vec<_> = sources.iter().map(|s| s.label.as_str()).collect();

        assert!(labels.contains(&"Cash App statement"));
        assert!(labels.contains(&"Chime database"));
        assert!(labels.contains(&"Apple Wallet pass catalog"));
    }

    #[test]
    fn financial_source_csv_flattens_tags() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sources.csv");
        let sources = vec![FinancialSourceRecord {
            label: "Cash App statement".to_string(),
            app: "Cash App".to_string(),
            source_type: "statement_pdf".to_string(),
            path: "/tmp/example.pdf".to_string(),
            size_bytes: 3,
            parsed: true,
            tags: vec!["finance".to_string(), "cash_app".to_string()],
        }];

        export_financial_sources_csv(&sources, &path).unwrap();
        let csv = fs::read_to_string(path).unwrap();

        assert!(csv.contains("finance;cash_app"));
    }

    #[test]
    fn sqlite_extraction_query_has_no_row_limit() {
        assert_eq!(
            sqlite_select_all_query("transactions"),
            "SELECT * FROM 'transactions'"
        );
        assert!(!sqlite_select_all_query("transactions")
            .to_ascii_lowercase()
            .contains("limit"));
    }

    #[test]
    fn interesting_tables_exclude_telemetry_noise() {
        assert!(is_interesting_table("payments"));
        assert!(is_interesting_table("card_transactions"));
        assert!(is_interesting_table("invoice_items"));
        assert!(is_interesting_table("money_requests"));
        assert!(!is_interesting_table("analytics_message"));
        assert!(!is_interesting_table("ZMANAGEDUSER"));
        assert!(!is_interesting_table("ZCLIENTS"));
    }

    #[test]
    fn money_request_records_are_identified() {
        let record = FinancialRecord {
            schema_version: PLUTUS_FINANCIAL_SCHEMA_VERSION,
            record_id: "r1".to_string(),
            source_agent: SOURCE_AGENT.to_string(),
            source_path: None,
            app: Some("Cash App".to_string()),
            account_label: None,
            observed_date: None,
            timestamp_utc: None,
            amount: Some(25.0),
            currency: Some("USD".to_string()),
            counterparty: Some("Example".to_string()),
            memo: Some("requested money for invoice".to_string()),
            transaction_type: Some("payment_request".to_string()),
            raw_reference: None,
            sha256: None,
            fee_amount: None,
            statement_period: None,
        };

        assert!(is_money_request_record(&record));

        let card_bill = FinancialRecord {
            memo: None,
            transaction_type: Some("Cash Card".to_string()),
            counterparty: Some("Apple.com Bill".to_string()),
            raw_reference: Some("Apple.com Bill Cash Card".to_string()),
            ..record
        };
        assert!(!is_money_request_record(&card_bill));
    }
}
