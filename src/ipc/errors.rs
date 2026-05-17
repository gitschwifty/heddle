//! Normalize provider errors into a structured envelope for the wire.

use std::collections::HashSet;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NormalizedError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub provider: Option<String>,
    pub details: Option<Value>,
}

impl NormalizedError {
    pub fn into_envelope(self) -> (ErrorEnvelope, Option<String>) {
        (
            ErrorEnvelope {
                code: self.code,
                message: self.message,
                retryable: self.retryable,
                details: self.details,
            },
            self.provider,
        )
    }
}

static RETRYABLE_CODES: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ["provider_error"].into_iter().collect());

fn error_code_label(code: &str) -> &'static str {
    match code {
        "provider_error" => "Provider error",
        "tool_error" => "Tool error",
        "protocol_error" => "Protocol error",
        "loop_detected" => "Doom loop detected",
        "timeout" => "Timeout",
        _ => "Unknown error",
    }
}

static PROVIDER_ERROR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(.+?)\s+API error\s+\((\d+[^)]*)\):\s*([\s\S]*)$").unwrap());

pub fn normalize_error(err: &str, code: &str) -> NormalizedError {
    let retryable = RETRYABLE_CODES.contains(code);
    let raw = err.to_string();
    let caps = PROVIDER_ERROR_RE.captures(&raw);

    if caps.is_none() {
        if raw.contains("API error") {
            return NormalizedError {
                code: code.to_string(),
                message: error_code_label(code).to_string(),
                retryable,
                provider: None,
                details: Some(Value::String(raw)),
            };
        }
        return NormalizedError {
            code: code.to_string(),
            message: raw,
            retryable,
            provider: None,
            details: None,
        };
    }

    let caps = caps.unwrap();
    let provider = caps
        .get(1)
        .map(|m| m.as_str().to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());
    let raw_details = caps.get(3).map(|m| m.as_str()).unwrap_or("").to_string();

    let details: Value = match serde_json::from_str(&raw_details) {
        Ok(v) => v,
        Err(_) => Value::String(raw_details.clone()),
    };

    let mut inner_msg: Option<String> = None;
    if let Value::String(s) = &details {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            inner_msg = Some(trimmed.to_string());
        }
    }
    if let Some(obj) = details.as_object() {
        if let Some(inner) = obj.get("error") {
            if let Some(m) = inner.get("message").and_then(Value::as_str) {
                inner_msg = Some(m.to_string());
            } else if let Some(s) = inner.as_str() {
                inner_msg = Some(s.to_string());
            }
        }
    }

    let message = inner_msg.unwrap_or_else(|| error_code_label(code).to_string());
    NormalizedError {
        code: code.to_string(),
        message,
        retryable,
        provider: Some(provider),
        details: Some(details),
    }
}
