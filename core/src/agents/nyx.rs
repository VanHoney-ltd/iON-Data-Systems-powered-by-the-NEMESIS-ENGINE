//! Nyx - web, privacy, and browser-surface extraction.

use anyhow::{Context, Result};
use chrono::{Duration, TimeZone, Utc};
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
use crate::common::target::{safari_bookmarks_target, safari_history_target, safari_tabs_target};
use crate::evidence::EvidenceRecord;

pub struct NyxAgent;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WebActivityRecord {
    record_id: String,
    source: String,
    source_path: String,
    event_type: String,
    timestamp_utc: Option<String>,
    url: Option<String>,
    domain: Option<String>,
    title: Option<String>,
    search_engine: Option<String>,
    search_query: Option<String>,
    load_successful: Option<bool>,
    is_deleted: bool,
    is_private_or_sensitive: bool,
    risk_level: String,
    risk_tags: Vec<String>,
    raw: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NyxFindingRecord {
    record_id: String,
    finding_type: String,
    severity: String,
    title: String,
    detail: String,
    related_record_id: Option<String>,
    url: Option<String>,
    domain: Option<String>,
    timestamp_utc: Option<String>,
    tags: Vec<String>,
}

impl Agent for NyxAgent {
    const NAME: &'static str = "Nyx";
    const SLUG: &'static str = "nyx";
    const SCHEMA_VERSION: u32 = 2;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let evidence_dir = ctx.case.evidence_path("nyx");
        fs::create_dir_all(&evidence_dir)?;

        let mut web_activity = Vec::new();
        let mut warnings = Vec::new();

        match extract_history(ctx) {
            Ok(mut records) => web_activity.append(&mut records),
            Err(err) => warnings.push(format!("Safari history extraction skipped: {err}")),
        }
        match extract_bookmarks(ctx, false) {
            Ok(mut records) => web_activity.append(&mut records),
            Err(err) => warnings.push(format!("Safari bookmarks extraction skipped: {err}")),
        }
        match extract_bookmarks(ctx, true) {
            Ok(mut records) => web_activity.append(&mut records),
            Err(err) => warnings.push(format!(
                "Safari deleted bookmarks extraction skipped: {err}"
            )),
        }
        match extract_tabs(ctx) {
            Ok(mut records) => web_activity.append(&mut records),
            Err(err) => warnings.push(format!("Safari tabs extraction skipped: {err}")),
        }
        match scan_browser_state(ctx) {
            Ok(mut records) => web_activity.append(&mut records),
            Err(err) => warnings.push(format!("Safari browser-state scan skipped: {err}")),
        }

        web_activity.sort_by(|left, right| {
            right
                .timestamp_utc
                .cmp(&left.timestamp_utc)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });
        let findings = build_findings(&web_activity);
        let domains = build_domain_summary(&web_activity);
        let searches: Vec<WebActivityRecord> = web_activity
            .iter()
            .filter(|record| record.search_query.is_some())
            .cloned()
            .collect();

        export_json(&evidence_dir.join("web_activity.json"), &web_activity)?;
        export_web_activity_csv(&evidence_dir.join("web_activity.csv"), &web_activity)?;
        export_json(&evidence_dir.join("searches.json"), &searches)?;
        export_web_activity_csv(&evidence_dir.join("searches.csv"), &searches)?;
        export_json(&evidence_dir.join("domain_summary.json"), &domains)?;
        export_domain_summary_csv(&evidence_dir.join("domain_summary.csv"), &domains)?;
        export_json(&evidence_dir.join("privacy_findings.json"), &findings)?;
        export_findings_csv(&evidence_dir.join("privacy_findings.csv"), &findings)?;
        export_index_html(
            ctx.case.name(),
            &evidence_dir.join("index.html"),
            &web_activity,
            &findings,
            &domains,
        )?;

        let deleted_count = web_activity
            .iter()
            .filter(|record| record.is_deleted)
            .count();
        let sensitive_count = web_activity
            .iter()
            .filter(|record| record.is_private_or_sensitive)
            .count();
        let report = json!({
            "case_id": ctx.case.name(),
            "state": "complete",
            "schema_version": Self::SCHEMA_VERSION,
            "web_activity_records": web_activity.len(),
            "search_records": searches.len(),
            "domains_observed": domains.len(),
            "privacy_findings": findings.len(),
            "deleted_records": deleted_count,
            "sensitive_records": sensitive_count,
            "warnings": warnings,
        });
        export_json(&evidence_dir.join("report.json"), &report)?;

