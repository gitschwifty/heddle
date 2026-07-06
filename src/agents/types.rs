use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub tools: Option<Vec<String>>,
    pub system_prompt: String,
    pub source: PathBuf,
}
