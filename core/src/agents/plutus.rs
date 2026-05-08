//! Plutus — Deterministic financial artifact extraction.

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};

use crate::common::prepared::{prepare_artifact, PrepareContext, PreparedArtifact};
use crate::common::resolver::{
    write_resolver_audit, ArtifactResolver, BackupResolver, ResolveMethod, ResolvedPath, ResolverAuditRecord,
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
pub struct PlutusReport {
    pub schema_version: u32,
    pub case_id: String,
    pub generated_at: String,
    pub state: String,
    pub files_examined: usize,
    pub files_parsed: usize,
    pub records_emitted: usize,
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

        let statements = discover_cash_app_statements(&resolver, &prepare_ctx)?;
        let mut supported_breakdown = BTreeMap::new();
        supported_breakdown.insert("cash_app_statement_pdf".to_string(), statements.len());

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

        export_transactions_json(&records, &transactions_json_path)?;
        export_transactions_csv(&records, &transactions_csv_path)?;

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
            payload: serde_json::to_value(&report)?,
        });

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
                payload: serde_json::to_value(&record)?,
            });
        }

        Ok(evidence_records)
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
        "cash", "venmo", "chime", "paypal", "zelle", "bank", "credit", "debit",
        "wallet", "pay", "transfer", "squareup", "1debit",
    ];
    keywords.iter().any(|k| lower.contains(k))
}

fn is_interesting_table(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("transaction")
        || lower.contains("payment")
        || lower.contains("transfer")
        || lower.contains("user")
        || lower.contains("business")
        || lower.contains("event")
        || lower.contains("card")
        || lower.contains("account")
        || lower.contains("search")
        || lower.contains("message")
        || lower.contains("client")
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
        let domain = domain_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
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
                        lower == "sqlite" || lower == "db" || lower == "sqlitedb" || lower == "storedata"
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
        let query = format!("SELECT * FROM '{}' LIMIT 500", table);
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
                record_id: format!(
                    "plutus_sqlite_{}_{}_{}",
                    domain,
                    table,
                    records.len()
                ),
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

fn find_counterparty(
    row: &HashMap<String, serde_json::Value>,
    cols: &[String],
) -> Option<String> {
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
        if lower.contains("type") || lower.contains("status") || lower.contains("state") {
            if let Some(serde_json::Value::String(s)) = row.get(col) {
                if !s.is_empty() {
                    return format!("{}:{}", table, s);
                }
            }
        }
    }
    format!("{}:{}", app, table)
}
