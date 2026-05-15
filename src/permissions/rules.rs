//! Rule parsing and evaluation. Mirrors `ts-src/permissions/rules.ts`.

use std::collections::HashMap;

use globset::Glob;
use once_cell::sync::Lazy;
use serde_json::Value;
use url::Url;

use super::checker::ToolCategory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRule {
    pub tool: String,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PermissionConfig {
    pub allow: Vec<PermissionRule>,
    pub deny: Vec<PermissionRule>,
    pub ask: Vec<PermissionRule>,
}

static TOOL_NAME_MAP: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("read", "read_file");
    m.insert("write", "write_file");
    m.insert("edit", "edit_file");
    m.insert("bash", "bash");
    m.insert("glob", "glob");
    m.insert("grep", "grep");
    m.insert("webfetch", "web_fetch");
    m.insert("askuser", "ask_user");
    m.insert("savememory", "save_memory");
    m
});

static KNOWN_TOOLS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    vec![
        "read_file",
        "write_file",
        "edit_file",
        "bash",
        "glob",
        "grep",
        "web_fetch",
        "ask_user",
        "save_memory",
    ]
});

static CATEGORY_TOOLS: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("read", vec!["read_file", "glob", "grep", "ask_user"]);
    m.insert("write", vec!["write_file", "edit_file", "save_memory"]);
    m.insert("execute", vec!["bash"]);
    m.insert("network", vec!["web_fetch"]);
    m
});

const PATH_TOOLS: &[&str] = &["read_file", "write_file", "edit_file", "glob", "grep"];

fn glob_match(pattern: &str, target: &str) -> bool {
    Glob::new(pattern)
        .map(|g| g.compile_matcher().is_match(target))
        .unwrap_or(false)
}

fn basename(file_path: &str) -> &str {
    file_path.rsplit('/').next().unwrap_or(file_path)
}

/// Matches a command pattern against a command string.
/// "rm *" → prefix match starting with "rm "; "rm" → exact match.
fn match_command(pattern: &str, command: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(" *") {
        return command.starts_with(&format!("{prefix} ")) || command == prefix;
    }
    command == pattern
}

fn extract_hostname(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
}

fn resolve_tool_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    if let Some(known) = KNOWN_TOOLS.iter().find(|t| **t == lower) {
        return Some(known);
    }
    let normalized: String = lower.chars().filter(|c| *c != '_' && *c != '-').collect();
    TOOL_NAME_MAP.get(normalized.as_str()).copied()
}

/// Parse a rule string like `"Write(src/**)"`. Returns one rule, a list (for
/// category names that expand to all tools), or `None` for unknown names.
pub enum ParsedRule {
    One(PermissionRule),
    Many(Vec<PermissionRule>),
}

pub fn parse_rule(raw: &str) -> Option<ParsedRule> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "*" {
        return Some(ParsedRule::One(PermissionRule {
            tool: "*".to_string(),
            pattern: None,
        }));
    }

    let (name, pattern) = if let Some(paren_idx) = trimmed.find('(') {
        if !trimmed.ends_with(')') {
            return None;
        }
        let n = &trimmed[..paren_idx];
        let p = &trimmed[paren_idx + 1..trimmed.len() - 1];
        (n.to_string(), Some(p.to_string()))
    } else {
        (trimmed.to_string(), None)
    };

    let lower_name = name.to_lowercase();

    // Lowercase + category → expand to all tools in category
    if name == lower_name && CATEGORY_TOOLS.contains_key(lower_name.as_str()) {
        let tools = &CATEGORY_TOOLS[lower_name.as_str()];
        let rules: Vec<PermissionRule> = tools
            .iter()
            .map(|t| PermissionRule {
                tool: t.to_string(),
                pattern: pattern.clone(),
            })
            .collect();
        return Some(ParsedRule::Many(rules));
    }

    // Resolve specific tool
    let tool_name = resolve_tool_name(&name)?;
    Some(ParsedRule::One(PermissionRule {
        tool: tool_name.to_string(),
        pattern,
    }))
}

pub fn match_rule(rule: &PermissionRule, tool_name: &str, args: Option<&Value>) -> bool {
    if rule.tool != "*" && rule.tool != tool_name {
        return false;
    }
    let pattern = match &rule.pattern {
        Some(p) => p,
        None => return true,
    };
    let args = match args {
        Some(a) => a,
        None => return false,
    };

    if PATH_TOOLS.contains(&tool_name) {
        if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
            return glob_match(pattern, path) || glob_match(pattern, basename(path));
        }
    }

    if tool_name == "bash" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            return match_command(pattern, cmd);
        }
    }

    if tool_name == "web_fetch" {
        if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
            if let Some(host) = extract_hostname(url) {
                return glob_match(pattern, &host);
            }
        }
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDecision {
    Allow,
    Deny,
    Ask,
}

pub fn evaluate_rules(
    config: &PermissionConfig,
    tool_name: &str,
    args: Option<&Value>,
) -> Option<RuleDecision> {
    if config.deny.iter().any(|r| match_rule(r, tool_name, args)) {
        return Some(RuleDecision::Deny);
    }
    if config.ask.iter().any(|r| match_rule(r, tool_name, args)) {
        return Some(RuleDecision::Ask);
    }
    if config.allow.iter().any(|r| match_rule(r, tool_name, args)) {
        return Some(RuleDecision::Allow);
    }
    None
}

pub fn merge_configs(configs: &[PermissionConfig]) -> PermissionConfig {
    if configs.is_empty() {
        return PermissionConfig::default();
    }
    if configs.len() == 1 {
        return configs[0].clone();
    }

    let mut merged_deny: Vec<PermissionRule> = Vec::new();
    let mut merged_allow: Vec<PermissionRule> = Vec::new();
    let mut merged_ask: Vec<PermissionRule> = Vec::new();

    for config in configs {
        // Allow in later layer removes matching deny from earlier layers
        for allow in &config.allow {
            if let Some(idx) = merged_deny
                .iter()
                .position(|d| d.tool == allow.tool && d.pattern == allow.pattern)
            {
                merged_deny.remove(idx);
            }
            merged_allow.push(allow.clone());
        }
        for deny in &config.deny {
            merged_deny.push(deny.clone());
        }
        for ask in &config.ask {
            merged_ask.push(ask.clone());
        }
    }

    PermissionConfig {
        allow: merged_allow,
        deny: merged_deny,
        ask: merged_ask,
    }
}

/// Stub so other modules can refer to category via this name (rebound below).
pub fn _category_tools() -> &'static HashMap<&'static str, Vec<&'static str>> {
    &CATEGORY_TOOLS
}

impl ToolCategory {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Execute => "execute",
            Self::Network => "network",
        }
    }
}
