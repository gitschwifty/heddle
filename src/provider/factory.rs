//! Build the bundle of `(main, weak, editor)` providers from `HeddleConfig`.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

use super::openrouter::create_openrouter_provider;
use super::types::{Provider, ProviderConfig, RetryConfig};
use crate::config::loader::HeddleConfig;

#[derive(Clone)]
pub struct Providers {
    pub main: Arc<dyn Provider>,
    pub weak: Option<Arc<dyn Provider>>,
    pub editor: Option<Arc<dyn Provider>>,
}

fn base_request_params(config: &HeddleConfig) -> Option<Value> {
    let mut map = Map::new();
    if let Some(mt) = config.max_tokens {
        map.insert("max_tokens".into(), Value::Number(mt.into()));
    }
    if let Some(t) = config.temperature {
        if let Some(n) = serde_json::Number::from_f64(t) {
            map.insert("temperature".into(), Value::Number(n));
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

pub fn create_providers(config: &HeddleConfig) -> Result<Providers> {
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| anyhow!("API key is required"))?;
    let params = base_request_params(config);

    let build = |model: &str| -> Arc<dyn Provider> {
        create_openrouter_provider(ProviderConfig {
            api_key: api_key.clone(),
            model: model.to_string(),
            base_url: config.base_url.clone(),
            request_params: params.clone(),
            app_attribution: config.app_attribution.clone(),
            retry: Some(RetryConfig::default()),
        })
    };

    let main = build(&config.model);
    let weak = config.weak_model.as_deref().map(build);
    let editor = config.editor_model.as_deref().map(build);

    Ok(Providers { main, weak, editor })
}
