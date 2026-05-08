//! Atlas — GPS/Location extraction agent.
//!
//! Scans backup for location-related SQLite databases from Apple Maps,
//! Google Maps, Snapchat, and payment apps. Extracts location records
//! from matching tables and returns them as EvidenceRecords.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::agents::{Agent, AgentCtx};

use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverContext};
use crate::common::target::{
    apple_maps_cloud_history_target, apple_maps_geo_target, apple_maps_history_target,
    google_maps_target, snapchat_maps_target,
};
use crate::evidence::EvidenceRecord;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationRecord {
    pub source: String,
    pub app_bundle: String,
    pub database_path: String,
    pub table_name: String,
    pub timestamp: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy: Option<f64>,
    pub altitude: Option<f64>,
    pub address: Option<String>,
    pub query: Option<String>,
    #[serde(flatten)]
    pub raw: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationSource {
    pub app: String,
    pub bundle_id: String,
    pub database: String,
    pub records: Vec<LocationRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AtlasReport {
    pub case_name: String,
    pub extracted_at: String,
    pub sources: Vec<LocationSource>,
}

pub struct AtlasAgent;

impl Agent for AtlasAgent {
    const NAME: &'static str = "Atlas";
    const SLUG: &'static str = "atlas";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let resolver = BackupResolver::new(ResolverContext {
            backup_root: ctx.backup_root.clone(),
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            manifest_db_path: ctx
                .backup_root
                .join("Manifest.db")
                .exists()
                .then(|| ctx.backup_root.join("Manifest.db")),
        });

        let mut all_sources = Vec::new();

        match extract_apple_maps(ctx, &resolver) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Apple Maps extraction failed: {}", e),
        }

        match extract_google_maps(ctx, &resolver) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Google Maps extraction failed: {}", e),
        }

        match extract_snapchat(ctx, &resolver) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Snapchat extraction failed: {}", e),
        }

        match extract_payment_apps(ctx) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Payment apps extraction failed: {}", e),
        }

        let mut records = Vec::new();
        let mut total_locations = 0usize;

        for source in &all_sources {
            for location in &source.records {
                records.push(EvidenceRecord {
                    schema_version: Self::SCHEMA_VERSION,
                    source_agent: Self::NAME.to_string(),
                    record_type: "location".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    payload: serde_json::to_value(location).unwrap_or_else(|_| json!({})),
                });
                total_locations += 1;
            }
        }

        let report = AtlasReport {
            case_name: ctx.case.name().to_string(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
            sources: all_sources,
        };

        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "total_locations": total_locations,
                "total_sources": report.sources.len(),
                "extracted_at": report.extracted_at,
            }),
        });

        Ok(records)
    }
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

fn sanitize_value(val: SqlValue) -> serde_json::Value {
    match val {
        SqlValue::Null => serde_json::Value::Null,
        SqlValue::Integer(i) => serde_json::Value::Number(i.into()),
        SqlValue::Real(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        SqlValue::Text(s) => serde_json::Value::String(s),
        SqlValue::Blob(b) => serde_json::Value::String(format!("<bytes {}>", b.len())),
    }
}

fn is_location_table(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("location")
        || lower.contains("geo")
        || lower.contains("place")
        || lower.contains("history")
        || lower.contains("map")
        || lower.contains("coordinate")
        || lower.contains("merchant")
        || lower.contains("visit")
        || lower.contains("transaction")
        || lower.contains("checkin")
        || lower.contains("position")
}

fn lat_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        (l.contains("lat") && !l.contains("later") && !l.contains("latency") && !l.contains("relat") && !l.contains("platinum") && !l.contains("translat"))
            || l == "latitude"
    }).cloned()
}

fn lon_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("lon") || l.contains("lng") || l == "longitude"
    }).cloned()
}

fn time_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("time") || l.contains("date") || l.contains("timestamp") || l.contains("created") || l.contains("modified")
    }).cloned()
}

fn address_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("address") || l.contains("street") || l.contains("city") || l.contains("place") || l.contains("name") || l.contains("location")
    }).cloned()
}

fn accuracy_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| c.to_lowercase().contains("accuracy")).cloned()
}

fn altitude_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("altitude") || (l.contains("alt") && !l.contains("alert") && !l.contains("alternate"))
    }).cloned()
}