        let mut evidence_records = Vec::new();
        evidence_records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            payload: report,
        });
        for record in web_activity {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "web_activity".to_string(),
                timestamp: record
                    .timestamp_utc
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(record)?,
            });
        }
        for finding in findings {
            evidence_records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "privacy_finding".to_string(),
                timestamp: finding
                    .timestamp_utc
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(finding)?,
            });
        }

        Ok(evidence_records)
    }
}

fn extract_history(ctx: &AgentCtx) -> Result<Vec<WebActivityRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = safari_history_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(value) => value,
        None => return Ok(Vec::new()),
    };
    write_target_audit(
        &resolver,
        &target,
        resolved.source_path.display().to_string(),
        "history",
    )?;
    let prepared = prepare_artifact(&prepare_context(ctx), &resolved, target.sqlite_like)?;
    let conn = prepared.open_sqlite_ro()?;

    let mut records = Vec::new();
    let query = r#"
        SELECT hv.id, hi.url, hv.title, hv.visit_time, hv.load_successful,
               hv.synthesized, hv.origin, hi.visit_count, hi.status_code
        FROM history_visits hv
        JOIN history_items hi ON hv.history_item = hi.id
        ORDER BY hv.visit_time DESC
    "#;
    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
        ))
    })?;

    for row in rows.filter_map(|row| row.ok()) {
        let (
            id,
            url,
            title,
            visit_time,
            load_successful,
            synthesized,
            origin,
            visit_count,
            status_code,
        ) = row;
        let timestamp_utc = safari_time_to_rfc3339(visit_time);
        records.push(web_record(
            format!("safari_history_{id}"),
            "Safari History",
            &prepared.source_path,
            "history_visit",
            timestamp_utc,
            Some(url),
            title,
            false,
            json!({
                "id": id,
                "visit_time": visit_time,
                "load_successful": load_successful != 0,
                "synthesized": synthesized != 0,
                "origin": origin,
                "visit_count": visit_count,
                "status_code": status_code,
            }),
        ));
    }

    records.extend(extract_history_tombstones(&conn, &prepared.source_path)?);
    Ok(records)
}

fn extract_history_tombstones(
    conn: &Connection,
    source_path: &Path,
) -> Result<Vec<WebActivityRecord>> {
    let mut stmt = match conn.prepare("SELECT id, start_time, end_time, url, generation FROM history_tombstones ORDER BY end_time DESC") {
        Ok(stmt) => stmt,
        Err(_) => return Ok(Vec::new()),
    };
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut records = Vec::new();
    for row in rows.filter_map(|row| row.ok()) {
        let (id, start_time, end_time, url, generation) = row;
        let mut record = web_record(
            format!("safari_history_tombstone_{id}"),
            "Safari History Tombstone",
            source_path,
            "history_deleted_window",
            safari_time_to_rfc3339(end_time),
            url,
            Some("Deleted Safari history window".to_string()),
            true,
            json!({"id": id, "start_time": start_time, "end_time": end_time, "generation": generation}),
        );
        record.is_deleted = true;
        record.risk_tags.push("deleted_history".to_string());
        records.push(record);
    }
    Ok(records)
}

fn extract_bookmarks(ctx: &AgentCtx, deleted_only: bool) -> Result<Vec<WebActivityRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = safari_bookmarks_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(value) => value,
        None => return Ok(Vec::new()),
    };
    write_target_audit(
        &resolver,
        &target,
        resolved.source_path.display().to_string(),
        "bookmarks",
    )?;
    let prepared = prepare_artifact(&prepare_context(ctx), &resolved, target.sqlite_like)?;
    let conn = prepared.open_sqlite_ro()?;
    extract_bookmarks_from_conn(
        &conn,
        &prepared.source_path,
        "Safari Bookmarks",
        deleted_only,
    )
}

fn extract_tabs(ctx: &AgentCtx) -> Result<Vec<WebActivityRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = safari_tabs_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(value) => value,
        None => return Ok(Vec::new()),
    };
    write_target_audit(
        &resolver,
        &target,
        resolved.source_path.display().to_string(),
        "tabs",
    )?;
    let prepared = prepare_artifact(&prepare_context(ctx), &resolved, target.sqlite_like)?;
    let conn = prepared.open_sqlite_ro()?;

    let mut records =
        extract_bookmarks_from_conn(&conn, &prepared.source_path, "Safari Tabs", false)?;
    for record in &mut records {
        record.source = "Safari Tabs".to_string();
        if record.event_type == "bookmark" {
            record.event_type = "open_tab".to_string();
        }
    }
    Ok(records)
}

