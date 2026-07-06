//! Persist a UsageRecord to disk.

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::collector::SessionMetrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub session_id: String,
    pub project: String,
    pub created: String,
    pub ended: String,
    pub duration_ms: u64,
    pub metrics: SessionMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

pub fn write_usage_record(record: &UsageRecord, project_dir: &Path) -> Result<()> {
    let usage_dir = project_dir.join("usage");
    std::fs::create_dir_all(&usage_dir)?;
    let file_path = usage_dir.join(format!("{}.json", record.session_id));
    let serialized = serde_json::to_string_pretty(record)?;
    std::fs::write(&file_path, serialized)?;
    Ok(())
}