fn query_column(cols: &[String]) -> Option<String> {
    cols.iter().find(|c| {
        let l = c.to_lowercase();
        l.contains("query") || l.contains("search") || l.contains("keyword") || l.contains("term")
    }).cloned()
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

fn list_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    if !is_safe_sql_identifier(table) {
        anyhow::bail!("Invalid table name: {}", table);
    }
    let mut stmt = conn.prepare(&format!("PRAGMA table_info('{}')", table))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn extract_from_table(
    conn: &Connection,
    table: &str,
    db_path: &str,
    app_name: &str,
    bundle_id: &str,
) -> Result<Vec<LocationRecord>> {
    if !is_safe_sql_identifier(table) {
        return Ok(Vec::new());
    }
    let cols = match table_columns(conn, table) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    if cols.is_empty() {
        return Ok(Vec::new());
    }

    let lat_col = lat_column(&cols);
    let lon_col = lon_column(&cols);
    let time_col = time_column(&cols);
    let addr_col = address_column(&cols);
    let acc_col = accuracy_column(&cols);
    let alt_col = altitude_column(&cols);
    let q_col = query_column(&cols);

    let has_location_signal = lat_col.is_some()
        || lon_col.is_some()
        || time_col.is_some()
        || addr_col.is_some()
        || acc_col.is_some()
        || alt_col.is_some()
        || q_col.is_some();
    if !has_location_signal && !is_location_table(table) {
        return Ok(Vec::new());
    }

    let query = format!("SELECT * FROM '{}' LIMIT 5000", table);
    let mut stmt = match conn.prepare(&query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let col_names: Vec<String> = (0..stmt.column_count())
        .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
        .collect();

    let rows = match stmt.query_map([], |row| {
        let mut map = HashMap::new();
        for (idx, name) in col_names.iter().enumerate() {
            let v = match row.get::<_, SqlValue>(idx) {
                Ok(v) => sanitize_value(v),
                Err(_) => serde_json::Value::Null,
            };
            map.insert(name.clone(), v);
        }
        Ok(map)
    }) {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let record = LocationRecord {
            source: app_name.to_string(),
            app_bundle: bundle_id.to_string(),
            database_path: db_path.to_string(),
            table_name: table.to_string(),
            timestamp: time_col.as_ref().and_then(|c| row.get(c).map(|v| value_to_string(v))),
            latitude: lat_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            longitude: lon_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            accuracy: acc_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            altitude: alt_col.as_ref().and_then(|c| row.get(c).and_then(value_to_f64)),
            address: addr_col.as_ref().and_then(|c| row.get(c).map(|v| value_to_string(v))),
            query: q_col.as_ref().and_then(|c| row.get(c).map(|v| value_to_string(v))),
            raw: row,
        };
        records.push(record);
    }
    Ok(records)
}

fn value_to_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => v.to_string(),
    }
}

fn prepare_known_target(
    ctx: &AgentCtx,
    resolver: &BackupResolver,
    target: &crate::common::target::KnownTarget,
) -> Result<Option<PathBuf>> {
    let resolved = match resolver.resolve_known_target(target)? {
        Some(r) => r,
        None => return Ok(None),
    };
    let prep_ctx = PrepareContext {
        case_root: ctx.case.root_path(),
        clean_root: ctx.case.root_path().join("clean"),
        temp_root: ctx.case.root_path().join("tmp"),
    };
    let prepared = prepare_artifact(&prep_ctx, &resolved, target.sqlite_like)?;
    Ok(Some(prepared.working_path))
}

fn extract_apple_maps(ctx: &AgentCtx, resolver: &BackupResolver) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    if let Some(path) = prepare_known_target(ctx, resolver, &apple_maps_history_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Apple Maps", "com.apple.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Apple Maps".to_string(),
                        bundle_id: "com.apple.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Apple Maps history DB: {}", e),
        }
    }

    if let Some(path) = prepare_known_target(ctx, resolver, &apple_maps_cloud_history_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Apple Maps Cloud", "com.apple.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Apple Maps Cloud".to_string(),
                        bundle_id: "com.apple.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Apple Maps cloud history DB: {}", e),
        }
    }

    if let Some(path) = prepare_known_target(ctx, resolver, &apple_maps_geo_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Apple Maps Geo", "com.apple.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Apple Maps Geo".to_string(),
                        bundle_id: "com.apple.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Apple Maps geo DB: {}", e),
        }
    }

    Ok(sources)
}

