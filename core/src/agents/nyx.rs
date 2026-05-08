//! Nyx — Safari history, bookmarks, autofill, tabs extraction.

use anyhow::{Context, Result};
use serde_json::json;

use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver};
use crate::common::target::{safari_history_target, safari_bookmarks_target, safari_tabs_target};
use crate::evidence::EvidenceRecord;

pub struct NyxAgent;

impl Agent for NyxAgent {
    const NAME: &'static str = "Nyx";
    const SLUG: &'static str = "nyx";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        let mut history_count = 0usize;
        let mut bookmark_count = 0usize;
        let mut tab_count = 0usize;

        // 1. Safari History
        match extract_history(ctx) {
            Ok(mut history_records) => {
                history_count = history_records.len();
                records.append(&mut history_records);
            }
            Err(e) => {
                ctx.log(&format!("Nyx history extraction skipped: {}", e));
            }
        }

        // 2. Safari Bookmarks
        match extract_bookmarks(ctx) {
            Ok(mut bookmark_records) => {
                bookmark_count = bookmark_records.len();
                records.append(&mut bookmark_records);
            }
            Err(e) => {
                ctx.log(&format!("Nyx bookmarks extraction skipped: {}", e));
            }
        }

        // 3. Safari Tabs
        match extract_tabs(ctx) {
            Ok(mut tab_records) => {
                tab_count = tab_records.len();
                records.append(&mut tab_records);
            }
            Err(e) => {
                ctx.log(&format!("Nyx tabs extraction skipped: {}", e));
            }
        }

        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "history_extracted": history_count,
                "bookmarks_extracted": bookmark_count,
                "tabs_extracted": tab_count,
                "records_emitted": records.len(),
                "warnings": Vec::<String>::new(),
            }),
        });

        Ok(records)
    }
}

fn extract_history(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = safari_history_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let query = r#"
        SELECT
            hv.id,
            hi.url,
            hv.title,
            hv.visit_time,
            hv.load_successful,
            hv.synthesized,
            hv.origin
        FROM history_visits hv
        JOIN history_items hi ON hv.history_item = hi.id
        ORDER BY hv.visit_time DESC
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, url, title, visit_time, load_successful, synthesized, origin) = row;
        records.push(EvidenceRecord {
            schema_version: NyxAgent::SCHEMA_VERSION,
            source_agent: NyxAgent::NAME.to_string(),
            record_type: "history".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "url": url,
                "title": title,
                "visit_time": visit_time,
                "load_successful": load_successful != 0,
                "synthesized": synthesized != 0,
                "origin": origin,
            }),
        });
    }

    Ok(records)
}

fn extract_bookmarks(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = safari_bookmarks_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let query = r#"
        SELECT
            id,
            title,
            url,
            parent,
            type,
            order_index,
            special_id,
            added,
            last_modified
        FROM bookmarks
        WHERE deleted = 0
        ORDER BY parent, order_index
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<f64>>(8)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, title, url, parent, btype, order_index, special_id, added, last_modified) = row;
        records.push(EvidenceRecord {
            schema_version: NyxAgent::SCHEMA_VERSION,
            source_agent: NyxAgent::NAME.to_string(),
            record_type: "bookmark".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "title": title,
                "url": url,
                "parent": parent,
                "type": btype,
                "order_index": order_index,
                "special_id": special_id,
                "added": added,
                "last_modified": last_modified,
            }),
        });
    }

    Ok(records)
}

fn extract_tabs(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = safari_tabs_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let query = r#"
        SELECT
            b.id,
            b.title,
            b.url,
            w.id AS window_id,
            w.order_index AS window_order
        FROM bookmarks b
        JOIN windows w ON b.parent = w.id
        WHERE b.deleted = 0 AND b.type = 0
        ORDER BY w.order_index, b.order_index
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, title, url, window_id, window_order) = row;
        records.push(EvidenceRecord {
            schema_version: NyxAgent::SCHEMA_VERSION,
            source_agent: NyxAgent::NAME.to_string(),
            record_type: "tab".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "title": title,
                "url": url,
                "window_id": window_id,
                "window_order": window_order,
            }),
        });
    }

    Ok(records)
}
