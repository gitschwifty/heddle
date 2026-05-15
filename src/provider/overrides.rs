//! Validate and filter request override fields. Mirrors
//! `ts-src/provider/overrides.ts`. Returns a sanitized JSON object — caller
//! merges it into the request body.

use once_cell::sync::Lazy;
use serde_json::{Map, Value};
use std::collections::HashSet;

use crate::debug::debug;

static VALID_REASONING_EFFORTS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["xhigh", "high", "medium", "low", "minimal", "none"]
        .into_iter()
        .collect()
});

static VALID_REASONING_SUMMARIES: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ["auto", "concise", "detailed"].into_iter().collect());

static KNOWN_KEYS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "model",
        "models",
        "route",
        "temperature",
        "max_tokens",
        "top_p",
        "seed",
        "stop",
        "frequency_penalty",
        "presence_penalty",
        "reasoning",
        "response_format",
        "tools",
        "tool_choice",
        "plugins",
        "provider",
        "session_id",
        "debug",
    ]
    .into_iter()
    .collect()
});

pub fn validate_overrides(raw: &Value) -> Value {
    let mut out = Map::new();
    let table = match raw.as_object() {
        Some(t) => t,
        None => return Value::Object(out),
    };

    for key in table.keys() {
        if !KNOWN_KEYS.contains(key.as_str()) {
            debug("provider", &format!("unknown override key: {key:?}"));
        }
    }

    if let Some(v) = table.get("model").and_then(Value::as_str) {
        out.insert("model".into(), Value::String(v.to_string()));
    }
    if let Some(v) = table.get("session_id").and_then(Value::as_str) {
        if v.len() <= 128 {
            out.insert("session_id".into(), Value::String(v.to_string()));
        } else {
            debug("provider", "session_id exceeds 128 chars, ignoring");
        }
    }
    if let Some(route) = table.get("route").and_then(Value::as_str) {
        if route == "fallback" || route == "sort" {
            out.insert("route".into(), Value::String(route.to_string()));
        }
    }
    if let Some(arr) = table.get("models").and_then(Value::as_array) {
        let all_strings = arr.iter().all(|v| v.is_string());
        if all_strings {
            let mut models: Vec<Value> = arr.clone();
            if models.len() > 3 {
                debug(
                    "provider",
                    &format!(
                        "models array has {} entries, truncating to 3 (OpenRouter limit)",
                        models.len()
                    ),
                );
                models.truncate(3);
            }
            out.insert("models".into(), Value::Array(models));
        }
    }
    if let Some(t) = table.get("temperature").and_then(Value::as_f64) {
        if (0.0..=2.0).contains(&t) {
            out.insert(
                "temperature".into(),
                Value::Number(serde_json::Number::from_f64(t).unwrap_or_else(|| 0.into())),
            );
        } else {
            debug(
                "provider",
                &format!("temperature {t} out of range [0, 2], ignoring"),
            );
        }
    }
    if let Some(n) = table.get("max_tokens").and_then(Value::as_i64) {
        if n > 0 {
            out.insert("max_tokens".into(), Value::Number(n.into()));
        } else {
            debug("provider", "max_tokens must be positive integer, ignoring");
        }
    }
    for key in ["top_p", "frequency_penalty", "presence_penalty"] {
        if let Some(v) = table.get(key).cloned() {
            if v.is_f64() || v.is_i64() {
                out.insert(key.into(), v);
            }
        }
    }
    if let Some(v) = table.get("seed").cloned() {
        if v.is_i64() {
            out.insert("seed".into(), v);
        }
    }
    if let Some(stop) = table.get("stop") {
        let ok = stop.is_string()
            || stop
                .as_array()
                .map(|a| a.iter().all(|x| x.is_string()))
                .unwrap_or(false);
        if ok {
            out.insert("stop".into(), stop.clone());
        }
    }

    if let Some(reasoning) = table.get("reasoning").and_then(Value::as_object) {
        let mut r = Map::new();
        if let Some(e) = reasoning.get("effort").and_then(Value::as_str) {
            if VALID_REASONING_EFFORTS.contains(e) {
                r.insert("effort".into(), Value::String(e.to_string()));
            }
        }
        if let Some(n) = reasoning.get("max_tokens").and_then(Value::as_i64) {
            if n > 0 {
                r.insert("max_tokens".into(), Value::Number(n.into()));
            }
        }
        if let Some(b) = reasoning.get("excluded").and_then(Value::as_bool) {
            r.insert("excluded".into(), Value::Bool(b));
        }
        if let Some(s) = reasoning.get("summary").and_then(Value::as_str) {
            if VALID_REASONING_SUMMARIES.contains(s) {
                r.insert("summary".into(), Value::String(s.to_string()));
            }
        }
        if !r.is_empty() {
            out.insert("reasoning".into(), Value::Object(r));
        }
    }

    for pass_through in [
        "response_format",
        "tools",
        "tool_choice",
        "plugins",
        "provider",
        "debug",
    ] {
        if let Some(v) = table.get(pass_through) {
            let ok = match pass_through {
                "tools" | "plugins" => v.is_array(),
                "tool_choice" => true,
                _ => v.is_object(),
            };
            if ok {
                out.insert(pass_through.into(), v.clone());
            }
        }
    }

    Value::Object(out)
}
