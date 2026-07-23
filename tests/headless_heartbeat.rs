//! Headless heartbeat + cancel tests against a slow/tool-slow mock SSE server.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

mod common;
use common::headless::{init_msg, parse_line, Headless};

const T: Duration = Duration::from_secs(15);

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
    json!({"id":"chatcmpl-test","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})
}
fn finish_with_usage() -> Value {
    json!({
        "id":"chatcmpl-test",
        "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
    })
}
fn tool_call_chunk(index: u32, id: Option<&str>, name: Option<&str>, args: Option<&str>) -> Value {
    let mut tc = serde_json::Map::new();
    tc.insert("index".into(), json!(index));
    if let Some(i) = id {
        tc.insert("id".into(), json!(i));
        tc.insert("type".into(), json!("function"));
    }
    if name.is_some() || args.is_some() {
        let mut f = serde_json::Map::new();
        if let Some(n) = name {
            f.insert("name".into(), json!(n));
        }
        if let Some(a) = args {
            f.insert("arguments".into(), json!(a));
        }
        tc.insert("function".into(), Value::Object(f));
    }
    json!({
        "id":"chatcmpl-test",
        "choices":[{"index":0,"delta":{"tool_calls":[Value::Object(tc)]},"finish_reason":null}]
    })
}
fn tool_finish_delta() -> Value {
    json!({
        "id":"chatcmpl-test",
        "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
    })
}

fn sse_template_delayed(body: String, delay_ms: u64) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(body.into_bytes(), "text/event-stream")
        .set_delay(Duration::from_millis(delay_ms))
}

struct ToolSlowResponder {
    count: Arc<AtomicUsize>,
}
impl Respond for ToolSlowResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        let body = if n == 0 {
            sse_body(&[
                tool_call_chunk(0, Some("call_0"), Some("bash"), None),
                tool_call_chunk(0, None, None, Some(r#"{"command":"sleep 30"}"#)),
                tool_finish_delta(),
            ])
        } else {
            sse_body(&[text_delta("Done"), finish_with_usage()])
        };
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(body.into_bytes(), "text/event-stream")
    }
}

fn collect_messages(lines: &[String]) -> Vec<Value> {
    lines.iter().map(|l| parse_line(l)).collect()
}

fn has_result(lines: &[String]) -> bool {
    collect_messages(lines)
        .iter()
        .any(|m| m["type"] == "result")
}

// ─── Heartbeat tests ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_events_emitted_during_active_send() {
    let server = MockServer::start().await;
    let body = sse_body(&[
        text_delta("Slow "),
        text_delta("response"),
        finish_with_usage(),
    ]);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_template_delayed(body, 600))
        .mount(&server)
        .await;

    let mut env = HashMap::new();
    env.insert("HEDDLE_BASE_URL".into(), server.uri());
    env.insert("HEDDLE_HEARTBEAT_INTERVAL".into(), "100".into());

    let mut h = Headless::spawn(env);
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Do something slow"}).to_string());
    let lines = h.wait_for(has_result, T);
    let msgs = collect_messages(&lines);
    let heartbeats: Vec<&Value> = msgs
        .iter()
        .filter(|m| m["type"] == "event" && m["event"]["event"] == "heartbeat")
        .collect();
    assert!(!heartbeats.is_empty(), "no heartbeats: {msgs:#?}");
    for hb in &heartbeats {
        assert!(hb["event_seq"].is_number());
        assert_eq!(hb["send_id"], "2");
        assert!(hb["event"]["duration_ms"].is_number());
    }
    let seqs: Vec<u64> = msgs
        .iter()
        .filter(|m| m["type"] == "event")
        .map(|m| m["event_seq"].as_u64().unwrap())
        .collect();
    for (idx, seq) in seqs.iter().enumerate() {
        assert_eq!(*seq, idx as u64, "non-monotonic event_seq in {msgs:#?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn no_heartbeat_after_send_completes() {
    let server = MockServer::start().await;
    let body = sse_body(&[text_delta("Hi"), finish_with_usage()]);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(body.into_bytes(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let mut env = HashMap::new();
    env.insert("HEDDLE_BASE_URL".into(), server.uri());
    env.insert("HEDDLE_HEARTBEAT_INTERVAL".into(), "50".into());

    let mut h = Headless::spawn(env);
    h.send_line(&init_msg());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Hi"}).to_string());
    h.wait_for(has_result, T);
    let after_result = h.line_count();

    // Wait long enough that several heartbeats would've fired if still active.
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(h.line_count(), after_result, "extra lines after result");
}

// ─── Cancel test ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn cancel_interrupts_bash_tool_and_resolves_quickly() {
    let server = MockServer::start().await;
    let responder = ToolSlowResponder {
        count: Arc::new(AtomicUsize::new(0)),
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(responder)
        .mount(&server)
        .await;

    let mut env = HashMap::new();
    env.insert("HEDDLE_BASE_URL".into(), server.uri());
    env.insert("HEDDLE_HEARTBEAT_INTERVAL".into(), "100".into());

    let mut h = Headless::spawn(env);
    let init = json!({
        "type":"init",
        "id":"1",
        "protocol_version":"0.4.0",
        "config":{
            "model":"openrouter/auto",
            "system_prompt":"You are helpful.",
            "tools":["bash"],
            "max_iterations":10
        }
    });
    h.send_line(&init.to_string());
    h.wait_for_lines(1, T);

    h.send_line(&json!({"type":"send","id":"2","message":"Run a slow command"}).to_string());

    // Wait for the bash tool_start event.
    h.wait_for(
        |lines| {
            collect_messages(lines).iter().any(|m| {
                m["type"] == "event"
                    && m["event"]["event"] == "tool_start"
                    && m["event"]["name"] == "bash"
            })
        },
        T,
    );

    // Cancel and time the response.
    let start = Instant::now();
    h.send_line(&json!({"type":"cancel","id":"3","target_id":"2"}).to_string());
    h.wait_for(has_result, T);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(8),
        "cancel took too long: {elapsed:?}"
    );

    let msgs = collect_messages(&h.lines_snapshot());
    let result = msgs.iter().find(|m| m["type"] == "result").unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(result["error"]["code"], "cancelled");
}
