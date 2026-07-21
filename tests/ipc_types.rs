//! The Rust port uses tagged enums with
//! serde, so we test serialize/deserialize round-trips rather than schema
//! validation.

use heddle::ipc::types::{InitConfig, IpcRequest, IpcResponse, WorkerEvent};
use serde_json::{json, Value};

fn check_request_ok(v: Value) {
    let r: IpcRequest = serde_json::from_value(v.clone())
        .unwrap_or_else(|e| panic!("expected request to parse: {v} ({e})"));
    let _ = r;
}

fn check_request_err(v: Value) {
    let r: Result<IpcRequest, _> = serde_json::from_value(v.clone());
    assert!(r.is_err(), "expected error for {v}");
}

fn check_response_ok(v: Value) {
    let r: IpcResponse = serde_json::from_value(v.clone())
        .unwrap_or_else(|e| panic!("expected response to parse: {v} ({e})"));
    let _ = r;
}

fn check_response_err(v: Value) {
    let r: Result<IpcResponse, _> = serde_json::from_value(v.clone());
    assert!(r.is_err(), "expected error for {v}");
}

fn check_event_ok(v: Value) {
    let r: WorkerEvent = serde_json::from_value(v.clone())
        .unwrap_or_else(|e| panic!("expected event to parse: {v} ({e})"));
    let _ = r;
}

fn check_event_err(v: Value) {
    let r: Result<WorkerEvent, _> = serde_json::from_value(v.clone());
    assert!(r.is_err(), "expected error for {v}");
}

// ─── InitConfig ─────────────────────────────────────────────────────────

#[test]
fn init_config_accepts_complete() {
    let v = json!({
        "model": "openrouter/auto",
        "system_prompt": "You are helpful.",
        "tools": ["read_file", "glob"],
        "max_iterations": 10,
    });
    let r: InitConfig = serde_json::from_value(v).unwrap();
    assert_eq!(r.model, "openrouter/auto");
}

#[test]
fn init_config_accepts_without_max_iterations() {
    let v = json!({
        "model": "openrouter/auto",
        "system_prompt": "prompt",
        "tools": [],
    });
    let _: InitConfig = serde_json::from_value(v).unwrap();
}

#[test]
fn init_config_accepts_task_and_worker_id() {
    let v = json!({
        "model": "openrouter/auto",
        "system_prompt": "prompt",
        "tools": [],
        "task_id": "task-123",
        "worker_id": "worker-0",
    });
    let r: InitConfig = serde_json::from_value(v).unwrap();
    assert_eq!(r.task_id.as_deref(), Some("task-123"));
    assert_eq!(r.worker_id.as_deref(), Some("worker-0"));
}

#[test]
fn init_config_rejects_missing_model() {
    let v = json!({ "system_prompt": "prompt", "tools": [] });
    let r: Result<InitConfig, _> = serde_json::from_value(v);
    assert!(r.is_err());
}

// ─── IpcRequest ─────────────────────────────────────────────────────────

#[test]
fn ipc_request_accepts_init() {
    check_request_ok(json!({
        "type": "init",
        "id": "1",
        "protocol_version": "0.3.0",
        "config": { "model": "m", "system_prompt": "s", "tools": [] },
    }));
}

#[test]
fn ipc_request_accepts_send() {
    check_request_ok(json!({ "type": "send", "id": "2", "message": "hello" }));
}

#[test]
fn ipc_request_accepts_cancel() {
    check_request_ok(json!({ "type": "cancel", "id": "3", "target_id": "2" }));
}

#[test]
fn ipc_request_rejects_unknown_type() {
    check_request_err(json!({ "type": "unknown", "id": "1" }));
}

// ─── WorkerEvent ────────────────────────────────────────────────────────

#[test]
fn worker_event_content_delta() {
    check_event_ok(json!({ "event": "content_delta", "text": "hello" }));
}

#[test]
fn worker_event_error_full_shape() {
    check_event_ok(json!({
        "event": "error",
        "code": "loop_detected",
        "message": "Doom loop detected",
        "retryable": false,
    }));
}

