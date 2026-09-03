use futures::StreamExt;
use heddle::config::loader::{HeddleConfig, ProviderKind};
use heddle::provider::factory::create_providers;
use heddle::types::{Message, ToolCallKind, ToolDefinition, ToolFunction, UserMessage};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config(base_url: String) -> HeddleConfig {
    HeddleConfig {
        provider: ProviderKind::Straitly,
        api_key: Some("straitly-test-key".to_string()),
        straitly_credential: None,
        base_url: Some(base_url),
        model: "claude-sonnet-5".to_string(),
        ..HeddleConfig::default()
    }
}

fn messages() -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: "hello".to_string(),
    })]
}

fn tool() -> ToolDefinition {
    ToolDefinition {
        kind: ToolCallKind::Function,
        function: ToolFunction {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type":"object","properties":{}}),
        },
    }
}

#[tokio::test]
async fn straitly_uses_openai_compatible_request_without_openrouter_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "choices": [{"index": 0, "message": {"content": "ok"}, "finish_reason": "stop"}]
        })))
        .mount(&server)
        .await;

    let provider = create_providers(&config(server.uri())).unwrap().main;
    let response = provider
        .send(&messages(), Some(&[tool()]), &Value::Null)
        .await
        .unwrap();
    assert_eq!(response.choices[0].message.content.as_deref(), Some("ok"));

    let request = server.received_requests().await.unwrap().remove(0);
    assert_eq!(
        request.headers.get("authorization").unwrap(),
        "Bearer straitly-test-key"
    );
    assert!(request.headers.get("x-openrouter-title").is_none());
    assert!(request.headers.get("x-openrouter-metadata").is_none());
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["model"], "claude-sonnet-5");
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
}

#[tokio::test]
async fn straitly_streams_openai_tool_call_chunks() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-test\",\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":8,\"cache_creation_5m_input_tokens\":8,\"prompt_tokens_details\":{\"cached_tokens\":4,\"cache_write_tokens\":8},\"completion_tokens_details\":{\"reasoning_tokens\":2},\"cost\":0.000123}}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let provider = create_providers(&config(server.uri())).unwrap().main;
    let chunks: Vec<_> = provider
        .stream(messages(), Some(vec![tool()]), Value::Null)
        .collect()
        .await;
    assert!(chunks.iter().any(|chunk| {
        chunk
            .as_ref()
            .unwrap()
            .choices
            .iter()
            .any(|choice| choice.delta.tool_calls.is_some())
    }));
    let usage = chunks
        .iter()
        .filter_map(|chunk| chunk.as_ref().unwrap().usage.as_ref())
        .next()
        .expect("usage chunk");
    assert_eq!(usage.cached_tokens(), Some(4));
    assert_eq!(usage.cache_write_tokens(), Some(8));
    assert_eq!(
        usage
            .completion_tokens_details
            .as_ref()
            .unwrap()
            .reasoning_tokens,
        Some(2)
    );
    assert_eq!(usage.cost, Some(0.000123));
    let request = server.received_requests().await.unwrap().remove(0);
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["stream_options"]["include_usage"], true);
}