fn extract_bookmarks_from_conn(
    conn: &Connection,
    source_path: &Path,
    source: &str,
    deleted_only: bool,
) -> Result<Vec<WebActivityRecord>> {
    let deleted_filter = if deleted_only {
        "deleted != 0"
    } else {
        "deleted = 0"
    };
    let query = format!(
        "SELECT id, title, url, parent, type, order_index, special_id, added, last_modified, deleted, hidden, date_closed, archive_status, web_filter_status FROM bookmarks WHERE {deleted_filter} ORDER BY parent, order_index"
    );
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<f64>>(7)?,
            row.get::<_, Option<f64>>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, Option<f64>>(11)?,
            row.get::<_, Option<i64>>(12)?,
            row.get::<_, Option<i64>>(13)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|row| row.ok()) {
        let (
            id,
            title,
            url,
            parent,
            btype,
            order_index,
            special_id,
            added,
            last_modified,
            deleted,
            hidden,
            date_closed,
            archive_status,
            web_filter_status,
        ) = row;
        let event_type = if deleted != 0 {
            "deleted_bookmark"
        } else if source.contains("Tabs") && btype == Some(0) {
            "open_tab"
        } else if btype == Some(0) {
            "bookmark"
        } else {
            "bookmark_folder"
        };
        let timestamp = last_modified
            .or(added)
            .or(date_closed)
            .and_then(safari_time_to_rfc3339);
        let mut record = web_record(
            format!("{}_{}", safe_id(source), id),
            source,
            source_path,
            event_type,
            timestamp,
            url,
            title,
            deleted != 0,
            json!({
                "id": id,
                "parent": parent,
                "type": btype,
                "order_index": order_index,
                "special_id": special_id,
                "added": added,
                "last_modified": last_modified,
                "deleted": deleted != 0,
                "hidden": hidden.unwrap_or(0) != 0,
                "date_closed": date_closed,
                "archive_status": archive_status,
                "web_filter_status": web_filter_status,
            }),
        );
        if hidden.unwrap_or(0) != 0 {
            record.risk_tags.push("hidden".to_string());
        }
        records.push(record);
    }
    Ok(records)
}

fn scan_browser_state(ctx: &AgentCtx) -> Result<Vec<WebActivityRecord>> {
    let root = backup_root(ctx).join("HomeDomain/Library/Safari");
    let db_path = root.join("BrowserState.db");
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let conn = open_db_ro(&db_path)?;
    let mut records = Vec::new();
    let tables = table_names(&conn)?;
    for table in tables {
        if table.starts_with("sqlite_") || !is_safe_sql_identifier(&table) {
            continue;
        }
        let query = format!("SELECT * FROM '{}'", table.replace('\'', "''"));
        let mut stmt = match conn.prepare(&query) {
            Ok(stmt) => stmt,
            Err(_) => continue,
        };
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|idx| stmt.column_name(idx).unwrap_or("unknown").to_string())
            .collect();
        let rows = match stmt.query_map([], |row| {
            let mut map = serde_json::Map::new();
            for idx in 0..col_count {
                let value = row.get::<_, SqlValue>(idx).ok();
                map.insert(col_names[idx].clone(), sqlite_json(value));
            }
            Ok(serde_json::Value::Object(map))
        }) {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        for (idx, row) in rows.filter_map(|row| row.ok()).enumerate() {
            let text = row.to_string();
            let Some(url) = first_url_in_text(&text) else {
                continue;
            };
            records.push(web_record(
                format!("browser_state_{}_{}", table, idx),
                "Safari BrowserState",
                &db_path,
                "browser_state_url",
                None,
                Some(url),
                None,
                false,
                json!({"table": table, "row_index": idx, "row": row}),
            ));
        }
    }
    Ok(records)
}

