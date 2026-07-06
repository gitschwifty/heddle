//! Hook matcher predicates.

use globset::Glob;
use serde_json::Value;

use super::types::{HookContext, ResolvedHookDefinition, ToolMatch};

fn glob_match(pattern: &str, target: &str) -> bool {
    match Glob::new(pattern) {
        Ok(g) => g.compile_matcher().is_match(target),
        Err(_) => false,
    }
}

pub fn matches_hook(hook: &ResolvedHookDefinition, ctx: &HookContext) -> bool {
    let matchers = match &hook.matchers {
        Some(m) => m,
        None => return true,
    };

    if let Some(tool) = &matchers.tool {
        let tool_name = match ctx.tool_name.as_deref() {
            Some(n) => n,
            None => return false,
        };
        match tool {
            ToolMatch::Single(s) => {
                if s != tool_name {
                    return false;
                }
            }
            ToolMatch::Many(list) => {
                if !list.iter().any(|s| s == tool_name) {
                    return false;
                }
            }
        }
    }

    if let Some(path_pat) = &matchers.match_path {
        let tool_args = match ctx.tool_args.as_deref() {
            Some(a) => a,
            None => return false,
        };
        let parsed: Result<Value, _> = serde_json::from_str(tool_args);
        let file_path = match parsed {
            Ok(v) => v
                .get("file_path")
                .and_then(|x| x.as_str())
                .map(String::from),
            Err(_) => return false,
        };
        let fp = match file_path {
            Some(p) => p,
            None => return false,
        };
        if !glob_match(path_pat, &fp) {
            return false;
        }
    }

    if let Some(args_pat) = &matchers.match_args {
        let tool_args = match ctx.tool_args.as_deref() {
            Some(a) => a,
            None => return false,
        };
        if !glob_match(args_pat, tool_args) {
            return false;
        }
    }

    if let Some(input_pat) = &matchers.match_input {
        let input = match ctx.user_input.as_deref() {
            Some(i) => i,
            None => return false,
        };
        if !glob_match(input_pat, input) {
            return false;
        }
    }

    true
}
