//! Headless tests that drive the agent loop with a mock SSE OpenRouter server.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::headless::{init_msg, parse_line, Headless};

const T: Duration = Duration::from_secs(8);

fn sse_body(chunks: &[Value]) -> String {
    let mut s = String::new();
    for c in chunks {
        s.push_str("data: ");
        s.push_str(&c.to_string());
        s.push_str("\n\n");
    }
    s.push_str("data: [DONE]\n\n");
    s
}

fn text_delta(text: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }]
    })
}

fn finish_delta_with_usage() -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    })
}

async fn mount_normal_sse(server: &MockServer) {
    let body = sse_body(&[
        text_delta("Hello! "),
        text_delta("How can I help?"),
        finish_delta_with_usage(),
    ]);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(body.into_bytes(), "text/event-stream"),
        )
        .mount(server)
        .await;
}

async fn mount_error_500(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": { "message": "Model error", "type": "error", "code": 500 }
        })))
        .mount(server)
        .await;
}

fn env(server: &MockServer) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("HEDDLE_BASE_URL".into(), server.uri());
    // Default heartbeat interval is 5s but tokio `interval` ticks immediately at start,
    // emitting a heartbeat event with its own seq counter that breaks strict-seq checks.
    // Push it past test timeouts so heartbeats don't fire on the happy path.
    m.insert("HEDDLE_HEARTBEAT_INTERVAL".into(), "3600000".into());
    m
}

fn collect_messages(lines: &[String]) -> Vec<Value> {
    lines.iter().map(|l| parse_line(l)).collect()
}

fn has_result(lines: &[String]) -> bool {
    collect_messages(lines)
        .iter()
        .any(|m| m["type"] == "result")
}

fn count_results(lines: &[String]) -> usize {
    collect_messages(lines)
        .iter()
        .filter(|m| m["type"] == "result")
        .count()
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn send_message_returns_streamed_events_and_result() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Hi there"}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);
    let result = msgs.iter().find(|m| m["type"] == "result").unwrap();
    assert_eq!(result["id"], "2");
    assert_eq!(result["status"], "ok");
    assert!(result["iterations"].as_u64().unwrap_or(0) >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_error_emits_error_event_and_structured_error_result() {
    let server = MockServer::start().await;
    mount_error_500(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Do the thing."}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);

    let error_event = msgs
        .iter()
        .find(|m| m["type"] == "event" && m["event"].is_object() && m["event"]["event"] == "error");
    assert!(error_event.is_some(), "no error event found: {msgs:#?}");
    let ee = error_event.unwrap();
    assert_eq!(ee["event"]["message"], "Model error");
    assert_eq!(ee["event"]["code"], "provider_error");
    assert_eq!(ee["event"]["retryable"], true);
    assert_eq!(ee["event"]["provider"], "openrouter");
    assert!(
        ee["event"]["details"].is_object()
            || ee["event"]["details"].is_string()
            || !ee["event"]["details"].is_null()
    );
    assert_eq!(ee["event_seq"], 0);
    assert_eq!(ee["send_id"], "2");

    let result = msgs.iter().find(|m| m["type"] == "result").unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(result["error"]["code"], "provider_error");
    assert_eq!(result["error"]["message"], "Model error");
    assert_eq!(result["error"]["retryable"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn streamed_events_have_sequential_event_seq_and_correct_send_id() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Hi there"}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);
    let events: Vec<&Value> = msgs.iter().filter(|m| m["type"] == "event").collect();
    assert!(!events.is_empty(), "no events emitted");

    for evt in &events {
        assert_eq!(evt["send_id"], "2");
    }
    let seqs: Vec<u64> = events
        .iter()
        .map(|e| e["event_seq"].as_u64().unwrap())
        .collect();
    for (i, s) in seqs.iter().enumerate() {
        assert_eq!(*s, i as u64, "event_seq mismatch at index {i}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn event_seq_resets_between_sends() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"First"}).to_string());
    h.wait_for(has_result, T);
    let first_line_count = h.line_count();

    h.send_line(&json!({"type":"send","id":"3","message":"Second"}).to_string());
    let lines = h.wait_for(|l| count_results(l) >= 2, T);

    let first_msgs = collect_messages(&lines[..first_line_count]);
    let first_events: Vec<&Value> = first_msgs.iter().filter(|m| m["type"] == "event").collect();
    let second_msgs = collect_messages(&lines[first_line_count..]);
    let second_events: Vec<&Value> = second_msgs
        .iter()
        .filter(|m| m["type"] == "event")
        .collect();

    assert!(!first_events.is_empty());
    assert_eq!(first_events[0]["event_seq"], 0);
    assert_eq!(first_events[0]["send_id"], "2");

    assert!(!second_events.is_empty());
    assert_eq!(second_events[0]["event_seq"], 0);
    assert_eq!(second_events[0]["send_id"], "3");
}

#[tokio::test(flavor = "multi_thread")]
async fn events_carry_session_id_after_init() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    let init_lines = h.wait_for_lines(1, T);
    let init_ok = parse_line(&init_lines[0]);
    let session_id = init_ok["session_id"].as_str().unwrap().to_string();

    h.send_line(&json!({"type":"send","id":"2","message":"Hi there"}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);
    let events: Vec<&Value> = msgs.iter().filter(|m| m["type"] == "event").collect();
    assert!(!events.is_empty());
    for evt in &events {
        assert_eq!(evt["session_id"], session_id);
    }
    let result = msgs.iter().find(|m| m["type"] == "result").unwrap();
    assert_eq!(result["session_id"], session_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn events_carry_task_id_and_worker_id_when_provided_in_init() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    let init = json!({
        "type": "init",
        "id": "1",
        "protocol_version": "0.2.0",
        "config": {
            "model": "openrouter/auto",
            "system_prompt": "You are helpful.",
            "tools": ["read_file","glob","grep"],
            "max_iterations": 10,
            "task_id": "task-42",
            "worker_id": "worker-7"
        }
    });
    h.send_line(&init.to_string());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Hi there"}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);
    let events: Vec<&Value> = msgs.iter().filter(|m| m["type"] == "event").collect();
    assert!(!events.is_empty());
    for evt in &events {
        assert_eq!(evt["task_id"], "task-42");
        assert_eq!(evt["worker_id"], "worker-7");
    }
    let result = msgs.iter().find(|m| m["type"] == "result").unwrap();
    assert_eq!(result["task_id"], "task-42");
    assert_eq!(result["worker_id"], "worker-7");
}

#[tokio::test(flavor = "multi_thread")]
async fn result_carries_latency_fields() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Hi there"}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);
    let result = msgs.iter().find(|m| m["type"] == "result").unwrap();
    assert!(result["total_latency_ms"].is_number());
    assert!(result["model_latency_ms"].is_number());
    assert!(result["tool_latency_ms"].is_number());
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_send_accumulates_messages_multi_turn() {
    let server = MockServer::start().await;
    mount_normal_sse(&server).await;

    let mut h = Headless::spawn(env(&server));
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"First message"}).to_string());
    h.wait_for(has_result, T);

    h.send_line(&json!({"type":"send","id":"3","message":"Second message"}).to_string());
    let lines = h.wait_for(|l| count_results(l) >= 2, T);

    let msgs = collect_messages(&lines);
    let results: Vec<&Value> = msgs.iter().filter(|m| m["type"] == "result").collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["id"], "2");
    assert_eq!(results[1]["id"], "3");
}
