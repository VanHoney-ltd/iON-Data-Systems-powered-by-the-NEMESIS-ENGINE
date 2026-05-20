//! Atlas — GPS/Location extraction agent.
//!
//! Scans backup for location-related SQLite databases from Apple Maps,
//! Google Maps, Snapchat, and payment apps. Extracts location records
//! from matching tables and returns them as EvidenceRecords.

use anyhow::{Context, Result};
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::fs;
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AtlasActivityRecord {
    pub source: String,
    pub app_bundle: String,
    pub database_path: String,
    pub table_name: String,
    pub timestamp_raw: Option<String>,
    pub timestamp_utc: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub address: Option<String>,
    pub query: Option<String>,
    pub signal_type: String,
    pub confidence: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AtlasSourceSummary {
    pub source: String,
    pub app_bundle: String,
    pub database_path: String,
    pub total_records: usize,
    pub coordinate_records: usize,
    pub activity_records: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AtlasUiReport {
    pub case_name: String,
    pub generated_at: String,
    pub total_records: usize,
    pub coordinate_records: usize,
    pub activity_records: usize,
    pub sources: Vec<AtlasSourceSummary>,
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

        match extract_prepared_coordinate_sources(ctx, &all_sources) {
            Ok(sources) => all_sources.extend(sources),
            Err(e) => eprintln!("⚠️  Prepared coordinate scan failed: {}", e),
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
                    payload: strip_nulls(
                        serde_json::to_value(location).unwrap_or_else(|_| json!({})),
                    ),
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

        export_atlas_outputs(ctx, &report)?;

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
    cols.iter()
        .find(|c| {
            let l = c.to_lowercase();
            (l.contains("lat")
                && !l.contains("later")
                && !l.contains("latest")
                && !l.contains("latency")
                && !l.contains("relat")
                && !l.contains("relation")
                && !l.contains("violation")
                && !l.contains("template")
                && !l.contains("platform")
                && !l.contains("platinum")
                && !l.contains("translat"))
                || l == "latitude"
        })
        .cloned()
}

fn lon_column(cols: &[String]) -> Option<String> {
    cols.iter()
        .find(|c| {
            let l = c.to_lowercase();
            (l.contains("lon") || l.contains("lng") || l == "longitude")
                && !l.contains("longdescription")
                && !l.contains("long_param")
                && !l.contains("long_data")
        })
        .cloned()
}

fn time_column(cols: &[String]) -> Option<String> {
    cols.iter()
        .find(|c| {
            let l = c.to_lowercase();
            !l.contains("timezone")
                && (l.contains("time")
                    || l.contains("date")
                    || l.contains("timestamp")
                    || l.contains("created")
                    || l.contains("modified"))
        })
        .cloned()
}

fn address_column(cols: &[String]) -> Option<String> {
    cols.iter()
        .find(|c| {
            let l = c.to_lowercase();
            !l.contains("timezone")
                && !l.contains("offset")
                && (l.contains("address")
                    || l.contains("street")
                    || l.contains("city")
                    || l.contains("place")
                    || l.contains("name")
                    || l.contains("location"))
        })
        .cloned()
}

fn accuracy_column(cols: &[String]) -> Option<String> {
    cols.iter()
        .find(|c| c.to_lowercase().contains("accuracy"))
        .cloned()
}

fn altitude_column(cols: &[String]) -> Option<String> {
    cols.iter()
        .find(|c| {
            let l = c.to_lowercase();
            l.contains("altitude")
                || (l.contains("alt") && !l.contains("alert") && !l.contains("alternate"))
        })
        .cloned()
}

fn query_column(cols: &[String]) -> Option<String> {
    cols.iter()
        .find(|c| {
            let l = c.to_lowercase();
            l.contains("query")
                || l.contains("search")
                || l.contains("keyword")
                || l.contains("term")
        })
        .cloned()
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
        let latitude = lat_col
            .as_ref()
            .and_then(|c| row.get(c).and_then(value_to_f64));
        let longitude = lon_col
            .as_ref()
            .and_then(|c| row.get(c).and_then(value_to_f64));
        let (latitude, longitude) = match (latitude, longitude) {
            (Some(lat), Some(lon)) if is_valid_coordinate(lat, lon) => (Some(lat), Some(lon)),
            _ => (None, None),
        };

        let record = LocationRecord {
            source: app_name.to_string(),
            app_bundle: bundle_id.to_string(),
            database_path: db_path.to_string(),
            table_name: table.to_string(),
            timestamp: time_col
                .as_ref()
                .and_then(|c| row.get(c).map(|v| value_to_string(v))),
            latitude,
            longitude,
            accuracy: acc_col
                .as_ref()
                .and_then(|c| row.get(c).and_then(value_to_f64)),
            altitude: alt_col
                .as_ref()
                .and_then(|c| row.get(c).and_then(value_to_f64)),
            address: addr_col
                .as_ref()
                .and_then(|c| row.get(c).map(|v| value_to_string(v))),
            query: q_col
                .as_ref()
                .and_then(|c| row.get(c).map(|v| value_to_string(v))),
            raw: row,
        };
        records.push(record);
    }
    Ok(records)
}

fn is_valid_coordinate(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
        && !(latitude.abs() < f64::EPSILON && longitude.abs() < f64::EPSILON)
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

fn export_atlas_outputs(ctx: &AgentCtx, report: &AtlasReport) -> Result<()> {
    let activities = normalize_atlas_activities(report);
    let coordinates: Vec<AtlasActivityRecord> = activities
        .iter()
        .filter(|record| record.latitude.is_some() && record.longitude.is_some())
        .cloned()
        .collect();
    let summaries = summarize_atlas_sources(report);
    let ui_report = AtlasUiReport {
        case_name: report.case_name.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        total_records: activities.len(),
        coordinate_records: coordinates.len(),
        activity_records: activities.len().saturating_sub(coordinates.len()),
        sources: summaries.clone(),
    };

    fs::create_dir_all(&ctx.evidence_dir)?;
    fs::write(
        ctx.evidence_dir.join("location_activity.json"),
        serde_json::to_vec_pretty(&activities)?,
    )?;
    fs::write(
        ctx.evidence_dir.join("coordinates.json"),
        serde_json::to_vec_pretty(&coordinates)?,
    )?;
    fs::write(
        ctx.evidence_dir.join("source_summary.json"),
        serde_json::to_vec_pretty(&summaries)?,
    )?;
    fs::write(
        ctx.evidence_dir.join("report.json"),
        serde_json::to_vec_pretty(&ui_report)?,
    )?;

    write_atlas_activity_csv(&ctx.evidence_dir.join("location_activity.csv"), &activities)?;
    write_atlas_activity_csv(&ctx.evidence_dir.join("coordinates.csv"), &coordinates)?;
    write_atlas_source_summary_csv(&ctx.evidence_dir.join("source_summary.csv"), &summaries)?;
    write_atlas_index_html(ctx, &ui_report, &activities, &coordinates)?;

    Ok(())
}

fn normalize_atlas_activities(report: &AtlasReport) -> Vec<AtlasActivityRecord> {
    let mut records = Vec::new();
    for source in &report.sources {
        for record in &source.records {
            let has_coordinates = record.latitude.is_some() && record.longitude.is_some();
            records.push(AtlasActivityRecord {
                source: record.source.clone(),
                app_bundle: record.app_bundle.clone(),
                database_path: record.database_path.clone(),
                table_name: record.table_name.clone(),
                timestamp_raw: record.timestamp.clone(),
                timestamp_utc: record.timestamp.as_deref().and_then(normalize_timestamp),
                latitude: record.latitude,
                longitude: record.longitude,
                address: record.address.clone(),
                query: record.query.clone(),
                signal_type: if has_coordinates {
                    "coordinate".to_string()
                } else {
                    "app_activity".to_string()
                },
                confidence: if has_coordinates {
                    "high".to_string()
                } else {
                    "low".to_string()
                },
            });
        }
    }
    records.sort_by(|left, right| {
        left.timestamp_utc
            .cmp(&right.timestamp_utc)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.table_name.cmp(&right.table_name))
    });
    records
}

fn summarize_atlas_sources(report: &AtlasReport) -> Vec<AtlasSourceSummary> {
    let mut summaries = Vec::new();
    for source in &report.sources {
        let coordinate_records = source
            .records
            .iter()
            .filter(|record| record.latitude.is_some() && record.longitude.is_some())
            .count();
        summaries.push(AtlasSourceSummary {
            source: source.app.clone(),
            app_bundle: source.bundle_id.clone(),
            database_path: source.database.clone(),
            total_records: source.records.len(),
            coordinate_records,
            activity_records: source.records.len().saturating_sub(coordinate_records),
        });
    }
    summaries.sort_by(|left, right| {
        right
            .total_records
            .cmp(&left.total_records)
            .then_with(|| left.source.cmp(&right.source))
    });
    summaries
}

fn normalize_timestamp(raw: &str) -> Option<String> {
    let value = raw.parse::<f64>().ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }

    let unix_seconds = if value > 10_000_000_000.0 {
        value / 1000.0
    } else if value > 1_000_000_000.0 {
        value
    } else if value > 100_000_000.0 {
        // Cocoa absolute time starts at 2001-01-01.
        value + 978_307_200.0
    } else {
        return None;
    };

    let seconds = unix_seconds.trunc() as i64;
    let nanos = ((unix_seconds.fract().abs()) * 1_000_000_000.0) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, nanos).map(|dt| dt.to_rfc3339())
}

