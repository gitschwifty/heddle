//! Tool trait. Each tool is named, JSON-described, and async-executable.

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct ExecOptions {
    pub signal: Option<CancellationToken>,
}

#[async_trait]
pub trait HeddleTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema describing tool parameters (passed straight through to the
    /// OpenAI/OpenRouter tools API).
    fn parameters(&self) -> Value;

    /// Execute the tool. Return a textual result (success or `Error: ...`).
    ///
    /// The TS port returns errors-as-strings rather than throwing so the agent
    /// loop can feed them back to the LLM; we keep that contract.
    async fn execute(&self, params: Value, options: ExecOptions) -> String;
}
