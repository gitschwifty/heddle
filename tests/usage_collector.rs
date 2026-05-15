use heddle::usage::collector::MetricsCollector;

mod common;

#[test]
fn initial_metrics_all_zero() {
    let c = MetricsCollector::new();
    let m = c.metrics();
    assert_eq!(m.message_count.user, 0);
    assert_eq!(m.message_count.assistant, 0);
    assert!(m.tool_calls.is_empty());
    assert_eq!(m.errors.tool, 0);
    assert_eq!(m.errors.provider, 0);
    assert_eq!(m.tokens.input, 0);
    assert_eq!(m.tokens.output, 0);
    assert_eq!(m.turns, 0);
}

#[test]
fn assistant_message_count() {
    let mut c = MetricsCollector::new();
    c.on_assistant_message();
    c.on_assistant_message();
    let m = c.metrics();
    assert_eq!(m.message_count.assistant, 2);
    assert_eq!(m.message_count.user, 0);
}

#[test]
fn user_message_increments_user_and_turns() {
    let mut c = MetricsCollector::new();
    c.on_user_message();
    c.on_user_message();
    c.on_user_message();
    let m = c.metrics();
    assert_eq!(m.message_count.user, 3);
    assert_eq!(m.turns, 3);
}

#[test]
fn tool_call_per_tool_counts() {
    let mut c = MetricsCollector::new();
    c.on_tool_call("read");
    c.on_tool_call("write");
    c.on_tool_call("read");
    c.on_tool_call("read");
    c.on_tool_call("write");
    let tc = &c.metrics().tool_calls;
    assert_eq!(tc.get("read"), Some(&3));
    assert_eq!(tc.get("write"), Some(&2));
}

#[test]
fn tool_errors() {
    let mut c = MetricsCollector::new();
    c.on_tool_error();
    c.on_tool_error();
    let m = c.metrics();
    assert_eq!(m.errors.tool, 2);
    assert_eq!(m.errors.provider, 0);
}

#[test]
fn provider_errors() {
    let mut c = MetricsCollector::new();
    c.on_provider_error();
    let m = c.metrics();
    assert_eq!(m.errors.tool, 0);
    assert_eq!(m.errors.provider, 1);
}

#[test]
fn mixed_errors() {
    let mut c = MetricsCollector::new();
    c.on_tool_error();
    c.on_provider_error();
    c.on_tool_error();
    let m = c.metrics();
    assert_eq!(m.errors.tool, 2);
    assert_eq!(m.errors.provider, 1);
}

#[test]
fn usage_accumulates_tokens() {
    let mut c = MetricsCollector::new();
    c.on_usage(100, 50);
    c.on_usage(200, 75);
    let m = c.metrics();
    assert_eq!(m.tokens.input, 300);
    assert_eq!(m.tokens.output, 125);
}

#[test]
fn metrics_returns_a_snapshot() {
    let mut c = MetricsCollector::new();
    let before = c.metrics();
    c.on_user_message();
    c.on_tool_call("read");
    let after = c.metrics();
    assert_eq!(before.turns, 0);
    assert_eq!(after.turns, 1);
    assert!(before.tool_calls.is_empty());
    assert_eq!(after.tool_calls.get("read"), Some(&1));
}
