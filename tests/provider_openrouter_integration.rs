//! Integration tests — gated on `HEDDLE_INTEGRATION_TESTS=1` and
//! `OPENROUTER_API_KEY` being set. Hits the real OpenRouter API with free
//! models. Mirrors `ts-test/provider/openrouter.integration.test.ts`.

use futures::StreamExt;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::ProviderConfig;
use heddle::types::{Message, UserMessage};
use serde_json::json;

const FREE_MODELS: &[&str] = &[
    "liquid/lfm-2.5-1.2b-instruct:free",
    "arcee-ai/trinity-large-preview:free",
    "arcee-ai/trinity-mini:free",
    "openrouter/free",
];

const REASONING_MODELS: &[&str] = &[
    "arcee-ai/trinity-mini:free",
    "stepfun/step-3.5-flash:free",
    "nvidia/nemotron-3-nano-30b-a3b:free",
    "openrouter/free",
];

fn enabled() -> Option<String> {
    if std::env::var("HEDDLE_INTEGRATION_TESTS").ok().as_deref() != Some("1") {
        return None;
    }
    std::env::var("OPENROUTER_API_KEY").ok()
}

fn user_msg() -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: "hello!".to_string(),
    })]
}

#[tokio::test]
async fn send_returns_text_response() {
    let Some(api_key) = enabled() else {
        eprintln!("skip: HEDDLE_INTEGRATION_TESTS != 1 or OPENROUTER_API_KEY unset");
        return;
    };
    let fallback: Vec<&str> = FREE_MODELS.iter().skip(1).copied().collect();
    let p = create_openrouter_provider(ProviderConfig {
        api_key,
        model: FREE_MODELS[0].to_string(),
        base_url: None,
        request_params: Some(json!({ "models": fallback, "route": "fallback" })),
        retry: None,
    });

    let resp = p
        .send(&user_msg(), None, &json!({}))
        .await
        .expect("send failed");
    assert!(!resp.id.is_empty());
    assert!(!resp.choices.is_empty());
    let content = resp.choices[0].message.content.as_deref().unwrap_or("");
    assert!(!content.is_empty(), "expected non-empty content");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn stream_yields_chunks_and_assembles_content() {
    let Some(api_key) = enabled() else {
        eprintln!("skip: HEDDLE_INTEGRATION_TESTS != 1 or OPENROUTER_API_KEY unset");
        return;
    };
    let fallback: Vec<&str> = FREE_MODELS.iter().skip(1).copied().collect();
    let p = create_openrouter_provider(ProviderConfig {
        api_key,
        model: FREE_MODELS[0].to_string(),
        base_url: None,
        request_params: Some(json!({ "models": fallback, "route": "fallback" })),
        retry: None,
    });

    let mut stream = p.stream(user_msg(), None, json!({}));
    let mut parts = Vec::new();
    let mut saw_finish = false;
    while let Some(item) = stream.next().await {
        let chunk = item.expect("stream chunk error");
        if let Some(choice) = chunk.choices.first() {
            if let Some(c) = &choice.delta.content {
                parts.push(c.clone());
            }
            if choice.finish_reason.is_some() {
                saw_finish = true;
            }
        }
    }
    let assembled = parts.join("");
    assert!(!parts.is_empty(), "expected content chunks");
    assert!(!assembled.is_empty());
    assert!(saw_finish, "expected a finish_reason");
}

#[tokio::test]
async fn send_with_reasoning_returns_response() {
    let Some(api_key) = enabled() else {
        eprintln!("skip: HEDDLE_INTEGRATION_TESTS != 1 or OPENROUTER_API_KEY unset");
        return;
    };
    let fallback: Vec<&str> = REASONING_MODELS.iter().skip(1).copied().collect();
    let p = create_openrouter_provider(ProviderConfig {
        api_key,
        model: REASONING_MODELS[0].to_string(),
        base_url: None,
        request_params: Some(json!({
            "models": fallback,
            "route": "fallback",
            "reasoning": { "enabled": true },
        })),
        retry: None,
    });

    let resp = p
        .send(&user_msg(), None, &json!({}))
        .await
        .expect("send failed");
    assert!(!resp.id.is_empty());
    assert!(!resp.choices.is_empty());
    let content = resp.choices[0].message.content.as_deref().unwrap_or("");
    assert!(!content.is_empty());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}
