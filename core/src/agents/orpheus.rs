//! Orpheus — SQLite database reconnaissance agent.

use anyhow::Result;
use serde_json::json;

use crate::agents::orpheus_recon::{list_databases, summarize_db};
use crate::agents::orpheus_report::render_markdown;
use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

const ORPHEUS_SCHEMA_VERSION: u32 = 1;
const ORPHEUS_MAX_ROWS: usize = 5;
const ORPHEUS_SENSITIVE_KEYS: &[&str] = &[
    "pass",
    "pw",
    "pwd",
    "token",
    "secret",
    "key",
    "auth",
    "credential",
];

pub struct OrpheusAgent;

impl Agent for OrpheusAgent {
    const NAME: &'static str = "Orpheus";
    const SLUG: &'static str = "orpheus";
    const SCHEMA_VERSION: u32 = ORPHEUS_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let db_paths = list_databases(&ctx.backup_root);

        let mut summaries = Vec::with_capacity(db_paths.len());
        let mut total_tables = 0usize;
        let mut total_rows = 0usize;
        let mut databases_with_errors = 0usize;
        let mut total_sensitive_columns = 0usize;

        for path in &db_paths {
            let summary = summarize_db(path, ORPHEUS_MAX_ROWS, ORPHEUS_SENSITIVE_KEYS);

            total_tables += summary.tables.len();
            total_rows += summary
                .tables
                .iter()
                .map(|t| t.row_count as usize)
                .sum::<usize>();
            total_sensitive_columns += summary
                .tables
                .iter()
                .map(|t| t.sensitive_columns.len())
                .sum::<usize>();
            if !summary.errors.is_empty() {
                databases_with_errors += 1;
            }

            summaries.push(summary);
        }

        let markdown_report = render_markdown(&summaries);

        let mut records = Vec::with_capacity(summaries.len() + 1);

        // Report record
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "total_databases": summaries.len(),
                "total_tables": total_tables,
                "total_rows": total_rows,
                "databases_with_errors": databases_with_errors,
                "total_sensitive_columns": total_sensitive_columns,
                "markdown_report": markdown_report,
            }),
        });

        // Database records
        for summary in summaries {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "database".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: serde_json::to_value(&summary)?,
            });
        }

        Ok(records)
    }
}
