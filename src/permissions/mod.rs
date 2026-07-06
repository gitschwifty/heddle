//! Permission system: rule parsing, mode matrix, session allowlist.

pub mod checker;
pub mod defaults;
pub mod rules;

pub use checker::{read_only_tool_filter, PermissionChecker, PermissionDecision, ToolCategory};
pub use defaults::{generate_default_permissions_toml, DEFAULT_DENY_RULES};
pub use rules::{
    evaluate_rules, match_rule, merge_configs, parse_rule, PermissionConfig, PermissionRule,
};
