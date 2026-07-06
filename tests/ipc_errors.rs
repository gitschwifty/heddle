use heddle::ipc::errors::normalize_error;

mod common;

#[test]
fn openrouter_api_error_with_json_details() {
    let err = r#"OpenRouter API error (500): {"error":{"message":"Model error","type":"error","code":500}}"#;
    let r = normalize_error(err, "provider_error");
    assert_eq!(r.code, "provider_error");
    assert_eq!(r.message, "Model error");
    assert!(r.retryable);
    assert_eq!(r.provider.as_deref(), Some("openrouter"));
    assert!(r.details.is_some());
}

#[test]
fn plain_string_error() {
    let r = normalize_error("Something broke", "provider_error");
    assert_eq!(r.code, "provider_error");
    assert_eq!(r.message, "Something broke");
    assert!(r.retryable);
}

#[test]
fn error_without_api_pattern() {
    let r = normalize_error("Connection refused", "provider_error");
    assert_eq!(r.code, "provider_error");
    assert_eq!(r.message, "Connection refused");
    assert!(r.retryable);
}

#[test]
fn protocol_error_not_retryable() {
    let r = normalize_error("Not initialized", "protocol_error");
    assert_eq!(r.code, "protocol_error");
    assert_eq!(r.message, "Not initialized");
    assert!(!r.retryable);
}

#[test]
fn loop_detected_not_retryable() {
    let r = normalize_error("3 iterations", "loop_detected");
    assert_eq!(r.code, "loop_detected");
    assert!(!r.retryable);
}

#[test]
fn cancelled_passthrough() {
    let r = normalize_error("cancelled", "cancelled");
    assert_eq!(r.code, "cancelled");
    assert_eq!(r.message, "cancelled");
    assert!(!r.retryable);
}

#[test]
fn tool_error_not_retryable() {
    let r = normalize_error("ENOENT: no such file", "tool_error");
    assert_eq!(r.code, "tool_error");
    assert!(!r.retryable);
}

#[test]
fn protocol_version_mismatch_not_retryable() {
    let r = normalize_error("protocol_version_mismatch", "protocol_version_mismatch");
    assert_eq!(r.code, "protocol_version_mismatch");
    assert!(!r.retryable);
}

#[test]
fn partial_api_error_pattern_falls_back_to_label() {
    let r = normalize_error("Something API error happened", "provider_error");
    assert_eq!(r.code, "provider_error");
    assert_eq!(r.message, "Provider error");
    assert!(r.retryable);
}

#[test]
fn extracts_provider_from_api_error() {
    let err = r#"Anthropic API error (429): {"error":{"message":"Rate limited"}}"#;
    let r = normalize_error(err, "provider_error");
    assert_eq!(r.provider.as_deref(), Some("anthropic"));
    assert_eq!(r.message, "Rate limited");
}

#[test]
fn no_provider_for_non_api_errors() {
    let r = normalize_error("timeout", "provider_error");
    assert!(r.provider.is_none());
}
