//! Subprocess-driven IPC replay against fixture pairs in `ts-test/ipc/fixtures/`.
//!
//! Mirrors `ts-test/ipc/replay.test.ts`. Strips non-deterministic fields per
//! `IGNORE_PATHS` before comparison.

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

mod common;
use common::headless::{parse_line, Headless};

const T: Duration = Duration::from_secs(15);

const IGNORE_PATHS: &[&str] = &[
    "session_id",
    "timestamp",
    "usage.prompt_tokens",
    "usage.completion_tokens",
    "usage.total_tokens",
    "event.result_preview",
    "event.details",
    "event.provider",
    "task_id",
    "worker_id",
    "model_latency_ms",
    "tool_latency_ms",
    "total_latency_ms",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ts-test/ipc/fixtures")
}

fn load_lines(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {path:?}: {e}"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect()
}

fn delete_path(obj: &mut Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = obj;
    for key in &parts[..parts.len() - 1] {
        match cur.get_mut(*key) {
            Some(v) if v.is_object() => cur = v,
            _ => return,
        }
    }
    if let Some(obj) = cur.as_object_mut() {
        obj.remove(parts[parts.len() - 1]);
    }
}

fn strip_ignored(v: &Value) -> Value {
    let mut clone = v.clone();
    for p in IGNORE_PATHS {
        delete_path(&mut clone, p);
    }
    clone
}

// ─── SSE mock helpers ────────────────────────────────────────────────────

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
    serde_json::json!({
        "id": "chatcmpl-test",
        "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
    })
}

fn finish_with_usage(p: u64, c: u64, t: u64) -> Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": p, "completion_tokens": c, "total_tokens": t}
    })
}

fn tool_call_chunks(name: &str, args: &str) -> Vec<Value> {
    vec![
        serde_json::json!({
            "id": "chatcmpl-test",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_0",
                        "type": "function",
                        "function": {"name": name, "arguments": ""}
                    }]
                },
                "finish_reason": null
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-test",
            "choices": [{
                "index": 0,
                "delta": {"tool_calls": [{"index": 0, "function": {"arguments": args}}]},
                "finish_reason": null
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-test",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
        }),
    ]
}

fn sse_resp(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(body.into_bytes(), "text/event-stream")
}

// Normal: first request → tool call, second → text.
struct NormalResponder {
    count: Arc<AtomicUsize>,
}
impl Respond for NormalResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            sse_resp(sse_body(&tool_call_chunks("glob", r#"{"pattern":"*"}"#)))
        } else {
            sse_resp(sse_body(&[
                text_delta("Here are the files..."),
                finish_with_usage(42, 15, 57),
            ]))
        }
    }
}

// Heartbeat: 250ms delay, then tool call → text.
struct HeartbeatResponder {
    count: Arc<AtomicUsize>,
}
impl Respond for HeartbeatResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        let body = if n == 0 {
            sse_body(&tool_call_chunks("read_file", r#"{"path":"src/main.rs"}"#))
        } else {
            sse_body(&[
                text_delta("The codebase has..."),
                finish_with_usage(120, 30, 150),
            ])
        };
        sse_resp(body).set_delay(Duration::from_millis(250))
    }
}

async fn mount_normal(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(NormalResponder {
            count: Arc::new(AtomicUsize::new(0)),
        })
        .mount(server)
        .await;
}

async fn mount_error(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "error": {"message": "Model error", "type": "error", "code": 500}
        })))
        .mount(server)
        .await;
}

async fn mount_cancel(server: &MockServer) {
    // Slow response so cancel beats it.
    let body = sse_body(&[
        text_delta("Working..."),
        text_delta("Still working..."),
        finish_with_usage(10, 5, 15),
    ]);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_resp(body).set_delay(Duration::from_secs(5)))
        .mount(server)
        .await;
}

async fn mount_heartbeat(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(HeartbeatResponder {
            count: Arc::new(AtomicUsize::new(0)),
        })
        .mount(server)
        .await;
}

// ─── Replay harness ──────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Mode {
    NoServer,
    Normal,
    Error,
    Cancel,
    Heartbeat,
}

