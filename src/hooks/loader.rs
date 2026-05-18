//! Parse hooks from raw TOML and merge global/local layers.
//! Mirrors `ts-src/hooks/loader.ts`.

use toml::Value as TomlValue;

use super::types::{
    HookEvent, HookMatchers, HookMode, ResolvedHookDefinition, ResolvedHooksConfig, ToolMatch,
};

fn to_hook_definition(raw: &TomlValue) -> Option<ResolvedHookDefinition> {
    let table = raw.as_table()?;
    let command = table.get("command")?.as_str()?.to_string();

    let timeout = table
        .get("timeout")
        .and_then(|v| v.as_integer())
        .map(|n| n as u64)
        .unwrap_or(10_000);

    let mode = table
        .get("mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "interactive" => HookMode::Interactive,
            "headless" => HookMode::Headless,
            _ => HookMode::Both,
        })
        .unwrap_or(HookMode::Both);

    let r#async = table
        .get("async")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut matchers = HookMatchers::default();
    let mut has_matchers = false;
    if let Some(m) = table.get("matchers").and_then(|v| v.as_table()) {
        if let Some(tool) = m.get("tool") {
            if let Some(s) = tool.as_str() {
                matchers.tool = Some(ToolMatch::Single(s.to_string()));
                has_matchers = true;
            } else if let Some(arr) = tool.as_array() {
                let v: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect();
                if !v.is_empty() {
                    matchers.tool = Some(ToolMatch::Many(v));
                    has_matchers = true;
                }
            }
        }
        if let Some(s) = m.get("match_path").and_then(|v| v.as_str()) {
            matchers.match_path = Some(s.to_string());
            has_matchers = true;
        }
        if let Some(s) = m.get("match_args").and_then(|v| v.as_str()) {
            matchers.match_args = Some(s.to_string());
            has_matchers = true;
        }
        if let Some(s) = m.get("match_input").and_then(|v| v.as_str()) {
            matchers.match_input = Some(s.to_string());
            has_matchers = true;
        }
    }

    Some(ResolvedHookDefinition {
        command,
        timeout,
        mode,
        r#async,
        matchers: if has_matchers { Some(matchers) } else { None },
    })
}

fn extract_hooks(raw: &TomlValue) -> ResolvedHooksConfig {
    let mut out = ResolvedHooksConfig::new();
    let table = match raw.as_table() {
        Some(t) => t,
        None => return out,
    };
    let hooks = match table.get("hooks").and_then(|v| v.as_table()) {
        Some(h) => h,
        None => return out,
    };
    for (event_name, entries) in hooks.iter() {
        let event = match event_name.parse::<HookEvent>() {
            Ok(e) => e,
            Err(_) => continue,
        };
        let arr = match entries.as_array() {
            Some(a) => a,
            None => continue,
        };
        let mut parsed = Vec::new();
        for entry in arr {
            if let Some(def) = to_hook_definition(entry) {
                parsed.push(def);
            }
        }
        if !parsed.is_empty() {
            out.insert(event, parsed);
        }
    }
    out
}

pub fn load_hooks(global_raw: &TomlValue, local_raw: &TomlValue) -> ResolvedHooksConfig {
    let global_hooks = extract_hooks(global_raw);
    let local_hooks = extract_hooks(local_raw);

    let mut merged = global_hooks;
    for (event, local_arr) in local_hooks {
        merged
            .entry(event)
            .and_modify(|existing| existing.extend(local_arr.iter().cloned()))
            .or_insert(local_arr);
    }
    merged
}

/// Merge file-based hooks with IPC hooks. IPC hooks _replace_ per-event.
pub fn merge_hooks_with_ipc(
    file_hooks: ResolvedHooksConfig,
    ipc_hooks: ResolvedHooksConfig,
) -> ResolvedHooksConfig {
    let mut merged = file_hooks;
    for (event, hooks) in ipc_hooks {
        merged.insert(event, hooks);
    }
    merged
}
