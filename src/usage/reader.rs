//! Read and aggregate UsageRecord files.

use std::collections::BTreeMap;
use std::path::Path;

use super::writer::UsageRecord;

pub fn read_usage_record(session_id: &str, project_dir: &Path) -> Option<UsageRecord> {
    let file_path = project_dir.join("usage").join(format!("{session_id}.json"));
    let content = std::fs::read_to_string(&file_path).ok()?;
    serde_json::from_str(&content).ok()
}

#[derive(Debug, Default, Clone)]
pub struct AggregatedUsage {
    pub total_sessions: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost: f64,
    pub tool_calls: BTreeMap<String, u64>,
}

pub fn aggregate_usage(project_dir: &Path) -> AggregatedUsage {
    let mut out = AggregatedUsage::default();
    let usage_dir = project_dir.join("usage");
    let entries = match std::fs::read_dir(&usage_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let record: UsageRecord = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(_) => continue,
        };
        out.total_sessions += 1;
        out.total_input_tokens += record.metrics.tokens.input;
        out.total_output_tokens += record.metrics.tokens.output;
        out.total_cost += record.cost_usd.unwrap_or(0.0);
        for (tool, count) in record.metrics.tool_calls {
            *out.tool_calls.entry(tool).or_insert(0) += count;
        }
    }
    out
}