fn collect_until_shutdown_or_exit(
    h: &Headless,
    expected_count: usize,
    timeout: Duration,
) -> Vec<String> {
    // Wait until we have at least the expected number of lines, OR the
    // process exits, OR we time out.
    let start = std::time::Instant::now();
    loop {
        let n = h.line_count();
        if n >= expected_count {
            return h.lines_snapshot();
        }
        if start.elapsed() > timeout {
            return h.lines_snapshot();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn has_shutdown_ok(lines: &[String]) -> bool {
    lines
        .iter()
        .any(|line| parse_line(line)["type"] == "shutdown_ok")
}

async fn run_fixture(name: &str, mode: Mode) {
    let in_lines = load_lines(&fixtures_dir().join(format!("{name}.in.jsonl")));
    let expected_lines = load_lines(&fixtures_dir().join(format!("{name}.out.jsonl")));

    let server_opt = match mode {
        Mode::NoServer => None,
        _ => Some(MockServer::start().await),
    };

    if let Some(s) = &server_opt {
        match mode {
            Mode::Normal => mount_normal(s).await,
            Mode::Error => mount_error(s).await,
            Mode::Cancel => mount_cancel(s).await,
            Mode::Heartbeat => mount_heartbeat(s).await,
            Mode::NoServer => unreachable!(),
        }
    }

    let mut env = HashMap::new();
    env.insert(
        "HEDDLE_BASE_URL".into(),
        server_opt
            .as_ref()
            .map(|s| s.uri())
            .unwrap_or_else(|| "http://localhost:1".to_string()),
    );
    if matches!(mode, Mode::Heartbeat) {
        env.insert("HEDDLE_HEARTBEAT_INTERVAL".into(), "100".into());
    } else {
        // Suppress heartbeats by pushing the interval past test runtime.
        env.insert("HEDDLE_HEARTBEAT_INTERVAL".into(), "3600000".into());
    }

    let mut h = Headless::spawn(env);
    for line in &in_lines {
        h.send_line(line);
    }

    let lines = if matches!(mode, Mode::Heartbeat) {
        h.wait_for(has_shutdown_ok, T)
    } else {
        collect_until_shutdown_or_exit(&h, expected_lines.len(), T)
    };

    if matches!(mode, Mode::Heartbeat) {
        compare_heartbeat(&lines, &expected_lines);
    } else {
        compare_strict(&lines, &expected_lines, name);
    }
}

fn compare_strict(actual: &[String], expected: &[String], name: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "fixture {name}: line count mismatch.\nactual:\n{}\nexpected:\n{}",
        actual.join("\n"),
        expected.join("\n")
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        let av = strip_ignored(&parse_line(a));
        let ev = strip_ignored(&parse_line(e));
        pretty_assertions::assert_eq!(
            av,
            ev,
            "fixture {name} line {i} mismatch\nactual: {a}\nexpected: {e}"
        );
    }
}

fn compare_heartbeat(actual: &[String], expected: &[String]) {
    let actual_parsed: Vec<Value> = actual.iter().map(|l| parse_line(l)).collect();
    let expected_parsed: Vec<Value> = expected.iter().map(|l| parse_line(l)).collect();

    let is_hb = |v: &Value| v["type"] == "event" && v["event"]["event"] == "heartbeat";

    let actual_hb: Vec<&Value> = actual_parsed.iter().filter(|v| is_hb(v)).collect();
    let actual_non_hb: Vec<&Value> = actual_parsed.iter().filter(|v| !is_hb(v)).collect();
    let expected_non_hb: Vec<&Value> = expected_parsed.iter().filter(|v| !is_hb(v)).collect();

    assert!(
        !actual_hb.is_empty(),
        "heartbeat fixture: expected at least one heartbeat event"
    );
    for hb in &actual_hb {
        assert_eq!(hb["event"]["event"], "heartbeat");
        assert!(hb["event"]["duration_ms"].is_number());
        assert!(hb["event_seq"].is_number());
        assert_eq!(hb["send_id"], "2");
    }

    assert_eq!(
        actual_non_hb.len(),
        expected_non_hb.len(),
        "heartbeat fixture: non-heartbeat line count mismatch\nactual:\n{}\nexpected:\n{}",
        actual
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        expected
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    );
    for (i, (a, e)) in actual_non_hb.iter().zip(expected_non_hb.iter()).enumerate() {
        let mut av = strip_ignored(a);
        let mut ev = strip_ignored(e);
        // Heartbeats shift event_seq counters; strip them.
        if let Some(obj) = av.as_object_mut() {
            obj.remove("event_seq");
        }
        if let Some(obj) = ev.as_object_mut() {
            obj.remove("event_seq");
        }
        pretty_assertions::assert_eq!(
            av,
            ev,
            "heartbeat fixture non-hb line {i} mismatch\nactual: {a}\nexpected: {e}"
        );
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn fixture_version_mismatch() {
    run_fixture("version-mismatch", Mode::NoServer).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fixture_error() {
    run_fixture("error", Mode::Error).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fixture_normal() {
    run_fixture("normal", Mode::Normal).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fixture_cancel() {
    run_fixture("cancel", Mode::Cancel).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fixture_heartbeat() {
    run_fixture("heartbeat", Mode::Heartbeat).await;
}