fn web_record(
    record_id: String,
    source: &str,
    source_path: &Path,
    event_type: &str,
    timestamp_utc: Option<String>,
    url: Option<String>,
    title: Option<String>,
    is_deleted: bool,
    raw: serde_json::Value,
) -> WebActivityRecord {
    let domain = url.as_deref().and_then(extract_domain);
    let (search_engine, search_query) = url
        .as_deref()
        .and_then(extract_search)
        .unwrap_or((None, None));
    let mut risk_tags = classify_risk(url.as_deref(), title.as_deref(), search_query.as_deref());
    if is_deleted {
        risk_tags.push("deleted".to_string());
    }
    let is_private_or_sensitive = !risk_tags.is_empty();
    let risk_level = risk_level(&risk_tags).to_string();

    WebActivityRecord {
        record_id,
        source: source.to_string(),
        source_path: source_path.display().to_string(),
        event_type: event_type.to_string(),
        timestamp_utc,
        url,
        domain,
        title,
        search_engine,
        search_query,
        load_successful: raw.get("load_successful").and_then(|value| value.as_bool()),
        is_deleted,
        is_private_or_sensitive,
        risk_level,
        risk_tags,
        raw,
    }
}

fn build_findings(records: &[WebActivityRecord]) -> Vec<NyxFindingRecord> {
    let mut findings = Vec::new();
    for record in records {
        if record.is_deleted {
            findings.push(NyxFindingRecord {
                record_id: format!("nyx_finding_{}", findings.len()),
                finding_type: "deleted_web_record".to_string(),
                severity: "medium".to_string(),
                title: "Deleted browser record".to_string(),
                detail: "Safari retained a deleted bookmark/history tombstone or deleted browser record.".to_string(),
                related_record_id: Some(record.record_id.clone()),
                url: record.url.clone(),
                domain: record.domain.clone(),
                timestamp_utc: record.timestamp_utc.clone(),
                tags: record.risk_tags.clone(),
            });
        }
        if record.is_private_or_sensitive && !record.risk_tags.iter().all(|tag| tag == "deleted") {
            findings.push(NyxFindingRecord {
                record_id: format!("nyx_finding_{}", findings.len()),
                finding_type: "sensitive_web_activity".to_string(),
                severity: record.risk_level.clone(),
                title: "Sensitive web activity signal".to_string(),
                detail: format!("Matched tags: {}", record.risk_tags.join(", ")),
                related_record_id: Some(record.record_id.clone()),
                url: record.url.clone(),
                domain: record.domain.clone(),
                timestamp_utc: record.timestamp_utc.clone(),
                tags: record.risk_tags.clone(),
            });
        }
    }
    findings
}

#[derive(Debug, Serialize, Deserialize)]
struct DomainSummaryRecord {
    domain: String,
    count: usize,
    first_seen_utc: Option<String>,
    last_seen_utc: Option<String>,
    risk_tags: Vec<String>,
}

fn build_domain_summary(records: &[WebActivityRecord]) -> Vec<DomainSummaryRecord> {
    let mut map: BTreeMap<String, DomainSummaryRecord> = BTreeMap::new();
    for record in records {
        let Some(domain) = record.domain.clone() else {
            continue;
        };
        let entry = map.entry(domain.clone()).or_insert(DomainSummaryRecord {
            domain,
            count: 0,
            first_seen_utc: None,
            last_seen_utc: None,
            risk_tags: Vec::new(),
        });
        entry.count += 1;
        if let Some(ts) = &record.timestamp_utc {
            if entry
                .first_seen_utc
                .as_ref()
                .map(|old| ts < old)
                .unwrap_or(true)
            {
                entry.first_seen_utc = Some(ts.clone());
            }
            if entry
                .last_seen_utc
                .as_ref()
                .map(|old| ts > old)
                .unwrap_or(true)
            {
                entry.last_seen_utc = Some(ts.clone());
            }
        }
        for tag in &record.risk_tags {
            if !entry.risk_tags.contains(tag) {
                entry.risk_tags.push(tag.clone());
            }
        }
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.domain.cmp(&right.domain))
    });
    out
}

fn classify_risk(
    url: Option<&str>,
    title: Option<&str>,
    search_query: Option<&str>,
) -> Vec<String> {
    let haystack = format!(
        "{} {} {}",
        url.unwrap_or_default(),
        title.unwrap_or_default(),
        search_query.unwrap_or_default()
    )
    .to_ascii_lowercase();
    let mut tags = Vec::new();
    let groups = [
        (
            "financial",
            [
                "bank",
                "cashapp",
                "cash.app",
                "paypal",
                "venmo",
                "chime",
                "varo",
                "moneylion",
                "onepay",
                "credit",
                "loan",
                "debit",
                "routing",
                "account",
            ]
            .as_slice(),
        ),
        (
            "legal",
            [
                "court",
                "docket",
                "police",
                "attorney",
                "lawyer",
                "jail",
                "prison",
                "warrant",
                "probation",
                "case search",
            ]
            .as_slice(),
        ),
        (
            "identity",
            [
                "ssn",
                "social security",
                "background check",
                "people search",
                "spokeo",
                "truthfinder",
                "beenverified",
            ]
            .as_slice(),
        ),
        (
            "location",
            ["maps", "directions", "address", "near me", "gps"].as_slice(),
        ),
        (
            "credential",
            [
                "login",
                "password",
                "signin",
                "2fa",
                "reset password",
                "account recovery",
            ]
            .as_slice(),
        ),
        ("adult", ["onlyfans", "porn", "adult"].as_slice()),
        (
            "health",
            [
                "doctor",
                "clinic",
                "hospital",
                "medication",
                "mental health",
                "therapy",
            ]
            .as_slice(),
        ),
    ];
    for (tag, terms) in groups {
        if terms.iter().any(|term| haystack.contains(term)) {
            tags.push(tag.to_string());
        }
    }
    tags
}

