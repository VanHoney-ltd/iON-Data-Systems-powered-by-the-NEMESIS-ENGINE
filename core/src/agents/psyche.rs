//! Psyche Agent

use anyhow::Result;
use serde_json::json;

use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub struct PsycheAgent;

impl Agent for PsycheAgent {
    const NAME: &'static str = "Psyche";
    const SLUG: &'static str = "psyche";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "records_emitted": 0,
                "warnings": Vec::<String>::new(),
            }),
        });
        Ok(records)
    }
}
