//! Obolus Agent
//!
//! Obolus is the financial review layer. Plutus does the low-level artifact
//! parsing; Obolus normalizes those results with money-related message context
//! so the case dashboard has one place to review financial signals.

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use flate2::read::GzDecoder;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

const MONEY_TERMS: &[&str] = &[
    "cashapp",
    "cash app",
    "venmo",
    "paypal",
    "zelle",
    "chime",
    "bank",
    "atm",
    "card",
    "debit",
    "credit",
    "invoice",
    "estimate",
    "transaction",
    "payment",
    "pay ",
    "paid",
    "cash",
    "check",
    "cheque",
    "deposit",
    "transfer",
    "refund",
    "fee",
    "$",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ObolusMoneyRecord {
    record_id: String,
    source_file: String,
    source_record_type: String,
    app: Option<String>,
    observed_at: Option<String>,
    amount: Option<f64>,
    amount_text: Option<String>,
    direction_hint: Option<String>,
    counterparty: Option<String>,
    memo: Option<String>,
    transaction_type: Option<String>,
    display_text: String,
    source_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ObolusThreadMention {
    record_id: String,
    message_id: Option<i64>,
    thread_id: Option<String>,
    phone_number: Option<String>,
    direction: Option<String>,
    service: Option<String>,
    message_date_raw: Option<i64>,
    keyword_hits: Vec<String>,
    amount_mentions: Vec<String>,
    text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ObolusNoteRecord {
    id: String,
    title: String,
    body: String,
    source: String,
    source_path: String,
    created: Option<String>,
    modified: Option<String>,
    deleted: bool,
    locked: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ObolusReport {
    schema_version: u32,
    case_id: String,
    generated_at: String,
    state: String,
    plutus_records: usize,
    notes: usize,
    thread_mentions: usize,
    amount_mentions: usize,
    source_breakdown: BTreeMap<String, usize>,
    warnings: Vec<String>,
}

pub struct ObolusAgent;

impl Agent for ObolusAgent {
    const NAME: &'static str = "Obolus";
    const SLUG: &'static str = "obolus";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let evidence_dir = ctx.case.evidence_path(Self::SLUG);
        fs::create_dir_all(&evidence_dir)?;

        let mut warnings = Vec::new();
        let plutus_dir = ctx.case.evidence_path("plutus");
        let cerberus_records_path = ctx.case.evidence_path("cerberus").join("records.json");
        let notes = extract_notes(&ctx.backup_root, &mut warnings)?;

        let mut money_records = Vec::new();
        for (file_name, source_type) in [
            ("transactions.json", "transaction"),
            ("money_requests.json", "money_request"),
            (
                "cash_app_indexed_transactions.json",
                "cash_app_indexed_transaction",
            ),
            ("cash_app_activity_history.json", "cash_app_activity"),
        ] {
            let path = plutus_dir.join(file_name);
            match read_json_array(&path) {
                Ok(values) => {
                    for (index, value) in values.iter().enumerate() {
                        if let Some(record) =
                            normalize_plutus_record(file_name, source_type, index, value)
                        {
                            money_records.push(record);
                        }
                    }
                }
                Err(error) if path.exists() => {
                    warnings.push(format!("Failed to read Plutus {}: {}", file_name, error))
                }
                Err(_) => warnings.push(format!("Missing Plutus {}", file_name)),
            }
        }

        let thread_mentions = match read_json_array(&cerberus_records_path) {
            Ok(values) => values
                .iter()
                .filter_map(normalize_money_thread_mention)
                .collect::<Vec<_>>(),
            Err(error) if cerberus_records_path.exists() => {
                warnings.push(format!("Failed to read Cerberus records: {}", error));
                Vec::new()
            }
            Err(_) => {
                warnings.push("Missing Cerberus records.json".to_string());
                Vec::new()
            }
        };

        money_records.sort_by(|left, right| {
            right
                .observed_at
                .cmp(&left.observed_at)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });

        let mut source_breakdown = BTreeMap::new();
        for record in &money_records {
            *source_breakdown
                .entry(record.source_record_type.clone())
                .or_insert(0) += 1;
        }
        if !notes.is_empty() {
            source_breakdown.insert("note".to_string(), notes.len());
        }
        if !thread_mentions.is_empty() {
            source_breakdown.insert(
                "financial_thread_mention".to_string(),
                thread_mentions.len(),
            );
        }

        write_json(&evidence_dir.join("money_index.json"), &money_records)?;
        write_money_index_csv(&evidence_dir.join("money_index.csv"), &money_records)?;
        write_json(&evidence_dir.join("notes.json"), &notes)?;
        write_notes_csv(&evidence_dir.join("notes.csv"), &notes)?;
        write_json(&evidence_dir.join("thread_mentions.json"), &thread_mentions)?;
        write_thread_mentions_csv(&evidence_dir.join("thread_mentions.csv"), &thread_mentions)?;

        let amount_mentions = thread_mentions
            .iter()
            .map(|mention| mention.amount_mentions.len())
            .sum();
        let report = ObolusReport {
            schema_version: Self::SCHEMA_VERSION,
            case_id: ctx.case.name().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            state: "complete".to_string(),
            plutus_records: money_records.len(),
            notes: notes.len(),
            thread_mentions: thread_mentions.len(),
            amount_mentions,
            source_breakdown,
            warnings,
        };
        write_json(&evidence_dir.join("report.json"), &report)?;
        write_index_html(
            &evidence_dir.join("index.html"),
            ctx.case.name(),
            &report,
            &notes,
            &money_records,
            &thread_mentions,
        )?;

        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        });
        for record in money_records {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: record.source_record_type.clone(),
                timestamp: record
                    .observed_at
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(record)?,
            });
        }
        for note in notes {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "note".to_string(),
                timestamp: note
                    .modified
                    .clone()
                    .or_else(|| note.created.clone())
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(note)?,
            });
        }
        for mention in thread_mentions {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "financial_thread_mention".to_string(),
                timestamp: Utc::now().to_rfc3339(),
                payload: serde_json::to_value(mention)?,
            });
        }
        Ok(records)
    }
}

