//! Validate and filter request override fields.
//! Returns a sanitized JSON object that the caller merges into the request body.

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
        "cache_control",
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
        if v.len() <= 256 {
            out.insert("session_id".into(), Value::String(v.to_string()));
        } else {
            debug("provider", "session_id exceeds 256 chars, ignoring");
        }
    }
    if let Some(cache_control) = table.get("cache_control").and_then(validate_cache_control) {
        out.insert("cache_control".into(), cache_control);
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

    if let Some(provider) = table.get("provider").and_then(validate_provider_routing) {
        out.insert("provider".into(), provider);
    }

    for pass_through in [
        "response_format",
        "tools",
        "tool_choice",
        "plugins",
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

fn validate_cache_control(value: &Value) -> Option<Value> {
    let raw = value.as_object()?;
    let cache_type = raw.get("type").and_then(Value::as_str)?;
    if cache_type != "ephemeral" {
        debug("provider", "cache_control.type must be ephemeral, ignoring");
        return None;
    }

    let mut out = Map::new();
    out.insert("type".into(), Value::String("ephemeral".to_string()));
    if let Some(ttl) = raw.get("ttl").and_then(Value::as_str) {
        if ttl == "1h" {
            out.insert("ttl".into(), Value::String(ttl.to_string()));
        } else {
            debug("provider", "cache_control.ttl must be 1h when present");
        }
    }
    Some(Value::Object(out))
}

fn validate_provider_routing(value: &Value) -> Option<Value> {
    let raw = value.as_object()?;
    let mut out = Map::new();

    if let Some(order) = raw.get("order").and_then(Value::as_array) {
        if order.iter().all(Value::is_string) {
            out.insert("order".into(), Value::Array(order.clone()));
        } else {
            debug("provider", "provider.order must be an array of strings");
        }
    }
    if let Some(allow_fallbacks) = raw.get("allow_fallbacks").and_then(Value::as_bool) {
        out.insert("allow_fallbacks".into(), Value::Bool(allow_fallbacks));
    }
    if let Some(require_parameters) = raw.get("require_parameters").and_then(Value::as_bool) {
        out.insert("require_parameters".into(), Value::Bool(require_parameters));
    }
    if let Some(data_collection) = raw.get("data_collection").and_then(Value::as_str) {
        if data_collection == "allow" || data_collection == "deny" {
            out.insert(
                "data_collection".into(),
                Value::String(data_collection.to_string()),
            );
        } else {
            debug("provider", "provider.data_collection must be allow or deny");
        }
    }
    if let Some(zdr) = raw.get("zdr").and_then(Value::as_bool) {
        out.insert("zdr".into(), Value::Bool(zdr));
    }

    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}
