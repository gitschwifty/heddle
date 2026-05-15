//! Heddle — TypeScript LLM API harness, Rust port.
//!
//! Library crate exposing the agent loop, provider abstraction, tool registry,
//! and session management. The `heddle` binary is a thin CLI wrapper on top.
//!
//! Module layout mirrors the original `ts-src/` tree one-for-one.

pub mod agent;
pub mod agents;
pub mod cli;
pub mod commands;
pub mod config;
pub mod context;
pub mod cost;
pub mod debug;
pub mod file_history;
pub mod headless;
pub mod history;
pub mod hooks;
pub mod ipc;
pub mod memory;
pub mod permissions;
pub mod plans;
pub mod provider;
pub mod session;
pub mod tasks;
pub mod tools;
pub mod types;
pub mod usage;

// ── Public re-exports (mirrors ts-src/index.ts) ────────────────────────
pub use agent::loop_::run_agent_loop;
pub use agent::types::AgentEvent;
pub use cost::pricing::{ModelPricing, ModelPricingInfo};
pub use cost::tracker::{CostTracker, TurnUsage};
pub use ipc::codec::{build_error, build_result, decode_request, encode_response, wrap_event};
pub use ipc::protocol::{check_compatibility, parse_semver, PROTOCOL_VERSION};
pub use ipc::schema::validate_ipc_message;
pub use ipc::types::*;
pub use provider::factory::{create_providers, Providers};
pub use provider::types::{Provider, ProviderConfig};
pub use session::jsonl::{append_message, load_session};
pub use session::setup::{create_session, SessionContext, SessionOptions};
pub use tools::*;
pub use types::*;