fn risk_level(tags: &[String]) -> &'static str {
    if tags
        .iter()
        .any(|tag| matches!(tag.as_str(), "credential" | "identity" | "legal"))
    {
        "high"
    } else if !tags.is_empty() {
        "medium"
    } else {
        "none"
    }
}

fn extract_domain(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let host = without_scheme.split(['/', '?', '#']).next()?.trim();
    if host.is_empty() {
        return None;
    }
    Some(host.trim_start_matches("www.").to_ascii_lowercase())
}

fn extract_search(url: &str) -> Option<(Option<String>, Option<String>)> {
    let domain = extract_domain(url)?;
    let engine = if domain.contains("google.") {
        "Google"
    } else if domain.contains("bing.") {
        "Bing"
    } else if domain.contains("duckduckgo.") {
        "DuckDuckGo"
    } else if domain.contains("yahoo.") {
        "Yahoo"
    } else if domain.contains("youtube.") || domain.contains("youtu.be") {
        "YouTube"
    } else {
        return None;
    };
    let query = query_param(url, "q")
        .or_else(|| query_param(url, "p"))
        .or_else(|| query_param(url, "search_query"));
    Some((Some(engine.to_string()), query))
}

fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    for part in query.split('&') {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        if key == name {
            return Some(percent_decode(value.replace('+', " ").as_str()));
        }
    }
    None
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[idx + 1..idx + 3], 16) {
                out.push(hex);
                idx += 3;
                continue;
            }
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn safari_time_to_rfc3339(raw: f64) -> Option<String> {
    if !raw.is_finite() || raw <= 60.0 {
        return None;
    }

    let unix = raw + 978_307_200.0;
    let secs = unix.trunc() as i64;
    let nanos = ((unix.fract()) * 1_000_000_000.0).round() as u32;
    let dt = Utc.timestamp_opt(secs, nanos).single()?;
    let min = Utc.with_ymd_and_hms(2007, 1, 1, 0, 0, 0).single()?;
    let max = Utc::now() + Duration::days(30);
    if dt < min || dt > max {
        return None;
    }

    Some(dt.to_rfc3339())
}

fn prepare_context(ctx: &AgentCtx) -> PrepareContext {
    PrepareContext {
        case_root: ctx.case.root_path(),
        clean_root: ctx.case.root_path().join("clean").join("nyx"),
        temp_root: ctx.case.root_path().join("temp").join("nyx"),
    }
}

fn backup_root(ctx: &AgentCtx) -> PathBuf {
    if ctx.backup_root.exists() {
        ctx.backup_root.clone()
    } else {
        ctx.case.backup_path()
    }
}

fn write_target_audit(
    resolver: &BackupResolver,
    target: &crate::common::target::KnownTarget,
    chosen: String,
    method: &str,
) -> Result<()> {
    crate::common::resolver::write_resolver_audit(
        resolver.case_root(),
        &ResolverAuditRecord {
            artifact_key: target.artifact_key.to_string(),
            candidates: target
                .candidates
                .iter()
                .map(|candidate| format!("{}/{}", candidate.domain, candidate.relative_path))
                .collect(),
            chosen: Some(chosen),
            method: Some(method.to_string()),
        },
    )
}

fn open_db_ro(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))
}

fn table_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|row| row.ok())
        .collect();
    Ok(names)
}

fn sqlite_json(value: Option<SqlValue>) -> serde_json::Value {
    match value {
        Some(SqlValue::Null) | None => serde_json::Value::Null,
        Some(SqlValue::Integer(value)) => value.into(),
        Some(SqlValue::Real(value)) => json!(value),
        Some(SqlValue::Text(value)) => value.into(),
        Some(SqlValue::Blob(bytes)) => String::from_utf8_lossy(&bytes).to_string().into(),
    }
}

