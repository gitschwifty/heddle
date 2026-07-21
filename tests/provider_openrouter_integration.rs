//! Integration tests — gated on `HEDDLE_INTEGRATION_TESTS=1` and
//! `OPENROUTER_API_KEY` being set. Hits the real OpenRouter API with free
//! models.

mod common;

use futures::StreamExt;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::ProviderConfig;
use heddle::types::{Message, Usage, UserMessage};
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
    common::env::init();
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
        app_attribution: None,
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
    let usage = resp.usage.as_ref().expect("expected usage");
    assert_basic_usage(usage);
    assert!(usage.cost.is_some(), "expected OpenRouter cost in usage");
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
        app_attribution: None,
        retry: None,
    });

    let mut stream = p.stream(user_msg(), None, json!({}));
    let mut parts = Vec::new();
    let mut saw_finish = false;
    let mut usage_chunk_id: Option<String> = None;
    let mut usage: Option<Usage> = None;
    while let Some(item) = stream.next().await {
        let chunk = item.expect("stream chunk error");
        if let Some(u) = chunk.usage {
            usage_chunk_id = Some(chunk.id.clone());
            usage = Some(u);
        }
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
    let usage = usage.expect("expected final usage chunk");
    assert!(usage_chunk_id.as_deref().is_some_and(|id| !id.is_empty()));
    assert_basic_usage(&usage);
    assert!(usage.cost.is_some(), "expected OpenRouter cost in usage");
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
        app_attribution: None,
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
    let usage = resp.usage.as_ref().expect("expected usage");
    assert_basic_usage(usage);
    assert!(usage.cost.is_some(), "expected OpenRouter cost in usage");
    if let Some(details) = &usage.completion_tokens_details {
        assert!(details.reasoning_tokens.is_some());
    }
}

fn assert_basic_usage(usage: &Usage) {
    assert!(usage.prompt_tokens > 0, "expected prompt tokens");
    assert!(usage.total_tokens >= usage.prompt_tokens);
    assert_eq!(
        usage.total_tokens,
        usage.prompt_tokens + usage.completion_tokens
    );
    if let Some(cost) = usage.cost {
        assert!(cost >= 0.0, "expected non-negative cost, got {cost}");
    }
}
