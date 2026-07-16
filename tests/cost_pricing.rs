//! ModelPricing fetches pricing from `/models`. Mocked with wiremock.

use heddle::cost::pricing::ModelPricing;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn mock_response() -> serde_json::Value {
    json!({
        "data": [
            {
                "id": "openai/gpt-4",
                "name": "GPT-4",
                "pricing": { "prompt": "0.00003", "completion": "0.00006" },
                "context_length": 128000,
                "top_provider": { "max_completion_tokens": 4096 },
                "architecture": { "modality": "text+image->text" },
                "supported_parameters": ["temperature", "top_p"]
            },
            {
                "id": "anthropic/claude-3-opus",
                "name": "Claude 3 Opus",
                "pricing": { "prompt": "0.000015", "completion": "0.000075" },
                "context_length": 200000,
                "top_provider": { "max_completion_tokens": 4096 },
                "architecture": { "modality": "text+image->text" },
                "supported_parameters": ["temperature", "top_p", "top_k"]
            }
        ]
    })
}

async fn mount_models(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn get_model_returns_pricing_info_for_known_model() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let m = pricing.get_model("openai/gpt-4").await.unwrap();
    assert_eq!(m.id, "openai/gpt-4");
    assert_eq!(m.name, "GPT-4");
    assert!((m.prompt_price - 0.00003).abs() < 1e-9);
    assert!((m.completion_price - 0.00006).abs() < 1e-9);
    assert_eq!(m.context_length, 128000);
    assert_eq!(m.max_completion_tokens, 4096);
    assert_eq!(m.modality, "text+image->text");
    assert_eq!(m.supported_parameters, vec!["temperature", "top_p"]);
}

#[tokio::test]
async fn get_model_returns_none_for_unknown_model() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    assert!(pricing.get_model("nonexistent/model").await.is_none());
}

#[tokio::test]
async fn get_all_models_returns_full_list() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let models = pricing.get_all_models().await;
    assert_eq!(models.len(), 2);
    let mut ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    ids.sort();
    assert_eq!(ids, vec!["anthropic/claude-3-opus", "openai/gpt-4"]);
}

#[tokio::test]
async fn search_models_filters_by_id_or_name() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let models = pricing.search_models("opus", 20).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "anthropic/claude-3-opus");
}

#[tokio::test]
async fn lazy_loading_no_fetch_until_first_access() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    assert!(!pricing.is_loaded().await);
}

#[tokio::test]
async fn caching_second_access_does_not_refetch() {
    let server = MockServer::start().await;
    // Mock that expects exactly 1 call.
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response()))
        .expect(1)
        .mount(&server)
        .await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    pricing.get_model("openai/gpt-4").await;
    pricing.get_model("anthropic/claude-3-opus").await;
    pricing.get_all_models().await;
    // mock drop verifies single-call expectation
}

#[tokio::test]
async fn concurrent_fetch_deduplication() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response()))
        .expect(1)
        .mount(&server)
        .await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let (a, b, c) = tokio::join!(
        pricing.get_model("openai/gpt-4"),
        pricing.get_all_models(),
        pricing.estimate_cost("openai/gpt-4", 1000, 500),
    );
    assert!(a.is_some());
    assert_eq!(b.len(), 2);
    assert!(c.is_some());
}

#[tokio::test]
async fn parses_string_prices_to_numbers_correctly() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let m = pricing.get_model("anthropic/claude-3-opus").await.unwrap();
    assert!((m.prompt_price - 0.000015).abs() < 1e-9);
    assert!((m.completion_price - 0.000075).abs() < 1e-9);
}

#[tokio::test]
async fn estimate_cost_calculates_correctly() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let cost = pricing
        .estimate_cost("openai/gpt-4", 1000, 500)
        .await
        .unwrap();
    assert!((cost - 0.06).abs() < 1e-6, "got {cost}");
}

#[tokio::test]
async fn estimate_cost_returns_none_for_unknown_model() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    assert!(pricing
        .estimate_cost("nonexistent/model", 1000, 500)
        .await
        .is_none());
}

#[tokio::test]
async fn is_loaded_reflects_fetch_state() {
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    assert!(!pricing.is_loaded().await);
    pricing.get_all_models().await;
    assert!(pricing.is_loaded().await);
}
