use heddle::config::loader::HeddleConfig;
use heddle::provider::factory::create_providers;
use heddle::types::{Message, UserMessage};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn base_config() -> HeddleConfig {
    HeddleConfig {
        api_key: Some("test-key".to_string()),
        model: "main-model".to_string(),
        ..HeddleConfig::default()
    }
}

fn ok_response() -> Value {
    json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "message": { "content": "ok" },
            "finish_reason": "stop"
        }]
    })
}

async fn mount_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response()))
        .mount(server)
        .await;
}

fn user_msg() -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: "hi".to_string(),
    })]
}

#[test]
fn returns_main_provider() {
    let providers = create_providers(&base_config()).unwrap();
    assert!(providers.weak.is_none());
    assert!(providers.editor.is_none());
    // Existence of main is implied by the unwrap above + struct field.
    let _ = providers.main;
}

#[test]
fn weak_model_set_returns_main_and_weak() {
    let providers = create_providers(&HeddleConfig {
        weak_model: Some("weak-model".to_string()),
        ..base_config()
    })
    .unwrap();
    assert!(providers.weak.is_some());
    assert!(providers.editor.is_none());
}

#[test]
fn editor_model_set_returns_main_and_editor() {
    let providers = create_providers(&HeddleConfig {
        editor_model: Some("editor-model".to_string()),
        ..base_config()
    })
    .unwrap();
    assert!(providers.editor.is_some());
    assert!(providers.weak.is_none());
}

#[test]
fn both_weak_and_editor_returns_all_three() {
    let providers = create_providers(&HeddleConfig {
        weak_model: Some("weak-model".to_string()),
        editor_model: Some("editor-model".to_string()),
        ..base_config()
    })
    .unwrap();
    assert!(providers.weak.is_some());
    assert!(providers.editor.is_some());
}

#[test]
fn no_weak_no_editor_returns_only_main() {
    let providers = create_providers(&base_config()).unwrap();
    assert!(providers.weak.is_none());
    assert!(providers.editor.is_none());
}

#[test]
fn missing_api_key_errors() {
    let err = create_providers(&HeddleConfig {
        api_key: None,
        ..base_config()
    })
    .err()
    .expect("expected an error");
    assert!(err.to_string().contains("API key is required"));
}

#[tokio::test]
async fn all_providers_use_same_base_url() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let providers = create_providers(&HeddleConfig {
        base_url: Some(server.uri()),
        weak_model: Some("weak-model".to_string()),
        editor_model: Some("editor-model".to_string()),
        ..base_config()
    })
    .unwrap();

    let overrides = json!({});
    providers
        .main
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();
    providers
        .weak
        .as_ref()
        .unwrap()
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();
    providers
        .editor
        .as_ref()
        .unwrap()
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for r in &requests {
        assert_eq!(r.url.path(), "/chat/completions");
    }
}

#[tokio::test]
async fn all_providers_share_request_params() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let providers = create_providers(&HeddleConfig {
        base_url: Some(server.uri()),
        max_tokens: Some(1000),
        temperature: Some(0.5),
        weak_model: Some("weak-model".to_string()),
        editor_model: Some("editor-model".to_string()),
        ..base_config()
    })
    .unwrap();

    let overrides = json!({});
    providers
        .main
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();
    providers
        .weak
        .as_ref()
        .unwrap()
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();
    providers
        .editor
        .as_ref()
        .unwrap()
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for r in &requests {
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["temperature"], 0.5);
    }
}

#[tokio::test]
async fn each_provider_sends_its_own_model() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let providers = create_providers(&HeddleConfig {
        base_url: Some(server.uri()),
        weak_model: Some("weak-model".to_string()),
        editor_model: Some("editor-model".to_string()),
        ..base_config()
    })
    .unwrap();

    let overrides = json!({});
    providers
        .main
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();
    providers
        .weak
        .as_ref()
        .unwrap()
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();
    providers
        .editor
        .as_ref()
        .unwrap()
        .send(&user_msg(), None, &overrides)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let bodies: Vec<Value> = requests
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    assert_eq!(bodies[0]["model"], "main-model");
    assert_eq!(bodies[1]["model"], "weak-model");
    assert_eq!(bodies[2]["model"], "editor-model");
}

#[tokio::test]
async fn factory_providers_retry_429_by_default() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_string("Rate limited"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_ok(&server).await;

    let providers = create_providers(&HeddleConfig {
        base_url: Some(server.uri()),
        ..base_config()
    })
    .unwrap();

    let response = providers
        .main
        .send(&user_msg(), None, &json!({}))
        .await
        .unwrap();

    assert_eq!(response.choices[0].message.content.as_deref(), Some("ok"));
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
