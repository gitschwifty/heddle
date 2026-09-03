//! Live Straitly contract test. Opt in with `HEDDLE_LIVE_PROVIDER_TESTS=1`.

use futures::StreamExt;
use heddle::provider::openrouter::create_straitly_provider;
use heddle::provider::types::ProviderConfig;
use heddle::types::{Message, UserMessage};
use serde_json::json;

fn enabled() -> Option<String> {
    if std::env::var("HEDDLE_LIVE_PROVIDER_TESTS").ok().as_deref() != Some("1") {
        return None;
    }
    std::env::var("STRAITLY_API_KEY").ok()
}

#[tokio::test]
async fn straitly_stream_includes_usage() {
    let Some(api_key) = enabled() else {
        eprintln!("skip: HEDDLE_LIVE_PROVIDER_TESTS != 1 or STRAITLY_API_KEY unset");
        return;
    };
    let model = std::env::var("HEDDLE_STRAITLY_TEST_MODEL")
        .unwrap_or_else(|_| "deepseek/deepseek-v4-flash-0731".to_string());
    let provider = create_straitly_provider(ProviderConfig {
        api_key,
        model,
        base_url: None,
        request_params: None,
        app_attribution: None,
        retry: None,
        stream_idle_timeout_secs: None,
    });
    let messages = vec![Message::User(UserMessage {
        content: "Reply with exactly: ok".into(),
    })];
    let chunks: Vec<_> = provider.stream(messages, None, json!({})).collect().await;
    let chunks: Vec<_> = chunks
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("stream succeeds");
    let usage = chunks
        .iter()
        .find_map(|chunk| chunk.usage.as_ref())
        .expect("usage chunk");
    assert!(usage.total_tokens > 0);
    assert!(usage.cost.is_some(), "Straitly should report cost");
}
