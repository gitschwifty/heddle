use futures::StreamExt;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::{ProviderConfig, ProviderFailure};
use heddle::types::{Message, UserMessage};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider(base_url: String) -> std::sync::Arc<dyn heddle::provider::types::Provider> {
    create_openrouter_provider(ProviderConfig {
        api_key: "sk-test".to_string(),
        model: "test-model".to_string(),
        base_url: Some(base_url),
        request_params: None,
        app_attribution: None,
        retry: None,
    })
}

fn user_msgs() -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: "Hi".to_string(),
    })]
}

// ─── send() error handling ────────────────────────────────────────────────

#[tokio::test]
async fn send_errors_on_network_failure() {
    // Point at a closed port (127.0.0.1:1) → connection refused.
    let p = provider("http://127.0.0.1:1".to_string());
    let err = p
        .send(&user_msgs(), None, &json!({}))
        .await
        .expect_err("expected error");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("connection") || msg.contains("refused") || msg.contains("error"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn send_http_error_retains_typed_status_and_safe_detail() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "Unauthorized"}})),
        )
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let err = p
        .send(&user_msgs(), None, &json!({}))
        .await
        .expect_err("expected error");
    let failure = err
        .downcast_ref::<ProviderFailure>()
        .expect("HTTP failure should retain typed provider telemetry");
    assert_eq!(failure.telemetry.status, Some(401));
    assert_eq!(failure.telemetry.detail.as_deref(), Some("Unauthorized"));
}

#[tokio::test]
async fn send_non_json_error_body_is_not_exposed_as_normal_telemetry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal server error"))
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let err = p
        .send(&user_msgs(), None, &json!({}))
        .await
        .expect_err("expected error");
    let failure = err
        .downcast_ref::<ProviderFailure>()
        .expect("HTTP failure should retain typed provider telemetry");
    assert_eq!(failure.telemetry.status, Some(500));
    assert!(failure.telemetry.detail.is_none());
    assert!(
        !err.to_string().contains("Internal server error"),
        "raw provider body must not be exposed: {err}"
    );
}

#[tokio::test]
async fn send_errors_on_403_forbidden() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"error": {"message": "Forbidden"}})),
        )
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let r = p.send(&user_msgs(), None, &json!({})).await;
    assert!(r.is_err());
}

// ─── stream() error handling ──────────────────────────────────────────────

async fn drain_stream(
    p: std::sync::Arc<dyn heddle::provider::types::Provider>,
) -> (Vec<heddle::types::StreamChunk>, Option<anyhow::Error>) {
    let mut stream = p.stream(user_msgs(), None, json!({}));
    let mut chunks = Vec::new();
    let mut err: Option<anyhow::Error> = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(c) => chunks.push(c),
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    (chunks, err)
}

#[tokio::test]
async fn stream_errors_on_network_failure() {
    let p = provider("http://127.0.0.1:1".to_string());
    let (chunks, err) = drain_stream(p).await;
    assert!(chunks.is_empty());
    assert!(err.is_some(), "expected an error");
}

#[tokio::test]
async fn stream_handles_only_done_marker() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_string("data: [DONE]\n\n"),
        )
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let (chunks, err) = drain_stream(p).await;
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert!(chunks.is_empty());
}

#[tokio::test]
async fn stream_ignores_comments_and_blanks() {
    let server = MockServer::start().await;
    let chunk_json = json!({
        "id": "test",
        "choices": [{"index": 0, "delta": {"content": "hi"}, "finish_reason": null}]
    })
    .to_string();
    let body = format!(": this is a comment\n\n\ndata: {chunk_json}\n\ndata: [DONE]\n\n");

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let (chunks, err) = drain_stream(p).await;
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn stream_errors_on_malformed_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_string("data: {invalid json}\n\n"),
        )
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let (_chunks, err) = drain_stream(p).await;
    assert!(err.is_some(), "expected parse error");
}

#[tokio::test]
async fn stream_errors_on_http_failure_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server boom"))
        .mount(&server)
        .await;

    let p = provider(server.uri());
    let (_chunks, err) = drain_stream(p).await;
    assert!(err.is_some(), "expected http error");
}