fn read_json_array(path: &Path) -> Result<Vec<serde_json::Value>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn extract_notes(root: &Path, warnings: &mut Vec<String>) -> Result<Vec<ObolusNoteRecord>> {
    let mut notes = Vec::new();
    let modern = root
        .join("AppDomainGroup-group.com.apple.notes")
        .join("NoteStore.sqlite");
    if modern.exists() {
        match extract_modern_notes(&modern) {
            Ok(mut records) => notes.append(&mut records),
            Err(error) => warnings.push(format!(
                "Failed to extract modern Notes from {}: {}",
                modern.display(),
                error
            )),
        }
    } else {
        warnings.push(format!("Missing modern Notes DB {}", modern.display()));
    }

    let legacy = root
        .join("HomeDomain")
        .join("Library")
        .join("Notes")
        .join("notes.sqlite");
    if legacy.exists() {
        match extract_legacy_notes(&legacy) {
            Ok(mut records) => notes.append(&mut records),
            Err(error) => warnings.push(format!(
                "Failed to extract legacy Notes from {}: {}",
                legacy.display(),
                error
            )),
        }
    }

    notes.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| right.created.cmp(&left.created))
            .then_with(|| left.id.cmp(&right.id))
    });
    notes.dedup_by(|left, right| {
        left.title == right.title && left.body == right.body && left.created == right.created
    });
    Ok(notes)
}

