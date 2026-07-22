use heddle::provider::overrides::validate_overrides;
use serde_json::{json, Value};

fn vo(raw: Value) -> Value {
    validate_overrides(&raw)
}

#[test]
fn accepts_valid_fields() {
    let r = vo(json!({
        "model": "anthropic/claude-sonnet",
        "temperature": 0.7,
        "max_tokens": 4096,
    }));
    assert_eq!(r["model"], "anthropic/claude-sonnet");
    assert_eq!(r["temperature"], 0.7);
    assert_eq!(r["max_tokens"], 4096);
}

#[test]
fn empty_overrides_returns_empty_object() {
    let r = vo(json!({}));
    assert_eq!(r.as_object().unwrap().len(), 0);
}

#[test]
fn rejects_temperature_outside_range() {
    assert!(vo(json!({ "temperature": -0.1 }))
        .get("temperature")
        .is_none());
    assert!(vo(json!({ "temperature": 2.1 }))
        .get("temperature")
        .is_none());
    assert_eq!(vo(json!({ "temperature": 0 }))["temperature"], 0.0);
    assert_eq!(vo(json!({ "temperature": 2 }))["temperature"], 2.0);
}

#[test]
fn rejects_invalid_max_tokens() {
    assert!(vo(json!({ "max_tokens": -1 })).get("max_tokens").is_none());
    assert!(vo(json!({ "max_tokens": 0 })).get("max_tokens").is_none());
    // Floats aren't accepted (as_i64 returns None for non-integers).
    assert!(vo(json!({ "max_tokens": 1.5 })).get("max_tokens").is_none());
    assert_eq!(vo(json!({ "max_tokens": 100 }))["max_tokens"], 100);
}

#[test]
fn validates_session_id_max_256() {
    assert_eq!(vo(json!({ "session_id": "short" }))["session_id"], "short");
    let long = "a".repeat(256);
    assert_eq!(
        vo(json!({ "session_id": long.clone() }))["session_id"],
        long
    );
    let too_long = "a".repeat(257);
    assert!(vo(json!({ "session_id": too_long }))
        .get("session_id")
        .is_none());
}

#[test]
fn validates_cache_control() {
    assert_eq!(
        vo(json!({ "cache_control": { "type": "ephemeral" } }))["cache_control"],
        json!({ "type": "ephemeral" })
    );
    assert_eq!(
        vo(json!({ "cache_control": { "type": "ephemeral", "ttl": "1h" } }))["cache_control"],
        json!({ "type": "ephemeral", "ttl": "1h" })
    );
    assert!(vo(json!({ "cache_control": { "type": "persistent" } }))
        .get("cache_control")
        .is_none());
    assert!(vo(json!({ "cache_control": "ephemeral" }))
        .get("cache_control")
        .is_none());
}

#[test]
fn validates_reasoning_nested_object() {
    let r = vo(json!({
        "reasoning": {
            "effort": "high",
            "max_tokens": 2000,
            "excluded": false,
            "summary": "concise",
        }
    }));
    let reasoning = &r["reasoning"];
    assert_eq!(reasoning["effort"], "high");
    assert_eq!(reasoning["max_tokens"], 2000);
    assert_eq!(reasoning["excluded"], false);
    assert_eq!(reasoning["summary"], "concise");
}

#[test]
fn rejects_invalid_reasoning_effort() {
    let r = vo(json!({ "reasoning": { "effort": "invalid" } }));
    assert!(r.get("reasoning").is_none());
}

#[test]
fn rejects_invalid_reasoning_summary() {
    let r = vo(json!({ "reasoning": { "summary": "verbose" } }));
    assert!(r.get("reasoning").is_none());
}

#[test]
fn rejects_negative_reasoning_max_tokens() {
    let r = vo(json!({ "reasoning": { "max_tokens": -10 } }));
    assert!(r.get("reasoning").is_none());
}

#[test]
fn validates_route_values() {
    assert_eq!(vo(json!({ "route": "fallback" }))["route"], "fallback");
    assert_eq!(vo(json!({ "route": "sort" }))["route"], "sort");
    assert!(vo(json!({ "route": "invalid" })).get("route").is_none());
}

#[test]
fn validates_models_array() {
    assert_eq!(
        vo(json!({ "models": ["a", "b"] }))["models"],
        json!(["a", "b"])
    );
    assert!(vo(json!({ "models": "not-array" })).get("models").is_none());
}

#[test]
fn truncates_models_over_three() {
    let r = vo(json!({ "models": ["a", "b", "c", "d", "e"] }));
    assert_eq!(r["models"], json!(["a", "b", "c"]));
}

#[test]
fn passes_through_complex_objects() {
    let r = vo(json!({
        "response_format": { "type": "json_object" },
        "tool_choice": "auto",
    }));
    assert_eq!(r["response_format"], json!({ "type": "json_object" }));
    assert_eq!(r["tool_choice"], "auto");
}

#[test]
fn validates_provider_routing_object() {
    let r = vo(json!({
        "provider": {
            "order": ["anthropic", "openai"],
            "allow_fallbacks": false,
            "require_parameters": true,
            "data_collection": "deny",
            "zdr": true,
            "ignored": "field",
        },
    }));
    assert_eq!(
        r["provider"],
        json!({
            "order": ["anthropic", "openai"],
            "allow_fallbacks": false,
            "require_parameters": true,
            "data_collection": "deny",
            "zdr": true,
        })
    );
}

#[test]
fn drops_invalid_provider_routing_fields() {
    let r = vo(json!({
        "provider": {
            "order": ["anthropic", 42],
            "allow_fallbacks": "false",
            "require_parameters": "true",
            "data_collection": "private",
            "zdr": "true",
        },
    }));
    assert!(r.get("provider").is_none());
}

#[test]
fn drops_unknown_keys() {
    let r = vo(json!({ "unknown_field": "value", "temperature": 0.5 }));
    assert_eq!(r["temperature"], 0.5);
    assert!(r.get("unknown_field").is_none());
}

#[test]
fn numeric_fields_pass_through() {
    let r = vo(json!({
        "top_p": 0.9,
        "seed": 42,
        "frequency_penalty": 0.5,
        "presence_penalty": -0.5,
    }));
    assert_eq!(r["top_p"], 0.9);
    assert_eq!(r["seed"], 42);
    assert_eq!(r["frequency_penalty"], 0.5);
    assert_eq!(r["presence_penalty"], -0.5);
}

#[test]
fn stop_as_string() {
    assert_eq!(vo(json!({ "stop": "\n" }))["stop"], "\n");
}

#[test]
fn stop_as_string_array() {
    assert_eq!(
        vo(json!({ "stop": ["\n", "END"] }))["stop"],
        json!(["\n", "END"])
    );
}
