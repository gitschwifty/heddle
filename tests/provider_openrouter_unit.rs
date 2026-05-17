use futures::StreamExt;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::{Provider, ProviderConfig, RetryConfig};
use heddle::types::{
    Message, StreamChunk, ToolCallKind, ToolDefinition, ToolFunction, UserMessage,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_KEY: &str = "sk-or-test-key";
const TEST_MODEL: &str = "openrouter/pony-alpha";

fn make_provider(base_url: &str, retry: Option<RetryConfig>) -> Arc<dyn Provider> {
    create_openrouter_provider(ProviderConfig {
        api_key: TEST_KEY.to_string(),
        model: TEST_MODEL.to_string(),
        base_url: Some(base_url.to_string()),
        request_params: None,
        retry,
    })
}

fn default_retry() -> RetryConfig {
    RetryConfig {
        max_retries: 3,
        base_delay_ms: 10,
    }
}

fn user_msgs() -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: "Hello".to_string(),
    })]
}

fn text_response_json(content: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "message": { "content": content },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    })
}

fn tool_call_response_json(name: &str, args: Value) -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "message": {
                "tool_calls": [{
                    "id": "call_0",
                    "type": "function",
                    "function": { "name": name, "arguments": args.to_string() }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    })
}

// SSE chunks (these are simple shapes the provider's deserializer accepts).
fn text_chunk_json(content: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "delta": { "content": content },
            "finish_reason": null
        }]
    })
}

fn finish_chunk_json(reason: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": reason
        }]
    })
}

fn tool_call_chunk_json(
    index: u32,
    id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
) -> Value {
    let mut tc = json!({ "index": index });
    if let Some(i) = id {
        tc["id"] = json!(i);
        tc["type"] = json!("function");
    }
    if name.is_some() || arguments.is_some() {
        let mut fc = json!({});
        if let Some(n) = name {
            fc["name"] = json!(n);
        }
        if let Some(a) = arguments {
            fc["arguments"] = json!(a);
        }
        tc["function"] = fc;
    }
    json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "delta": { "tool_calls": [tc] },
            "finish_reason": null
        }]
    })
}

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

fn sse_template(chunks: &[Value]) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("Content-Type", "text/event-stream")
        .set_body_string(sse_body(chunks))
}

async fn mount_json_once(server: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

async fn mount_json_persistent(server: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_status_once(server: &MockServer, status: u16, body: &str) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body.to_string()))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

async fn drain_stream(p: Arc<dyn Provider>, overrides: Value) -> Vec<StreamChunk> {
    let mut stream = p.stream(user_msgs(), None, overrides);
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        if let Ok(c) = item {
            out.push(c);
        }
    }
    out
}

#[derive(Serialize)]
struct PathArg {
    path: String,
}

// ─── send() ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn send_posts_to_chat_completions_path() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi there!")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(&user_msgs(), None, &json!({})).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].method.as_str(), "POST");
    assert_eq!(reqs[0].url.path(), "/chat/completions");
}

#[tokio::test]
async fn send_includes_auth_and_content_type_headers() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(&user_msgs(), None, &json!({})).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    let h = &reqs[0].headers;
    assert_eq!(
        h.get("authorization").unwrap(),
        &format!("Bearer {TEST_KEY}")
    );
    assert_eq!(h.get("content-type").unwrap(), "application/json");
    assert!(h.get("http-referer").is_some());
}

#[tokio::test]
async fn send_includes_model_messages_stream_false_in_body() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(&user_msgs(), None, &json!({})).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["model"], TEST_MODEL);
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "Hello");
}

#[tokio::test]
async fn send_includes_tools_when_provided() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi")).await;

    let tools = vec![ToolDefinition {
        kind: ToolCallKind::Function,
        function: ToolFunction {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        },
    }];

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(&user_msgs(), Some(&tools), &json!({}))
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
}

