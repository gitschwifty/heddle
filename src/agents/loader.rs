//! Parse agent definition files and assemble a name → definition map.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::types::AgentDefinition;
use crate::config::discovery::DiscoveryResult;

#[derive(Debug, Deserialize, Default)]
struct AgentFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
}

/// Split a markdown file into (frontmatter YAML string, body).
fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    // Must start with --- on its own line
    if !raw.starts_with("---") {
        return (None, raw);
    }
    let after_open = match raw.strip_prefix("---\n") {
        Some(s) => s,
        None => return (None, raw),
    };
    // Find a closing --- on its own line
    let close = after_open.find("\n---\n").or_else(|| {
        after_open
            .strip_suffix("\n---")
            .map(|_| after_open.len() - 4)
    });
    let close_idx = match close {
        Some(i) => i,
        None => return (None, raw),
    };
    let yaml = &after_open[..close_idx];
    let body_start = close_idx + 5; // skip "\n---\n"
    let body = if body_start <= after_open.len() {
        &after_open[body_start.min(after_open.len())..]
    } else {
        ""
    };
    (Some(yaml), body)
}

pub fn parse_agent_file(file_path: &Path) -> Option<AgentDefinition> {
    let raw = std::fs::read_to_string(file_path).ok()?;
    if raw.trim().is_empty() {
        return None;
    }

    let (fm_str, body) = split_frontmatter(&raw);
    let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    let fm: AgentFrontmatter = fm_str
        .and_then(|s| serde_yaml::from_str(s).ok())
        .unwrap_or_default();

    Some(AgentDefinition {
        name: fm.name.unwrap_or_else(|| stem.to_string()),
        description: fm.description.unwrap_or_default(),
        model: fm.model,
        tools: fm.tools,
        system_prompt: body.trim().to_string(),
        source: file_path.to_path_buf(),
    })
}

pub fn load_agent_definitions(discovery: &DiscoveryResult) -> HashMap<String, AgentDefinition> {
    let mut agents = HashMap::new();
    for level in &discovery.levels {
        for filename in &level.agents {
            let file_path = level.path.join("agents").join(filename);
            if let Some(agent) = parse_agent_file(&file_path) {
                agents.entry(agent.name.clone()).or_insert(agent);
            }
        }
    }
    agents
}