#[test]
fn worker_event_error_with_provider_and_details() {
    check_event_ok(json!({
        "event": "error",
        "code": "provider_error",
        "message": "Model error",
        "retryable": true,
        "provider": "openrouter",
        "details": { "error": { "message": "Model error", "type": "error", "code": 500 } },
    }));
}

#[test]
fn worker_event_error_rejects_missing_retryable() {
    check_event_err(json!({
        "event": "error",
        "code": "provider_error",
        "message": "fail",
    }));
}

#[test]
fn worker_event_error_rejects_missing_code() {
    check_event_err(json!({
        "event": "error",
        "message": "fail",
        "retryable": false,
    }));
}

#[test]
fn worker_event_rejects_old_flat_error_shape() {
    check_event_err(json!({ "event": "error", "error": "something broke" }));
}

#[test]
fn worker_event_context_prune_full() {
    check_event_ok(json!({
        "event": "context_prune",
        "messages_pruned": 5,
        "tokens_before": 50000,
        "tokens_after": 30000,
    }));
}

#[test]
fn worker_event_heartbeat() {
    check_event_ok(json!({ "event": "heartbeat", "duration_ms": 5000 }));
}

#[test]
fn worker_event_context_prune_rejects_partial() {
    check_event_err(json!({
        "event": "context_prune",
        "messages_pruned": 5,
    }));
}

#[test]
fn worker_event_heartbeat_rejects_missing_duration() {
    check_event_err(json!({ "event": "heartbeat" }));
}

// ─── IpcResponse ────────────────────────────────────────────────────────

#[test]
fn ipc_response_init_ok() {
    check_response_ok(json!({
        "type": "init_ok",
        "id": "1",
        "session_id": "sess-1",
        "protocol_version": "0.3.0",
    }));
}

#[test]
fn ipc_response_init_ok_with_error_envelope() {
    check_response_ok(json!({
        "type": "init_ok",
        "id": "1",
        "session_id": "sess-1",
        "protocol_version": "0.3.0",
        "error": { "code": "protocol_error", "message": "bad config", "retryable": false },
    }));
}

#[test]
fn ipc_response_result_without_error() {
    check_response_ok(json!({
        "type": "result",
        "id": "2",
        "status": "ok",
        "tool_calls_made": [],
        "iterations": 1,
    }));
}

#[test]
fn ipc_response_result_with_error_envelope() {
    check_response_ok(json!({
        "type": "result",
        "id": "2",
        "status": "error",
        "tool_calls_made": [],
        "iterations": 0,
        "error": { "code": "provider_error", "message": "Model error", "retryable": true },
    }));
}

#[test]
fn ipc_response_result_rejects_old_flat_error_string() {
    check_response_err(json!({
        "type": "result",
        "id": "2",
        "status": "error",
        "tool_calls_made": [],
        "iterations": 0,
        "error": "Model error",
    }));
}

#[test]
fn ipc_response_event_full_correlation() {
    check_response_ok(json!({
        "type": "event",
        "event": { "event": "content_delta", "text": "hi" },
        "event_seq": 0,
        "send_id": "2",
        "session_id": "sess-1",
        "task_id": "task-1",
        "worker_id": "worker-0",
    }));
}

#[test]
fn ipc_response_result_with_latency_fields() {
    check_response_ok(json!({
        "type": "result",
        "id": "2",
        "status": "ok",
        "tool_calls_made": [],
        "iterations": 1,
        "session_id": "sess-1",
        "task_id": "task-1",
        "worker_id": "worker-0",
        "model_latency_ms": 150,
        "tool_latency_ms": 50,
        "total_latency_ms": 200,
    }));
}

#[test]
fn ipc_response_event_rejects_missing_event_seq() {
    check_response_err(json!({
        "type": "event",
        "event": { "event": "content_delta", "text": "hi" },
        "send_id": "2",
    }));
}

#[test]
fn ipc_response_event_rejects_missing_send_id() {
    check_response_err(json!({
        "type": "event",
        "event": { "event": "content_delta", "text": "hi" },
        "event_seq": 0,
    }));
}
