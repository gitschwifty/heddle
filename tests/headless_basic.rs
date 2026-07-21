//! Headless protocol tests that don't require the LLM (init, errors, shutdown, status).

use std::collections::HashMap;
use std::time::Duration;

mod common;
use common::headless::{init_msg, parse_line, Headless};

const T: Duration = Duration::from_secs(5);

#[test]
fn init_returns_init_ok_with_session_id_and_protocol_version() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line(&init_msg());
    let lines = h.wait_for_lines(1, T);
    let msg = parse_line(&lines[0]);
    assert_eq!(msg["type"], "init_ok");
    assert_eq!(msg["id"], "1");
    assert!(msg["session_id"].is_string());
    assert_eq!(msg["protocol_version"], "0.3.0");
}

#[test]
fn send_before_init_returns_structured_error_envelope() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line(&serde_json::json!({"type":"send","id":"2","message":"Hi"}).to_string());
    let lines = h.wait_for_lines(1, T);
    let msg = parse_line(&lines[0]);
    assert_eq!(msg["type"], "result");
    assert_eq!(msg["status"], "error");
    assert_eq!(msg["error"]["code"], "protocol_error");
    assert_eq!(
        msg["error"]["message"],
        "Not initialized. Send 'init' first."
    );
    assert_eq!(msg["error"]["retryable"], false);
}

#[test]
fn malformed_json_returns_structured_error_and_process_survives() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line("not valid json{{");
    let lines = h.wait_for_lines(1, T);
    let msg = parse_line(&lines[0]);
    assert_eq!(msg["type"], "result");
    assert_eq!(msg["status"], "error");
    assert_eq!(msg["error"]["code"], "protocol_error");
    assert_eq!(msg["error"]["message"], "Invalid JSON");
    assert_eq!(msg["error"]["retryable"], false);

    // Process should survive — send a valid init now
    h.send_line(&init_msg());
    let lines = h.wait_for_lines(2, T);
    let init_ok = parse_line(&lines[1]);
    assert_eq!(init_ok["type"], "init_ok");
}

#[test]
fn shutdown_returns_shutdown_ok_and_process_exits() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);
    h.send_line(&serde_json::json!({"type":"shutdown","id":"99"}).to_string());
    let lines = h.wait_for_lines(2, T);
    let msg = parse_line(&lines[1]);
    assert_eq!(msg["type"], "shutdown_ok");
    assert_eq!(msg["id"], "99");
    let code = h.wait_exit(Duration::from_secs(3));
    assert_eq!(code, Some(0));
}

#[test]
fn status_returns_status_ok_with_correct_fields() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);
    h.send_line(&serde_json::json!({"type":"status","id":"s1"}).to_string());
    let lines = h.wait_for_lines(2, T);
    let msg = parse_line(&lines[1]);
    assert_eq!(msg["type"], "status_ok");
    assert_eq!(msg["id"], "s1");
    assert!(msg["model"].is_string());
    assert_eq!(msg["messages_count"].as_u64(), Some(0));
    assert!(msg["session_id"].is_string());
    assert!(msg["active"].is_boolean());
}

#[test]
fn protocol_version_included_in_init_ok() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line(&init_msg());
    let lines = h.wait_for_lines(1, T);
    let msg = parse_line(&lines[0]);
    assert_eq!(msg["protocol_version"], "0.3.0");
}

#[test]
fn version_mismatch_returns_structured_error_and_exits() {
    let mut h = Headless::spawn(HashMap::new());
    h.send_line(
        &serde_json::json!({
            "type": "init",
            "id": "1",
            "protocol_version": "1.1.0",
            "config": {"model":"openrouter/auto","system_prompt":"x","tools":[],"max_iterations":2}
        })
        .to_string(),
    );
    let lines = h.wait_for_lines(1, T);
    let msg = parse_line(&lines[0]);
    assert_eq!(msg["type"], "result");
    assert_eq!(msg["status"], "error");
    assert_eq!(msg["error"]["code"], "protocol_version_mismatch");
    assert_eq!(msg["error"]["retryable"], false);

    let code = h.wait_exit(Duration::from_secs(3));
    assert_eq!(code, Some(1));
}

#[test]
fn tool_restriction_via_init_config_tools() {
    let mut h = Headless::spawn(HashMap::new());
    let init = serde_json::json!({
        "type": "init",
        "id": "1",
        "protocol_version": "0.3.0",
        "config": {
            "model": "openrouter/auto",
            "system_prompt": "You are helpful.",
            "tools": ["read_file"],
            "max_iterations": 10
        }
    });
    h.send_line(&init.to_string());
    h.wait_for_lines(1, T);
    h.send_line(&serde_json::json!({"type":"status","id":"s1"}).to_string());
    let lines = h.wait_for_lines(2, T);
    let status = parse_line(&lines[1]);
    assert_eq!(status["type"], "status_ok");
}
