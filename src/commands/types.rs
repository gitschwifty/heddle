//! Slash command type. The `execute` callback returns `Some(model)` to request
//! a model switch (so the REPL can rebuild its provider).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::agents::types::AgentDefinition;
use crate::config::discovery::DiscoveryResult;
use crate::config::loader::HeddleConfig;
use crate::context::paste_cache::PasteCache;
use crate::cost::tracker::CostTracker;
use crate::provider::types::Provider;
use crate::tools::registry::ToolRegistry;
use crate::types::Message;

pub struct CommandContext<'a> {
    pub config: &'a mut HeddleConfig,
    pub messages: &'a mut Vec<Message>,
    pub registry: &'a ToolRegistry,
    pub cost_tracker: Arc<Mutex<CostTracker>>,
    pub session_file: PathBuf,
    pub session_id: String,
    pub provider: Arc<dyn Provider>,
    pub weak_provider: Option<Arc<dyn Provider>>,
    pub editor_provider: Option<Arc<dyn Provider>>,
    pub discovery: Option<&'a DiscoveryResult>,
    pub agent_definitions: &'a std::collections::HashMap<String, AgentDefinition>,
    pub paste_cache: Option<Arc<Mutex<PasteCache>>>,
}

pub type CommandFn = Arc<
    dyn for<'a> Fn(
            &'a str,
            &'a mut CommandContext<'_>,
        ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>
        + Send
        + Sync,
>;

pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub execute: CommandFn,
}