fn write_atlas_activity_csv(path: &Path, records: &[AtlasActivityRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Source",
        "Bundle ID",
        "Signal Type",
        "Confidence",
        "Timestamp UTC",
        "Timestamp Raw",
        "Latitude",
        "Longitude",
        "Address",
        "Query",
        "Table",
        "Database",
    ])?;
    for record in records {
        wtr.write_record([
            record.source.clone(),
            record.app_bundle.clone(),
            record.signal_type.clone(),
            record.confidence.clone(),
            record.timestamp_utc.clone().unwrap_or_default(),
            record.timestamp_raw.clone().unwrap_or_default(),
            record
                .latitude
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .longitude
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.address.clone().unwrap_or_default(),
            record.query.clone().unwrap_or_default(),
            record.table_name.clone(),
            record.database_path.clone(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_atlas_source_summary_csv(path: &Path, summaries: &[AtlasSourceSummary]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Source",
        "Bundle ID",
        "Total Records",
        "Coordinate Records",
        "Activity Records",
        "Database",
    ])?;
    for summary in summaries {
        wtr.write_record([
            summary.source.clone(),
            summary.app_bundle.clone(),
            summary.total_records.to_string(),
            summary.coordinate_records.to_string(),
            summary.activity_records.to_string(),
            summary.database_path.clone(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_atlas_index_html(
    ctx: &AgentCtx,
    report: &AtlasUiReport,
    activities: &[AtlasActivityRecord],
    coordinates: &[AtlasActivityRecord],
) -> Result<()> {
    let source_rows = report
        .sources
        .iter()
        .map(|source| {
            format!(
                "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>{}</td></tr>",
                html_escape(&source.source),
                html_escape(&source.app_bundle),
                source.total_records,
                source.coordinate_records,
                source.activity_records,
                html_escape(&source.database_path),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let coordinate_rows = coordinates
        .iter()
        .take(500)
        .map(activity_row_html)
        .collect::<Vec<_>>()
        .join("\n");
    let activity_rows = activities
        .iter()
        .take(500)
        .map(activity_row_html)
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Atlas Location Activity - {case}</title>
<style>
body {{ font-family: Arial, sans-serif; margin: 32px; color: #1f2933; background: #f7f9fb; }}
h1 {{ font-size: 22px; margin: 0 0 4px; }}
h2 {{ font-size: 16px; margin: 24px 0 10px; }}
.meta {{ color: #5c6873; margin-bottom: 20px; }}
.summary {{ display: grid; grid-template-columns: repeat(3, minmax(140px, 1fr)); gap: 10px; margin: 18px 0; }}
.metric {{ background: #fff; border: 1px solid #d8dee4; padding: 12px; }}
.metric .label {{ color: #5c6873; font-size: 11px; text-transform: uppercase; }}
.metric .value {{ font-size: 22px; margin-top: 4px; font-weight: 700; }}
table {{ border-collapse: collapse; width: 100%; font-size: 12px; background: #fff; margin-bottom: 16px; }}
th, td {{ border: 1px solid #d8dee4; padding: 6px 8px; text-align: left; vertical-align: top; }}
th {{ background: #eef2f6; }}
td {{ overflow-wrap: anywhere; }}
.num {{ text-align: right; font-variant-numeric: tabular-nums; }}
.badge {{ display: inline-block; padding: 2px 6px; border-radius: 4px; font-size: 11px; font-weight: 700; }}
.coordinate {{ background: #e5f7ed; color: #166534; }}
.app_activity {{ background: #fff1d6; color: #8a5600; }}
</style>
</head>
<body>
<h1>Atlas Location Activity</h1>
<div class="meta">Case: {case} | Generated: {generated_at}</div>
<div class="summary">
  <div class="metric"><div class="label">Total Records</div><div class="value">{total}</div></div>
  <div class="metric"><div class="label">Coordinate Records</div><div class="value">{coordinates}</div></div>
  <div class="metric"><div class="label">App Activity Records</div><div class="value">{activity}</div></div>
</div>
<h2>Sources</h2>
<table>
<thead><tr><th>Source</th><th>Bundle</th><th>Total</th><th>Coordinates</th><th>Activity</th><th>Database</th></tr></thead>
<tbody>{source_rows}</tbody>
</table>
<h2>Coordinate Records</h2>
<table>
<thead><tr><th>Signal</th><th>Source</th><th>Time UTC</th><th>Lat</th><th>Lon</th><th>Address/Query</th><th>Table</th></tr></thead>
<tbody>{coordinate_rows}</tbody>
</table>
<h2>Activity Records</h2>
<table>
<thead><tr><th>Signal</th><th>Source</th><th>Time UTC</th><th>Lat</th><th>Lon</th><th>Address/Query</th><th>Table</th></tr></thead>
<tbody>{activity_rows}</tbody>
</table>
</body>
</html>
"#,
        case = html_escape(ctx.case.name()),
        generated_at = html_escape(&report.generated_at),
        total = report.total_records,
        coordinates = report.coordinate_records,
        activity = report.activity_records,
        source_rows = source_rows,
        coordinate_rows = coordinate_rows,
        activity_rows = activity_rows,
    );
    fs::write(ctx.evidence_dir.join("index.html"), html)?;
    Ok(())
}

fn activity_row_html(record: &AtlasActivityRecord) -> String {
    let label = record
        .address
        .as_deref()
        .or(record.query.as_deref())
        .unwrap_or("");
    format!(
        "<tr><td><span class=\"badge {signal_class}\">{signal}</span></td><td>{source}</td><td>{time}</td><td>{lat}</td><td>{lon}</td><td>{label}</td><td>{table}</td></tr>",
        signal_class = html_escape(&record.signal_type),
        signal = html_escape(&record.signal_type),
        source = html_escape(&record.source),
        time = html_escape(record.timestamp_utc.as_deref().unwrap_or("")),
        lat = record.latitude.map(|value| value.to_string()).unwrap_or_default(),
        lon = record.longitude.map(|value| value.to_string()).unwrap_or_default(),
        label = html_escape(label),
        table = html_escape(&record.table_name),
    )
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &path.to_string_lossy(),
                        "Apple Maps",
                        "com.apple.Maps",
                    ) {
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &path.to_string_lossy(),
                        "Apple Maps Cloud",
                        "com.apple.Maps",
                    ) {
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &path.to_string_lossy(),
                        "Apple Maps Geo",
                        "com.apple.Maps",
                    ) {
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &path.to_string_lossy(),
                        "Google Maps",
                        "com.google.Maps",
                    ) {
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
    for entry in WalkDir::new(&ctx.backup_root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&ctx.backup_root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();
        if rel_str.starts_with(google_domain_prefix) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite"
                    || ext_lower == "db"
                    || ext_lower == "sqlitedb"
                    || ext_lower == "storedata"
                {
                    if !sources
                        .iter()
                        .any(|s| s.database == path.to_string_lossy().to_string())
                    {
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &db_path.to_string_lossy(),
                        "Google Maps",
                        "com.google.Maps",
                    ) {
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &path.to_string_lossy(),
                        "Snapchat",
                        "com.toyopagroup.picaboo",
                    ) {
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
    for entry in WalkDir::new(&ctx.backup_root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&ctx.backup_root).unwrap_or(path);
        if rel.to_string_lossy().starts_with(snap_domain_prefix) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite"
                    || ext_lower == "db"
                    || ext_lower == "sqlitedb"
                    || ext_lower == "storedata"
                {
                    if !sources
                        .iter()
                        .any(|s| s.database == path.to_string_lossy().to_string())
                    {
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
                    if let Ok(mut r) = extract_from_table(
                        &conn,
                        &table,
                        &db_path.to_string_lossy(),
                        "Snapchat",
                        "com.toyopagroup.picaboo",
                    ) {
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
        "bank",
        "chase",
        "wells",
        "paypal",
        "venmo",
        "cash",
        "zelle",
        "square",
        "credit",
        "debit",
        "card",
        "pay",
        "money",
        "transfer",
        "wallet",
        "capitalone",
        "citi",
        "bofa",
        "usbank",
        "pnc",
        "truist",
    ];
    keywords.iter().any(|k| lower.contains(k))
}

fn extract_payment_apps(ctx: &AgentCtx) -> Result<Vec<LocationSource>> {
    let mut sources = Vec::new();

    for entry in WalkDir::new(&ctx.backup_root)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let domain_path = entry.path();
        let domain_name = domain_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if !domain_name.starts_with("AppDomain-") {
            continue;
        }
        if !is_banking_domain(domain_name) {
            continue;
        }

        let bundle_id = domain_name
            .strip_prefix("AppDomain-")
            .unwrap_or(domain_name)
            .to_string();
        let app_name = bundle_id
            .split('.')
            .last()
            .unwrap_or(&bundle_id)
            .to_string();

        let mut db_files = Vec::new();
        for sub in WalkDir::new(domain_path)
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !sub.file_type().is_file() {
                continue;
            }
            let path = sub.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "sqlite"
                    || ext_lower == "db"
                    || ext_lower == "sqlitedb"
                    || ext_lower == "storedata"
                {
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
                        if let Ok(mut r) = extract_from_table(
                            &conn,
                            &table,
                            &db_path.to_string_lossy(),
                            &app_name,
                            &bundle_id,
                        ) {
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

fn extract_prepared_coordinate_sources(
    ctx: &AgentCtx,
    existing_sources: &[LocationSource],
) -> Result<Vec<LocationSource>> {
    let prepared_root = PathBuf::from("prepared")
        .join("helios")
        .join(ctx.case.name());
    if !prepared_root.exists() {
        return Ok(Vec::new());
    }

    let existing_databases = existing_sources
        .iter()
        .map(|source| source.database.clone())
        .collect::<BTreeSet<_>>();
    let mut sources = Vec::new();
    let mut db_files = Vec::new();

    for entry in WalkDir::new(&prepared_root)
        .max_depth(10)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !looks_like_sqlite_path(path) || should_skip_prepared_db(path) {
            continue;
        }
        let display = path.to_string_lossy().to_string();
        if existing_databases.contains(&display) {
            continue;
        }
        db_files.push(path.to_path_buf());
    }

    db_files.sort();
    db_files.dedup();

    for db_path in db_files {
        let Ok(conn) = open_db_ro(&db_path) else {
            continue;
        };
        let Ok(tables) = list_tables(&conn) else {
            continue;
        };

        let mut records = Vec::new();
        for table in tables {
            let Ok(cols) = table_columns(&conn, &table) else {
                continue;
            };
            if should_skip_coordinate_table(&table) {
                continue;
            }
            if lat_column(&cols).is_none() || lon_column(&cols).is_none() {
                continue;
            }
            let Ok(mut extracted) = extract_from_table(
                &conn,
                &table,
                &db_path.to_string_lossy(),
                &prepared_source_name(&prepared_root, &db_path),
                &prepared_bundle_id(&prepared_root, &db_path),
            ) else {
                continue;
            };
            extracted.retain(|record| record.latitude.is_some() && record.longitude.is_some());
            records.append(&mut extracted);
        }

        if !records.is_empty() {
            sources.push(LocationSource {
                app: prepared_source_name(&prepared_root, &db_path),
                bundle_id: prepared_bundle_id(&prepared_root, &db_path),
                database: db_path.to_string_lossy().to_string(),
                records,
            });
        }
    }

    Ok(sources)
}

fn looks_like_sqlite_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "db" | "sqlite" | "sqlitedb" | "storedata")
}

fn should_skip_prepared_db(path: &Path) -> bool {
    let value = path.to_string_lossy().to_ascii_lowercase();
    value.contains("/_manifest/")
        || value.contains("/resourceloadstatistics/")
        || value.contains("/alternativeservices/")
}

fn should_skip_coordinate_table(table: &str) -> bool {
    let lower = table.to_ascii_lowercase();
    lower.contains("explorecity") || lower.contains("explore_city")
}

fn prepared_source_name(prepared_root: &Path, db_path: &Path) -> String {
    let bundle = prepared_bundle_id(prepared_root, db_path);
    bundle
        .strip_prefix("com.")
        .unwrap_or(&bundle)
        .split('.')
        .last()
        .unwrap_or(&bundle)
        .to_string()
}

fn prepared_bundle_id(prepared_root: &Path, db_path: &Path) -> String {
    let rel = db_path.strip_prefix(prepared_root).unwrap_or(db_path);
    let first = rel
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or("prepared");
    first
        .strip_prefix("AppDomainGroup-")
        .or_else(|| first.strip_prefix("AppDomainPlugin-"))
        .or_else(|| first.strip_prefix("AppDomain-"))
        .unwrap_or(first)
        .to_string()
}