fn extract_modern_notes(path: &Path) -> Result<Vec<ObolusNoteRecord>> {
    let conn = open_db(path)?;
    let note_entity: i64 = conn
        .query_row(
            "SELECT Z_ENT FROM Z_PRIMARYKEY WHERE Z_NAME='ICNote'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(12);
    let mut stmt = conn.prepare(
        "SELECT note.Z_PK,
                COALESCE(note.ZTITLE, note.ZFALLBACKTITLE, ''),
                COALESCE(note.ZSNIPPET, note.ZSUMMARY, note.ZFALLBACKSUBTITLEIOS, ''),
                note.ZCREATIONDATE3,
                note.ZMODIFICATIONDATE1,
                COALESCE(note.ZMARKEDFORDELETION, 0),
                COALESCE(note.ZISPASSWORDPROTECTED, 0),
                data.ZDATA
         FROM ZICCLOUDSYNCINGOBJECT note
         LEFT JOIN ZICNOTEDATA data ON data.ZNOTE = note.Z_PK
         WHERE note.Z_ENT = ?1
         ORDER BY note.ZMODIFICATIONDATE1 DESC",
    )?;
    let rows = stmt.query_map([note_entity], |row| {
        let id: i64 = row.get(0)?;
        let title: String = row.get(1)?;
        let snippet: String = row.get(2)?;
        let created: Option<f64> = row.get(3)?;
        let modified: Option<f64> = row.get(4)?;
        let deleted: i64 = row.get(5)?;
        let locked: i64 = row.get(6)?;
        let data: Option<Vec<u8>> = row.get(7)?;
        let body = data
            .as_deref()
            .and_then(decode_note_blob)
            .map(|text| html_to_text(&text))
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| snippet.clone());
        Ok(ObolusNoteRecord {
            id: format!("modern:{id}"),
            title,
            body,
            source: "modern_notes".to_string(),
            source_path: path.display().to_string(),
            created: apple_time_to_rfc3339(created),
            modified: apple_time_to_rfc3339(modified),
            deleted: deleted != 0,
            locked: locked != 0,
        })
    })?;
    Ok(rows.filter_map(|row| row.ok()).collect())
}

fn extract_legacy_notes(path: &Path) -> Result<Vec<ObolusNoteRecord>> {
    let conn = open_db(path)?;
    let mut stmt = conn.prepare(
        "SELECT note.Z_PK,
                COALESCE(note.ZTITLE, ''),
                COALESCE(body.ZCONTENT, note.ZSUMMARY, ''),
                note.ZCREATIONDATE,
                note.ZMODIFICATIONDATE,
                COALESCE(note.ZDELETEDFLAG, 0)
         FROM ZNOTE note
         LEFT JOIN ZNOTEBODY body ON body.ZOWNER = note.Z_PK
         ORDER BY note.ZMODIFICATIONDATE DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let title: String = row.get(1)?;
        let body_html: String = row.get(2)?;
        let created: Option<f64> = row.get(3)?;
        let modified: Option<f64> = row.get(4)?;
        let deleted: i64 = row.get(5)?;
        Ok(ObolusNoteRecord {
            id: format!("legacy:{id}"),
            title,
            body: html_to_text(&body_html),
            source: "legacy_notes".to_string(),
            source_path: path.display().to_string(),
            created: apple_time_to_rfc3339(created),
            modified: apple_time_to_rfc3339(modified),
            deleted: deleted != 0,
            locked: false,
        })
    })?;
    Ok(rows.filter_map(|row| row.ok()).collect())
}

fn open_db(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("opening sqlite {}", path.display()))
}

fn decode_note_blob(data: &[u8]) -> Option<String> {
    if data.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(data);
        let mut output = Vec::new();
        if decoder.read_to_end(&mut output).is_ok() {
            return String::from_utf8(output).ok();
        }
    }
    String::from_utf8(data.to_vec()).ok()
}

fn apple_time_to_rfc3339(value: Option<f64>) -> Option<String> {
    let seconds = value?;
    if !seconds.is_finite() {
        return None;
    }
    let unix = seconds + 978_307_200.0;
    let secs = unix.trunc() as i64;
    let nanos = ((unix.fract().abs()) * 1_000_000_000.0).round() as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .map(|dt| dt.to_rfc3339())
}

fn html_to_text(value: &str) -> String {
    let mut text = value
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</div>", "\n")
        .replace("</p>", "\n")
        .replace("</tr>", "\n")
        .replace("</li>", "\n")
        .replace("</td>", "\t");
    let mut stripped = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.drain(..) {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => stripped.push(ch),
            _ => {}
        }
    }
    decode_entities(&stripped)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}