fn first_url_in_text(text: &str) -> Option<String> {
    let start = text.find("http://").or_else(|| text.find("https://"))?;
    let rest = &text[start..];
    let end = rest
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\\' | '<' | '>'))
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

fn is_safe_sql_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn export_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn export_web_activity_csv(path: &Path, records: &[WebActivityRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "record_id",
        "source",
        "event_type",
        "timestamp_utc",
        "url",
        "domain",
        "title",
        "search_engine",
        "search_query",
        "load_successful",
        "is_deleted",
        "is_private_or_sensitive",
        "risk_level",
        "risk_tags",
        "source_path",
        "raw",
    ])?;
    for record in records {
        wtr.write_record([
            record.record_id.clone(),
            record.source.clone(),
            record.event_type.clone(),
            record.timestamp_utc.clone().unwrap_or_default(),
            record.url.clone().unwrap_or_default(),
            record.domain.clone().unwrap_or_default(),
            record.title.clone().unwrap_or_default(),
            record.search_engine.clone().unwrap_or_default(),
            record.search_query.clone().unwrap_or_default(),
            record
                .load_successful
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.is_deleted.to_string(),
            record.is_private_or_sensitive.to_string(),
            record.risk_level.clone(),
            record.risk_tags.join(";"),
            record.source_path.clone(),
            serde_json::to_string(&record.raw)?,
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn export_findings_csv(path: &Path, records: &[NyxFindingRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "record_id",
        "finding_type",
        "severity",
        "title",
        "detail",
        "related_record_id",
        "url",
        "domain",
        "timestamp_utc",
        "tags",
    ])?;
    for record in records {
        wtr.write_record([
            record.record_id.clone(),
            record.finding_type.clone(),
            record.severity.clone(),
            record.title.clone(),
            record.detail.clone(),
            record.related_record_id.clone().unwrap_or_default(),
            record.url.clone().unwrap_or_default(),
            record.domain.clone().unwrap_or_default(),
            record.timestamp_utc.clone().unwrap_or_default(),
            record.tags.join(";"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn export_domain_summary_csv(path: &Path, records: &[DomainSummaryRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "domain",
        "count",
        "first_seen_utc",
        "last_seen_utc",
        "risk_tags",
    ])?;
    for record in records {
        wtr.write_record([
            record.domain.clone(),
            record.count.to_string(),
            record.first_seen_utc.clone().unwrap_or_default(),
            record.last_seen_utc.clone().unwrap_or_default(),
            record.risk_tags.join(";"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn export_index_html(
    case_name: &str,
    path: &Path,
    records: &[WebActivityRecord],
    findings: &[NyxFindingRecord],
    domains: &[DomainSummaryRecord],
) -> Result<()> {
    let top_domains = domains
        .iter()
        .take(25)
        .map(|domain| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&domain.domain),
                domain.count,
                escape_html(&domain.risk_tags.join("; "))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let finding_rows = findings
        .iter()
        .take(100)
        .map(|finding| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&finding.severity),
                escape_html(&finding.finding_type),
                escape_html(finding.domain.as_deref().unwrap_or("")),
                escape_html(&finding.detail)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Nyx {case}</title>
<style>body{{font-family:Arial,sans-serif;margin:24px;color:#1f2933}}table{{border-collapse:collapse;width:100%;margin:16px 0}}td,th{{border:1px solid #d9dee7;padding:6px;font-size:12px;vertical-align:top}}th{{background:#f3f5f8;text-align:left}}.meta{{color:#5c6873}}</style></head><body>
<h1>Nyx Web And Privacy Review</h1><p class="meta">Case: {case} | Web records: {records} | Findings: {findings} | Domains: {domains}</p>
<h2>Top Domains</h2><table><thead><tr><th>Domain</th><th>Count</th><th>Risk Tags</th></tr></thead><tbody>{top_domains}</tbody></table>
<h2>Findings</h2><table><thead><tr><th>Severity</th><th>Type</th><th>Domain</th><th>Detail</th></tr></thead><tbody>{finding_rows}</tbody></table>
</body></html>"#,
        case = escape_html(case_name),
        records = records.len(),
        findings = findings.len(),
        domains = domains.len(),
        top_domains = top_domains,
        finding_rows = finding_rows
    );
    fs::write(path, html)?;
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
