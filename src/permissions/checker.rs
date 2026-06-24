//! PermissionChecker — mode matrix + rule overlay + directory scoping.
//! Mirrors `ts-src/permissions/checker.ts`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use once_cell::sync::Lazy;
use serde_json::Value;

use super::rules::{
    evaluate_rules, parse_rule, ParsedRule, PermissionConfig, PermissionRule, RuleDecision,
};
use crate::config::loader::{ApprovalMode, PermissionsLayer};
use crate::types::ToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    Read,
    Write,
    Execute,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub decision: Decision,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

static TOOL_CATEGORIES: Lazy<HashMap<&'static str, ToolCategory>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("read_file", ToolCategory::Read);
    m.insert("glob", ToolCategory::Read);
    m.insert("grep", ToolCategory::Read);
    m.insert("ask_user", ToolCategory::Read);
    m.insert("write_file", ToolCategory::Write);
    m.insert("edit_file", ToolCategory::Write);
    m.insert("save_memory", ToolCategory::Write);
    m.insert("bash", ToolCategory::Execute);
    m.insert("web_fetch", ToolCategory::Network);
    m
});

const PATH_ARG_TOOLS: &[&str] = &["read_file", "write_file", "edit_file", "glob", "grep"];

fn mode_matrix(mode: ApprovalMode) -> [(ToolCategory, Decision); 4] {
    match mode {
        ApprovalMode::Plan => [
            (ToolCategory::Read, Decision::Allow),
            (ToolCategory::Network, Decision::Allow),
            (ToolCategory::Write, Decision::Deny),
            (ToolCategory::Execute, Decision::Deny),
        ],
        ApprovalMode::Suggest => [
            (ToolCategory::Read, Decision::Allow),
            (ToolCategory::Network, Decision::Allow),
            (ToolCategory::Write, Decision::Ask),
            (ToolCategory::Execute, Decision::Ask),
        ],
        ApprovalMode::AutoEdit => [
            (ToolCategory::Read, Decision::Allow),
            (ToolCategory::Network, Decision::Allow),
            (ToolCategory::Write, Decision::Allow),
            (ToolCategory::Execute, Decision::Ask),
        ],
        ApprovalMode::FullAuto | ApprovalMode::Yolo => [
            (ToolCategory::Read, Decision::Allow),
            (ToolCategory::Network, Decision::Allow),
            (ToolCategory::Write, Decision::Allow),
            (ToolCategory::Execute, Decision::Allow),
        ],
    }
}

fn lookup_matrix(matrix: &[(ToolCategory, Decision); 4], category: ToolCategory) -> Decision {
    for (cat, dec) in matrix {
        if *cat == category {
            return *dec;
        }
    }
    Decision::Ask
}

pub fn read_only_tool_filter(tools: &[ToolDefinition]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .filter(|t| {
            let cat = TOOL_CATEGORIES
                .get(t.function.name.as_str())
                .copied()
                .unwrap_or(ToolCategory::Execute);
            matches!(cat, ToolCategory::Read | ToolCategory::Network)
        })
        .cloned()
        .collect()
}

pub struct PermissionChecker {
    mode: ApprovalMode,
    always_allowed: HashSet<String>,
    merged_rules: Option<PermissionConfig>,
    project_dir: Option<PathBuf>,
}

impl PermissionChecker {
    pub fn new(
        mode: ApprovalMode,
        layers: Option<&[PermissionsLayer]>,
        project_dir: Option<PathBuf>,
    ) -> Self {
        let merged_rules = layers.and_then(|layers| {
            if layers.is_empty() {
                return None;
            }
            let configs: Vec<PermissionConfig> = layers.iter().map(Self::parse_layer).collect();
            Some(super::rules::merge_configs(&configs))
        });
        Self {
            mode,
            always_allowed: HashSet::new(),
            merged_rules,
            project_dir,
        }
    }