fn normalize_plutus_record(
    source_file: &str,
    source_record_type: &str,
    index: usize,
    value: &serde_json::Value,
) -> Option<ObolusMoneyRecord> {
    let display_text = first_string(
        value,
        &["display_text", "raw_reference", "memo", "raw_payload"],
    )
    .unwrap_or_else(|| compact_json(value));
    if display_text.trim().is_empty() {
        return None;
    }

    Some(ObolusMoneyRecord {
        record_id: first_string(value, &["record_id"])
            .unwrap_or_else(|| format!("obolus:{}:{}", source_file, index)),
        source_file: source_file.to_string(),
        source_record_type: source_record_type.to_string(),
        app: first_string(value, &["app"]),
        observed_at: first_string(
            value,
            &[
                "observed_at_utc",
                "timestamp_utc",
                "observed_date",
                "recorded_at_utc",
            ],
        ),
        amount: first_f64(value, &["amount"]),
        amount_text: first_string(value, &["amount_text"]),
        direction_hint: first_string(
            value,
            &["direction_hint", "payment_orientation", "payment_role"],
        ),
        counterparty: first_string(
            value,
            &["counterparty", "counterparty_name", "counterparty_cashtag"],
        ),
        memo: first_string(value, &["memo"]),
        transaction_type: first_string(
            value,
            &["transaction_type", "activity_item_type", "payment_state"],
        ),
        display_text,
        source_path: first_string(value, &["source_path"]),
    })
}

fn normalize_money_thread_mention(value: &serde_json::Value) -> Option<ObolusThreadMention> {
    if value.get("record_type").and_then(|value| value.as_str()) != Some("message") {
        return None;
    }
    let text = first_string(value, &["text"])?;
    let lower = text.to_lowercase();
    let keyword_hits = MONEY_TERMS
        .iter()
        .filter(|term| lower.contains(*term))
        .map(|term| term.trim().to_string())
        .collect::<Vec<_>>();
    let amount_mentions = extract_amount_mentions(&text);
    if keyword_hits.is_empty() && amount_mentions.is_empty() {
        return None;
    }

    Some(ObolusThreadMention {
        record_id: format!(
            "cerberus_message_{}",
            value
                .get("id")
                .and_then(|value| value.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_else(
                    || first_string(value, &["guid"]).unwrap_or_else(|| "unknown".to_string())
                )
        ),
        message_id: value.get("id").and_then(|value| value.as_i64()),
        thread_id: first_string(value, &["thread_id"]),
        phone_number: first_string(value, &["phone_number"]),
        direction: first_string(value, &["direction"]),
        service: first_string(value, &["service"]),
        message_date_raw: value.get("date").and_then(|value| value.as_i64()),
        keyword_hits,
        amount_mentions,
        text,
    })
}

fn first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn first_f64(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|value| value.as_f64()))
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn extract_amount_mentions(text: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    for token in text.split_whitespace() {
        let trimmed = token.trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | '.' | ';' | ':' | ')' | '(' | '[' | ']' | '"' | '\''
            )
        });
        if trimmed.starts_with('$') && trimmed.len() > 1 {
            mentions.push(trimmed.to_string());
            continue;
        }
        let digits = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
        if digits > 0
            && trimmed.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            && (lower_neighbor_money(text, trimmed) || trimmed.contains('.'))
        {
            mentions.push(trimmed.to_string());
        }
    }
    mentions.sort();
    mentions.dedup();
    mentions
}

