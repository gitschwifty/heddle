//! Tool registry: register / lookup / dispatch by name with JSON args.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::string_distance::find_closest;
use super::types::{ExecOptions, HeddleTool};
use crate::types::{ToolCallKind, ToolDefinition, ToolFunction};

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn HeddleTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn HeddleTool>) -> Result<()> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(anyhow!("Tool {name:?} already registered"));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn HeddleTool>> {
        self.tools.get(name).cloned()
    }

    pub fn all(&self) -> Vec<Arc<dyn HeddleTool>> {
        self.tools.values().cloned().collect()
    }

    /// OpenAI-format tool definitions for the API.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.all()
            .into_iter()
            .map(|t| ToolDefinition {
                kind: ToolCallKind::Function,
                function: ToolFunction {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters(),
                },
            })
            .collect()
    }

    pub fn subset(&self, names: &[String]) -> ToolRegistry {
        let mut sub = ToolRegistry::new();
        for n in names {
            if let Some(t) = self.tools.get(n) {
                let _ = sub.register(t.clone());
            }
        }
        sub
    }

    pub async fn execute(&self, name: &str, args_json: &str, options: ExecOptions) -> String {
        let tool = match self.tools.get(name) {
            Some(t) => t.clone(),
            None => {
                let available: Vec<String> = self.tools.keys().cloned().collect();
                let suggestion = find_closest(name, &available, 3);
                let available_str = available.join(", ");
                return match suggestion {
                    Some(s) => format!(
                        "Error: Unknown tool: {name}. Did you mean {s:?}? Available tools: {available_str}"
                    ),
                    None => format!("Error: Unknown tool: {name}. Available tools: {available_str}"),
                };
            }
        };

        let parsed: Value = match serde_json::from_str(args_json) {
            Ok(v) => v,
            Err(_) => return format!("Error: Invalid JSON arguments: {args_json}"),
        };

        tool.execute(parsed, options).await
    }
}
