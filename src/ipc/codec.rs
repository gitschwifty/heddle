//! Encode/decode IPC messages over JSONL.

use anyhow::Result;
use serde_json::Value;

use super::errors::ErrorEnvelope;
use super::types::{IpcRequest, IpcResponse, ToolCallSummary, UsageSummary, WorkerEvent};

#[derive(Debug, Clone, Default)]
pub struct CorrelationContext {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub worker_id: Option<String>,
}

pub fn encode_response(response: &IpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| "{}".to_string())
}

pub enum DecodeResult {
    Ok(IpcRequest),
    Err(String),
}

pub fn decode_request(line: &str) -> DecodeResult {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return DecodeResult::Err("Invalid JSON".to_string()),
    };
    if !parsed.is_object() {
        return DecodeResult::Err("Expected JSON object".to_string());
    }
    if parsed.get("type").and_then(Value::as_str).is_none() {
        return DecodeResult::Err("Missing 'type' field".to_string());
    }
    if parsed.get("id").and_then(Value::as_str).is_none() {
        return DecodeResult::Err("Missing 'id' field".to_string());
    }
    match serde_json::from_value::<IpcRequest>(parsed) {
        Ok(req) => DecodeResult::Ok(req),
        Err(e) => DecodeResult::Err(e.to_string()),
    }
}

pub fn wrap_event(
    event: WorkerEvent,
    send_id: &str,
    event_seq: u64,
    ctx: Option<&CorrelationContext>,
) -> IpcResponse {
    IpcResponse::Event {
        event,
        send_id: send_id.to_string(),
        event_seq,
        session_id: ctx.and_then(|c| c.session_id.clone()),
        task_id: ctx.and_then(|c| c.task_id.clone()),
        worker_id: ctx.and_then(|c| c.worker_id.clone()),
    }
}

#[derive(Debug, Clone, Default)]
pub struct BuildResultArgs {
    pub status: String,
    pub response: Option<String>,
    pub tool_calls_made: Vec<ToolCallSummary>,
    pub usage: Option<UsageSummary>,
    pub iterations: u32,
    pub error: Option<ErrorEnvelope>,
    pub correlation: Option<CorrelationContext>,
    pub model_latency_ms: Option<u64>,
    pub tool_latency_ms: Option<u64>,
    pub total_latency_ms: Option<u64>,
}

pub fn build_result(id: &str, args: BuildResultArgs) -> IpcResponse {
    IpcResponse::Result {
        id: id.to_string(),
        status: args.status,
        response: args.response,
        tool_calls_made: args.tool_calls_made,
        usage: args.usage,
        iterations: args.iterations,
        error: args.error,
        session_id: args.correlation.as_ref().and_then(|c| c.session_id.clone()),
        task_id: args.correlation.as_ref().and_then(|c| c.task_id.clone()),
        worker_id: args.correlation.as_ref().and_then(|c| c.worker_id.clone()),
        model_latency_ms: args.model_latency_ms,
        tool_latency_ms: args.tool_latency_ms,
        total_latency_ms: args.total_latency_ms,
    }
}

pub fn build_error(
    id: Option<&str>,
    error: ErrorEnvelope,
    correlation: Option<&CorrelationContext>,
) -> IpcResponse {
    IpcResponse::Result {
        id: id.unwrap_or("unknown").to_string(),
        status: "error".to_string(),
        response: None,
        tool_calls_made: Vec::new(),
        usage: None,
        iterations: 0,
        error: Some(error),
        session_id: correlation.and_then(|c| c.session_id.clone()),
        task_id: correlation.and_then(|c| c.task_id.clone()),
        worker_id: correlation.and_then(|c| c.worker_id.clone()),
        model_latency_ms: None,
        tool_latency_ms: None,
        total_latency_ms: None,
    }
}

#[allow(dead_code)]
fn _unused_result() -> Result<()> {
    Ok(())
}
