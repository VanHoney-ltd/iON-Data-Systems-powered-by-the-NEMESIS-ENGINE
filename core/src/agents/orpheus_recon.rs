//! Orpheus Database Reconnaissance Module
//!
//! Discovers and analyzes SQLite databases across mobile device backups.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Validates that an identifier is safe to use in SQL (table/column names).
/// Only allows alphanumeric characters and underscores. Must start with a letter or underscore.
fn is_safe_sql_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // First character must be alphabetic or underscore
    let first = name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    // Rest must be alphanumeric or underscore
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Summary of a database table
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableSummary {
    pub name: String,
    pub columns: Vec<String>,
    pub row_count: i64,
    pub sensitive_columns: Vec<String>,
    pub sample_rows: Vec<HashMap<String, serde_json::Value>>,
}

/// Summary of a complete database
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DatabaseSummary {
    pub path: String,
    pub tables: Vec<TableSummary>,
    pub errors: Vec<String>,
}

/// List all SQLite databases in a directory tree
pub fn list_databases(root: &Path) -> Vec<PathBuf> {
    let extensions: HashSet<&str> = ["sqlite", "db", "sqlitedb", "storedata"]
        .iter()
        .cloned()
        .collect();

    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Analyze a single database and return summary
pub fn summarize_db(path: &Path, max_rows: usize, sensitive_keys: &[&str]) -> DatabaseSummary {
    let path_str = path.to_string_lossy().to_string();
    let mut tables = Vec::new();
    let mut errors = Vec::new();

    // Evidence databases must be opened read-only; fail closed instead of mutating files.
    let conn_result = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    );

    let conn = match conn_result {
        Ok(c) => c,
        Err(e) => {
            return DatabaseSummary {
                path: path_str,
                tables: Vec::new(),
                errors: vec![format!("open_error: {}", e)],
            };
        }
    };

    // Set busy timeout to avoid locking issues in parallel mode
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));

    // Get table names
    let table_names: Vec<String> =
        match conn.prepare("SELECT name FROM sqlite_master WHERE type='table'") {
            Ok(mut stmt) => match stmt.query_map([], |row| row.get::<_, String>(0)) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    errors.push(format!("table_list_error: {}", e));
                    Vec::new()
                }
            },
            Err(e) => {
                errors.push(format!("db_error: {}", e));
                return DatabaseSummary {
                    path: path_str,
                    tables,
                    errors,
                };
            }
        };

    for table_name in table_names {
        match summarize_table(&conn, &table_name, max_rows, sensitive_keys) {
            Ok(summary) => tables.push(summary),
            Err(e) => errors.push(format!("table_error {}: {}", table_name, e)),
        }
    }

    DatabaseSummary {
        path: path_str,
        tables,
        errors,
    }
}

/// Analyze a single table within a database
fn summarize_table(
    conn: &rusqlite::Connection,
    table_name: &str,
    max_rows: usize,
    sensitive_keys: &[&str],
) -> Result<TableSummary> {
    // Validate table name to prevent SQL injection
    if !is_safe_sql_identifier(table_name) {
        return Err(anyhow!("Invalid table name: {}", table_name));
    }
    
    // Get columns
    let mut col_stmt = conn.prepare(&format!("PRAGMA table_info('{}')", table_name))?;
    let columns: Vec<String> = col_stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();

    // Detect sensitive columns
    let sensitive_columns: Vec<String> = columns
        .iter()
        .filter(|col| {
            let col_lower = col.to_lowercase();
            sensitive_keys.iter().any(|k| col_lower.contains(k))
        })
        .cloned()
        .collect();

    // Get row count (table_name already validated above)
    let count: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM '{}'", table_name),
        [],
        |row| row.get(0),
    )?;

    // Get sample rows (table_name already validated above)
    let mut samples = Vec::new();
    if max_rows > 0 && count > 0 {
        let query = format!("SELECT * FROM '{}' LIMIT {}", table_name, max_rows);
        let mut stmt = conn.prepare(&query)?;
        let column_names: Vec<String> = (0..stmt.column_count())
            .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
            .collect();

        let rows = stmt.query_map([], |row| {
            let mut row_map = HashMap::new();
            for (idx, col_name) in column_names.iter().enumerate() {
                let value = match row.get::<_, rusqlite::types::Value>(idx) {
                    Ok(v) => sanitize_value(v),
                    Err(_) => serde_json::Value::Null,
                };
                row_map.insert(col_name.clone(), value);
            }
            Ok(row_map)
        })?;

        for row in rows.take(max_rows) {
            if let Ok(r) = row {
                samples.push(r);
            }
        }
    }

    Ok(TableSummary {
        name: table_name.to_string(),
        columns,
        row_count: count,
        sensitive_columns,
        sample_rows: samples,
    })
}

/// Sanitize a database value for JSON serialization
fn sanitize_value(val: rusqlite::types::Value) -> serde_json::Value {
    match val {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::Value::Number(i.into()),
        rusqlite::types::Value::Real(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)),
        ),
        rusqlite::types::Value::Text(s) => serde_json::Value::String(s),
        rusqlite::types::Value::Blob(b) => {
            serde_json::Value::String(format!("<bytes {}>", b.len()))
        }
    }
}