#[tokio::test]
async fn send_parses_text_response() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hello world!")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let resp = p.send(&user_msgs(), None, &json!({})).await.unwrap();
    assert_eq!(resp.id, "chatcmpl-test");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello world!")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn send_parses_tool_call_response() {
    let server = MockServer::start().await;
    mount_json_once(
        &server,
        tool_call_response_json("read_file", json!({"path": "/tmp/test.txt"})),
    )
    .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let r = p.send(&user_msgs(), None, &json!({})).await.unwrap();
    let calls = r.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].function.name, "read_file");
    let parsed: Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
    assert_eq!(parsed["path"], "/tmp/test.txt");
}

#[tokio::test]
async fn send_errors_on_non_200() {
    let server = MockServer::start().await;
    mount_status_once(&server, 401, r#"{"error":{"message":"Invalid API key"}}"#).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    assert!(p.send(&user_msgs(), None, &json!({})).await.is_err());
}

#[tokio::test]
async fn send_429_exhausts_retries_then_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limit exceeded"))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let err = p.send(&user_msgs(), None, &json!({})).await.err().unwrap();
    assert!(err.to_string().contains("429"));
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn send_overrides_merge_into_body() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(
        &user_msgs(),
        None,
        &json!({"temperature": 0.5, "max_tokens": 1000}),
    )
    .await
    .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["max_tokens"], 1000);
    assert_eq!(body["model"], TEST_MODEL);
}

#[tokio::test]
async fn per_call_model_override_replaces_config_model() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(&user_msgs(), None, &json!({"model": "override-model"}))
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["model"], "override-model");
}

#[tokio::test]
async fn invalid_overrides_are_filtered() {
    let server = MockServer::start().await;
    mount_json_once(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    p.send(
        &user_msgs(),
        None,
        &json!({"temperature": 5.0, "max_tokens": -1}),
    )
    .await
    .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert!(body.get("temperature").is_none() || body["temperature"].is_null());
    assert!(body.get("max_tokens").is_none() || body["max_tokens"].is_null());
}

// ─── stream() ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn stream_sends_stream_true_in_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_template(&[
            text_chunk_json("Hello"),
            finish_chunk_json("stop"),
        ]))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    drain_stream(p, json!({})).await;

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn stream_yields_text_content_chunks() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_template(&[
            text_chunk_json("Hello"),
            text_chunk_json(" world"),
            finish_chunk_json("stop"),
        ]))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let chunks = drain_stream(p, json!({})).await;
    let texts: Vec<String> = chunks
        .iter()
        .filter_map(|c| c.choices.first().and_then(|ch| ch.delta.content.clone()))
        .collect();
    assert_eq!(texts, vec!["Hello", " world"]);
}

#[tokio::test]
async fn stream_yields_tool_call_deltas() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_template(&[
            tool_call_chunk_json(0, Some("call_0"), Some("read_file"), None),
            tool_call_chunk_json(0, None, None, Some("{\"path\":")),
            tool_call_chunk_json(0, None, None, Some("\"/tmp/test.txt\"}")),
            finish_chunk_json("tool_calls"),
        ]))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let chunks = drain_stream(p, json!({})).await;

    let mut tool_names = Vec::new();
    let mut arg_parts = String::new();
    for c in chunks {
        if let Some(tcs) = c
            .choices
            .first()
            .and_then(|ch| ch.delta.tool_calls.as_ref())
        {
            for tc in tcs {
                if let Some(fc) = &tc.function {
                    if let Some(n) = &fc.name {
                        tool_names.push(n.clone());
                    }
                    if let Some(a) = &fc.arguments {
                        arg_parts.push_str(a);
                    }
                }
            }
        }
    }
    assert_eq!(tool_names, vec!["read_file"]);
    assert_eq!(arg_parts, "{\"path\":\"/tmp/test.txt\"}");
}

#[tokio::test]
async fn stream_errors_on_non_200() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let mut stream = p.stream(user_msgs(), None, json!({}));
    let mut saw_err = false;
    while let Some(item) = stream.next().await {
        if item.is_err() {
            saw_err = true;
            break;
        }
    }
    assert!(saw_err);
}

