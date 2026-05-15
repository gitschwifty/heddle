//! Provider abstraction (LLM HTTP clients).

pub mod factory;
pub mod openrouter;
pub mod overrides;
pub mod types;

pub use factory::{create_providers, Providers};
pub use openrouter::create_openrouter_provider;
pub use types::{Provider, ProviderConfig, RetryConfig};
