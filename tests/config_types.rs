//! Schema tests for the wire-format (snake_case) config types. Rust uses
//! serde for parsing instead of TypeBox; the tests check round-trip success
//! and rejection on type mismatches.

use heddle::config::types::{
    ApprovalModeWire, HeddleConfigSchema, ProviderConfigSchema, SessionConfigSchema,
};
use serde_json::{json, Value};

// ─── ApprovalModeWire ───────────────────────────────────────────────────

#[test]
fn approval_mode_accepts_all_valid_kebab_modes() {
    for mode in ["suggest", "auto-edit", "full-auto", "plan", "yolo"] {
        let v: Result<ApprovalModeWire, _> = serde_json::from_value(json!(mode));
        assert!(v.is_ok(), "should accept {mode}");
    }
}

#[test]
fn approval_mode_rejects_invalid_strings() {
    let v: Result<ApprovalModeWire, _> = serde_json::from_value(json!("invalid"));
    assert!(v.is_err());
    let v: Result<ApprovalModeWire, _> = serde_json::from_value(json!(""));
    assert!(v.is_err());
    let v: Result<ApprovalModeWire, _> = serde_json::from_value(json!(42));
    assert!(v.is_err());
}

// ─── ProviderConfigSchema ───────────────────────────────────────────────

#[test]
fn provider_config_accepts_empty_object() {
    let v: ProviderConfigSchema = serde_json::from_value(json!({})).unwrap();
    assert!(v.model.is_none());
}

#[test]
fn provider_config_accepts_full() {
    let v: Result<ProviderConfigSchema, _> = serde_json::from_value(json!({
        "model": "anthropic/claude-sonnet",
        "weak_model": "openrouter/free",
        "editor_model": "anthropic/claude-opus",
        "max_tokens": 4096,
        "temperature": 0.7,
        "base_url": "http://localhost:8080"
    }));
    assert!(v.is_ok());
    let v = v.unwrap();
    assert_eq!(v.model.as_deref(), Some("anthropic/claude-sonnet"));
}

#[test]
fn provider_config_rejects_wrong_types() {
    let v: Result<ProviderConfigSchema, _> =
        serde_json::from_value(json!({ "temperature": "hot" }));
    assert!(v.is_err());
    let v: Result<ProviderConfigSchema, _> =
        serde_json::from_value(json!({ "max_tokens": "many" }));
    assert!(v.is_err());
}

// ─── SessionConfigSchema ────────────────────────────────────────────────

#[test]
fn session_config_accepts_empty_object() {
    let v: SessionConfigSchema = serde_json::from_value(json!({})).unwrap();
    assert!(v.system_prompt.is_none());
}

#[test]
fn session_config_accepts_full() {
    let v: Result<SessionConfigSchema, _> = serde_json::from_value(json!({
        "system_prompt": "You are helpful.",
        "approval_mode": "full-auto",
        "instructions": ["HEDDLE.md"],
        "tools": ["read_file", "glob"],
        "doom_loop_threshold": 5,
        "budget_limit": 1.5
    }));
    assert!(v.is_ok(), "err={:?}", v.err());
}

#[test]
fn session_config_rejects_invalid_approval_mode() {
    let v: Result<SessionConfigSchema, _> =
        serde_json::from_value(json!({ "approval_mode": "banana" }));
    assert!(v.is_err());
}

#[test]
fn session_config_rejects_tools_as_bare_string() {
    let v: Result<SessionConfigSchema, _> = serde_json::from_value(json!({ "tools": "read_file" }));
    assert!(v.is_err());
}

#[test]
fn session_config_rejects_instructions_as_bare_string() {
    let v: Result<SessionConfigSchema, _> =
        serde_json::from_value(json!({ "instructions": "HEDDLE.md" }));
    assert!(v.is_err());
}

// ─── HeddleConfigSchema ─────────────────────────────────────────────────

#[test]
fn heddle_config_accepts_empty_object() {
    let v: HeddleConfigSchema = serde_json::from_value(json!({})).unwrap();
    assert!(v.api_key.is_none());
}

#[test]
fn heddle_config_accepts_full() {
    let v: Result<HeddleConfigSchema, _> = serde_json::from_value(json!({
        "api_key": "sk-test",
        "model": "anthropic/claude-sonnet",
        "weak_model": "openrouter/free",
        "editor_model": "anthropic/claude-opus",
        "max_tokens": 4096,
        "temperature": 0.7,
        "base_url": "http://localhost:8080",
        "system_prompt": "Be helpful.",
        "approval_mode": "suggest",
        "instructions": ["HEDDLE.md", "AGENTS.md"],
        "tools": ["read_file", "glob", "grep"],
        "doom_loop_threshold": 3,
        "budget_limit": 5.0
    }));
    assert!(v.is_ok(), "err={:?}", v.err());
}

#[test]
fn heddle_config_rejects_wrong_field_types() {
    let v: Result<HeddleConfigSchema, _> = serde_json::from_value(json!({ "model": 123 }));
    assert!(v.is_err());
    let v: Result<HeddleConfigSchema, _> =
        serde_json::from_value(json!({ "budget_limit": "five" }));
    assert!(v.is_err());
}

#[test]
fn heddle_config_round_trips_via_json() {
    // Round-trip a populated value to verify serialization shape.
    let v = HeddleConfigSchema {
        model: Some("openrouter/free".into()),
        tools: Some(vec!["read_file".into()]),
        ..Default::default()
    };
    let s = serde_json::to_value(&v).unwrap();
    assert_eq!(s["model"], "openrouter/free");
    assert_eq!(s["tools"][0], "read_file");
    let parsed: Value = serde_json::to_value(&v).unwrap();
    let back: HeddleConfigSchema = serde_json::from_value(parsed).unwrap();
    assert_eq!(back.model, v.model);
    assert_eq!(back.tools, v.tools);
}