fn extract_google_maps(ctx: &AgentCtx, resolver: &BackupResolver) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    if let Some(path) = prepare_known_target(ctx, resolver, &google_maps_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Google Maps", "com.google.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Google Maps".to_string(),
                        bundle_id: "com.google.Maps".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Google Maps DB: {}", e),
        }
    }

    let google_domain_prefix = "AppDomain-com.google.Maps";
    let mut found = Vec::new();
    for entry in WalkDir::new(&ctx.backup_root).max_depth(8).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&ctx.backup_root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();
        if rel_str.starts_with(google_domain_prefix) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite" || ext_lower == "db" || ext_lower == "sqlitedb" || ext_lower == "storedata" {
                    if !sources.iter().any(|s| s.database == path.to_string_lossy().to_string()) {
                        found.push(path.to_path_buf());
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();

    for db_path in found {
        match open_db_ro(&db_path) {
            Ok(conn) => {
                let tables = match list_tables(&conn) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &db_path.to_string_lossy(), "Google Maps", "com.google.Maps") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Google Maps".to_string(),
                        bundle_id: "com.google.Maps".to_string(),
                        database: db_path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(_) => continue,
        }
    }

    Ok(sources)
}

fn extract_snapchat(ctx: &AgentCtx, resolver: &BackupResolver) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    if let Some(path) = prepare_known_target(ctx, resolver, &snapchat_maps_target())? {
        match open_db_ro(&path) {
            Ok(conn) => {
                let tables = list_tables(&conn)?;
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &path.to_string_lossy(), "Snapchat", "com.toyopagroup.picaboo") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Snapchat".to_string(),
                        bundle_id: "com.toyopagroup.picaboo".to_string(),
                        database: path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(e) => eprintln!("⚠️  Could not open Snapchat map DB: {}", e),
        }
    }

    let snap_domain_prefix = "AppDomain-com.toyopagroup.picaboo";
    let mut found = Vec::new();
    for entry in WalkDir::new(&ctx.backup_root).max_depth(8).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&ctx.backup_root).unwrap_or(path);
        if rel.to_string_lossy().starts_with(snap_domain_prefix) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite" || ext_lower == "db" || ext_lower == "sqlitedb" || ext_lower == "storedata" {
                    if !sources.iter().any(|s| s.database == path.to_string_lossy().to_string()) {
                        found.push(path.to_path_buf());
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();

    for db_path in found {
        match open_db_ro(&db_path) {
            Ok(conn) => {
                let tables = match list_tables(&conn) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let mut records = Vec::new();
                for table in tables {
                    if let Ok(mut r) = extract_from_table(&conn, &table, &db_path.to_string_lossy(), "Snapchat", "com.toyopagroup.picaboo") {
                        records.append(&mut r);
                    }
                }
                if !records.is_empty() {
                    sources.push(LocationSource {
                        app: "Snapchat".to_string(),
                        bundle_id: "com.toyopagroup.picaboo".to_string(),
                        database: db_path.to_string_lossy().to_string(),
                        records,
                    });
                }
            }
            Err(_) => continue,
        }
    }

    Ok(sources)
}

fn is_banking_domain(domain: &str) -> bool {
    let lower = domain.to_lowercase();
    let keywords = [
        "bank", "chase", "wells", "paypal", "venmo", "cash", "zelle", "square",
        "credit", "debit", "card", "pay", "money", "transfer", "wallet",
        "capitalone", "citi", "bofa", "usbank", "pnc", "truist",
    ];
    keywords.iter().any(|k| lower.contains(k))
}

fn extract_payment_apps(ctx: &AgentCtx) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    for entry in WalkDir::new(&ctx.backup_root).max_depth(1).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_dir() {
            continue;
        }
        let domain_path = entry.path();
        let domain_name = domain_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !domain_name.starts_with("AppDomain-") {
            continue;
        }
        if !is_banking_domain(domain_name) {
            continue;
        }

        let bundle_id = domain_name.strip_prefix("AppDomain-").unwrap_or(domain_name).to_string();
        let app_name = bundle_id.split('.').last().unwrap_or(&bundle_id).to_string();

        let mut db_files = Vec::new();
        for sub in WalkDir::new(domain_path).max_depth(6).into_iter().filter_map(|e| e.ok()) {
            if !sub.file_type().is_file() {
                continue;
            }
            let path = sub.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite" || ext_lower == "db" || ext_lower == "sqlitedb" || ext_lower == "storedata" {
                    db_files.push(path.to_path_buf());
                }
            }
        }

        for db_path in db_files {
            match open_db_ro(&db_path) {
                Ok(conn) => {
                    let tables = match list_tables(&conn) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    let mut records = Vec::new();
                    for table in tables {
                        if let Ok(mut r) = extract_from_table(&conn, &table, &db_path.to_string_lossy(), &app_name, &bundle_id) {
                            records.append(&mut r);
                        }
                    }
                    if !records.is_empty() {
                        sources.push(LocationSource {
                            app: app_name.clone(),
                            bundle_id: bundle_id.clone(),
                            database: db_path.to_string_lossy().to_string(),
                            records,
                        });
                    }
                }
                Err(_) => continue,
            }
        }
    }

    Ok(sources)
}