    fn parse_layer(layer: &PermissionsLayer) -> PermissionConfig {
        let parse = |rules: &[String]| -> Vec<PermissionRule> {
            let mut out = Vec::new();
            for raw in rules {
                match parse_rule(raw) {
                    Some(ParsedRule::One(r)) => out.push(r),
                    Some(ParsedRule::Many(rs)) => out.extend(rs),
                    None => {}
                }
            }
            out
        };
        PermissionConfig {
            allow: parse(&layer.allow),
            deny: parse(&layer.deny),
            ask: parse(&layer.ask),
        }
    }

    pub fn check(&self, tool_name: &str, args: Option<&Value>) -> PermissionDecision {
        if matches!(self.mode, ApprovalMode::Yolo) {
            return PermissionDecision {
                decision: Decision::Allow,
                reason: None,
            };
        }

        if let Some(rules) = &self.merged_rules {
            match evaluate_rules(rules, tool_name, args) {
                Some(RuleDecision::Deny) => {
                    return PermissionDecision {
                        decision: Decision::Deny,
                        reason: Some(self.rule_reason(tool_name, RuleDecision::Deny)),
                    };
                }
                Some(RuleDecision::Ask) => {
                    return PermissionDecision {
                        decision: Decision::Ask,
                        reason: Some(self.rule_reason(tool_name, RuleDecision::Ask)),
                    };
                }
                Some(RuleDecision::Allow) => {
                    return PermissionDecision {
                        decision: Decision::Allow,
                        reason: None,
                    };
                }
                None => {}
            }
        }

        let dir_downgrade = self.should_downgrade(tool_name, args);

        if self.always_allowed.contains(tool_name) {
            if dir_downgrade {
                return PermissionDecision {
                    decision: Decision::Ask,
                    reason: Some(format!(
                        "{tool_name} targets path outside project directory"
                    )),
                };
            }
            return PermissionDecision {
                decision: Decision::Allow,
                reason: None,
            };
        }

        let category = TOOL_CATEGORIES
            .get(tool_name)
            .copied()
            .unwrap_or(ToolCategory::Execute);
        let matrix = mode_matrix(self.mode);
        let mut decision = lookup_matrix(&matrix, category);
        if dir_downgrade && decision == Decision::Allow {
            decision = Decision::Ask;
        }

        if decision == Decision::Allow {
            return PermissionDecision {
                decision: Decision::Allow,
                reason: None,
            };
        }
        let reason = format!(
            "{tool_name} ({}) requires approval in {} mode",
            category.name(),
            self.mode.as_str()
        );
        PermissionDecision {
            decision,
            reason: Some(reason),
        }
    }

    pub fn allow_always(&mut self, tool_name: &str) {
        self.always_allowed.insert(tool_name.to_string());
    }

    fn should_downgrade(&self, tool_name: &str, args: Option<&Value>) -> bool {
        let project_dir = match &self.project_dir {
            Some(p) => p,
            None => return false,
        };
        if !PATH_ARG_TOOLS.contains(&tool_name) {
            return false;
        }
        let path_arg = match tool_name {
            "read_file" | "write_file" | "edit_file" => "file_path",
            "glob" | "grep" => "path",
            _ => return false,
        };
        let path = match args.and_then(|a| a.get(path_arg)).and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return false,
        };
        !is_inside_project(project_dir, path)
    }

    fn rule_reason(&self, tool_name: &str, decision: RuleDecision) -> String {
        match decision {
            RuleDecision::Deny => format!("{tool_name} blocked by deny rule"),
            RuleDecision::Ask => format!("{tool_name} requires confirmation (ask rule)"),
            RuleDecision::Allow => String::new(),
        }
    }
}

fn is_inside_project(project_dir: &PathBuf, file_path: &str) -> bool {
    let raw = PathBuf::from(file_path);
    let absolute = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&raw))
            .unwrap_or(raw)
    };
    let resolved = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    let project_resolved =
        std::fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.clone());
    resolved.starts_with(&project_resolved)
}