fn lower_neighbor_money(text: &str, needle: &str) -> bool {
    let lower = text.to_lowercase();
    let Some(index) = lower.find(&needle.to_lowercase()) else {
        return false;
    };
    let start = index.saturating_sub(24);
    let end = (index + needle.len() + 24).min(lower.len());
    let start = floor_char_boundary(&lower, start);
    let end = ceil_char_boundary(&lower, end);
    let window = &lower[start..end];
    [
        "cash", "pay", "paid", "bank", "card", "invoice", "check", "transfer", "fee", "dollar",
        "owe",
    ]
    .iter()
    .any(|term| window.contains(term))
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn write_money_index_csv(path: &Path, records: &[ObolusMoneyRecord]) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    for record in records {
        writer.serialize(record)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_notes_csv(path: &Path, records: &[ObolusNoteRecord]) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    for record in records {
        writer.serialize(record)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_thread_mentions_csv(path: &Path, records: &[ObolusThreadMention]) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "record_id",
        "message_id",
        "thread_id",
        "phone_number",
        "direction",
        "service",
        "message_date_raw",
        "keyword_hits",
        "amount_mentions",
        "text",
    ])?;
    for record in records {
        writer.write_record([
            record.record_id.clone(),
            record
                .message_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.thread_id.clone().unwrap_or_default(),
            record.phone_number.clone().unwrap_or_default(),
            record.direction.clone().unwrap_or_default(),
            record.service.clone().unwrap_or_default(),
            record
                .message_date_raw
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.keyword_hits.join("; "),
            record.amount_mentions.join("; "),
            record.text.clone(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

fn write_index_html(
    path: &Path,
    case_name: &str,
    report: &ObolusReport,
    notes: &[ObolusNoteRecord],
    money_records: &[ObolusMoneyRecord],
    thread_mentions: &[ObolusThreadMention],
) -> Result<()> {
    let mut html = String::new();
    html.push_str(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Obolus Financial Review</title>",
    );
    html.push_str("<style>body{font-family:system-ui,sans-serif;margin:24px;color:#1f2937}table{border-collapse:collapse;width:100%;margin-top:16px}th,td{border-bottom:1px solid #ddd;padding:6px 8px;text-align:left;vertical-align:top}th{background:#f3f4f6}.num{text-align:right}.metric{display:inline-block;margin:0 12px 12px 0;padding:10px 12px;border:1px solid #ddd;border-radius:6px}.preview{max-width:760px}pre{white-space:pre-wrap;font-family:inherit;margin:0}</style></head><body>");
    html.push_str(&format!(
        "<h1>Obolus Financial Review - {}</h1>",
        html_escape(case_name)
    ));
    html.push_str(&format!(
        "<div class=\"metric\"><strong>{}</strong><br>Plutus Records</div>",
        report.plutus_records
    ));
    html.push_str(&format!(
        "<div class=\"metric\"><strong>{}</strong><br>Notes</div>",
        report.notes
    ));
    html.push_str(&format!(
        "<div class=\"metric\"><strong>{}</strong><br>Thread Mentions</div>",
        report.thread_mentions
    ));
    html.push_str(&format!(
        "<div class=\"metric\"><strong>{}</strong><br>Amount Mentions</div>",
        report.amount_mentions
    ));

    html.push_str("<h2>Source Breakdown</h2><table><thead><tr><th>Source</th><th class=\"num\">Count</th></tr></thead><tbody>");
    for (source, count) in &report.source_breakdown {
        html.push_str(&format!(
            "<tr><td>{}</td><td class=\"num\">{}</td></tr>",
            html_escape(source),
            count
        ));
    }
    html.push_str("</tbody></table>");

    html.push_str("<h2>Notes</h2><table><thead><tr><th>Created</th><th>Modified</th><th>Title</th><th>Full Body</th><th>Source</th></tr></thead><tbody>");
    for note in notes {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"preview\"><pre>{}</pre></td><td>{}</td></tr>",
            html_escape(note.created.as_deref().unwrap_or("")),
            html_escape(note.modified.as_deref().unwrap_or("")),
            html_escape(&note.title),
            html_escape(&note.body),
            html_escape(&note.source)
        ));
    }
    html.push_str("</tbody></table>");

    html.push_str("<h2>Money Index</h2><table><thead><tr><th>Observed</th><th>Type</th><th>Amount</th><th>Counterparty</th><th>Text</th></tr></thead><tbody>");
    for record in money_records.iter().take(500) {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"preview\">{}</td></tr>",
            html_escape(record.observed_at.as_deref().unwrap_or("")),
            html_escape(&record.source_record_type),
            html_escape(
                &record
                    .amount_text
                    .clone()
                    .or_else(|| record.amount.map(|amount| format!("{amount:.2}")))
                    .unwrap_or_default()
            ),
            html_escape(record.counterparty.as_deref().unwrap_or("")),
            html_escape(&record.display_text)
        ));
    }
    html.push_str("</tbody></table>");

    html.push_str("<h2>Financial Thread Mentions</h2><table><thead><tr><th>Message</th><th>Direction</th><th>Phone</th><th>Hits</th><th>Text</th></tr></thead><tbody>");
    for mention in thread_mentions.iter().take(500) {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"preview\">{}</td></tr>",
            mention
                .message_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            html_escape(mention.direction.as_deref().unwrap_or("")),
            html_escape(mention.phone_number.as_deref().unwrap_or("")),
            html_escape(&mention.keyword_hits.join(", ")),
            html_escape(&mention.text)
        ));
    }
    html.push_str("</tbody></table></body></html>");
    fs::write(path, html).with_context(|| format!("writing {}", path.display()))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