#[tokio::test]
async fn stream_overrides_merge_into_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_template(&[
            text_chunk_json("Hi"),
            finish_chunk_json("stop"),
        ]))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    drain_stream(p, json!({"temperature": 0.8})).await;

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["temperature"], 0.8);
    assert_eq!(body["stream"], true);
}

// ─── request_params + per-call overrides ──────────────────────────────────

#[tokio::test]
async fn per_call_overrides_win_over_request_params() {
    let server = MockServer::start().await;
    mount_json_persistent(&server, text_response_json("Hi")).await;

    let p = create_openrouter_provider(ProviderConfig {
        api_key: TEST_KEY.to_string(),
        model: TEST_MODEL.to_string(),
        base_url: Some(server.uri()),
        request_params: Some(json!({"temperature": 0.3, "top_p": 0.9})),
        retry: None,
    });

    p.send(&user_msgs(), None, &json!({"temperature": 0.8}))
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["temperature"], 0.8);
    assert_eq!(body["top_p"], 0.9);
}

// ─── 429 retry ────────────────────────────────────────────────────────────

#[tokio::test]
async fn retries_once_on_429_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_json_persistent(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let resp = p.send(&user_msgs(), None, &json!({})).await.unwrap();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi"));
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn retries_multiple_429s_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    mount_json_persistent(&server, text_response_json("Finally")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let resp = p.send(&user_msgs(), None, &json!({})).await.unwrap();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Finally"));
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn does_not_retry_non_429_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server error"))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let err = p.send(&user_msgs(), None, &json!({})).await.err().unwrap();
    assert!(err.to_string().contains("500"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn retry_disabled_when_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), None);
    let err = p.send(&user_msgs(), None, &json!({})).await.err().unwrap();
    assert!(err.to_string().contains("429"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn stream_retries_on_429() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(sse_template(&[
            text_chunk_json("Hi"),
            finish_chunk_json("stop"),
        ]))
        .mount(&server)
        .await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let chunks = drain_stream(p, json!({})).await;
    let texts: Vec<String> = chunks
        .iter()
        .filter_map(|c| c.choices.first().and_then(|ch| ch.delta.content.clone()))
        .collect();
    assert_eq!(texts, vec!["Hi"]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

// ─── with() ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn with_returns_new_provider_original_unchanged() {
    let server = MockServer::start().await;
    mount_json_persistent(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let derived = p.with(json!({"temperature": 0.5}));

    p.send(&user_msgs(), None, &json!({})).await.unwrap();
    derived.send(&user_msgs(), None, &json!({})).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body1: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let body2: Value = serde_json::from_slice(&reqs[1].body).unwrap();
    assert!(body1.get("temperature").is_none() || body1["temperature"].is_null());
    assert_eq!(body2["temperature"], 0.5);
}

#[tokio::test]
async fn with_composes_via_chained_calls() {
    let server = MockServer::start().await;
    mount_json_persistent(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let derived = p
        .with(json!({"temperature": 0.5}))
        .with(json!({"max_tokens": 1000}));
    derived.send(&user_msgs(), None, &json!({})).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["max_tokens"], 1000);
}

#[tokio::test]
async fn with_model_changes_the_model() {
    let server = MockServer::start().await;
    mount_json_persistent(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let derived = p.with(json!({"model": "different-model"}));
    derived.send(&user_msgs(), None, &json!({})).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["model"], "different-model");
}

#[tokio::test]
async fn per_call_overrides_win_over_with_sticky() {
    let server = MockServer::start().await;
    mount_json_persistent(&server, text_response_json("Hi")).await;

    let p = make_provider(&server.uri(), Some(default_retry()));
    let derived = p.with(json!({"temperature": 0.5}));
    derived
        .send(&user_msgs(), None, &json!({"temperature": 0.8}))
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["temperature"], 0.8);
}

#[allow(dead_code)]
fn _path_arg_keepalive() -> PathArg {
    PathArg {
        path: String::new(),
    }
}
