use heddle::hooks::loader::{load_hooks, merge_hooks_with_ipc};
use heddle::hooks::types::{HookEvent, HookMode, ResolvedHookDefinition, ResolvedHooksConfig};
use toml::Value as TomlValue;

fn toml(s: &str) -> TomlValue {
    s.parse::<TomlValue>().unwrap()
}

fn empty() -> TomlValue {
    "".parse::<TomlValue>().unwrap()
}

#[test]
fn returns_empty_when_no_hooks_section() {
    let result = load_hooks(&empty(), &empty());
    assert!(result.is_empty());
}

#[test]
fn parses_hooks_from_global_config() {
    let global = toml(
        r#"
        [[hooks.pre_tool]]
        command = "lint.sh"
        "#,
    );
    let result = load_hooks(&global, &empty());
    let pre = result.get(&HookEvent::PreTool).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].command, "lint.sh");
}

#[test]
fn parses_hooks_from_local_config() {
    let local = toml(
        r#"
        [[hooks.post_tool]]
        command = "log.sh"
        "#,
    );
    let result = load_hooks(&empty(), &local);
    let post = result.get(&HookEvent::PostTool).unwrap();
    assert_eq!(post.len(), 1);
    assert_eq!(post[0].command, "log.sh");
}

#[test]
fn merges_global_then_local_additively() {
    let global = toml(
        r#"
        [[hooks.pre_tool]]
        command = "global-lint.sh"
        "#,
    );
    let local = toml(
        r#"
        [[hooks.pre_tool]]
        command = "local-lint.sh"
        "#,
    );
    let result = load_hooks(&global, &local);
    let pre = result.get(&HookEvent::PreTool).unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0].command, "global-lint.sh");
    assert_eq!(pre[1].command, "local-lint.sh");
}

#[test]
fn merges_different_event_keys() {
    let global = toml(
        r#"
        [[hooks.pre_tool]]
        command = "pre.sh"
        "#,
    );
    let local = toml(
        r#"
        [[hooks.post_tool]]
        command = "post.sh"
        "#,
    );
    let result = load_hooks(&global, &local);
    assert_eq!(result.get(&HookEvent::PreTool).unwrap().len(), 1);
    assert_eq!(result.get(&HookEvent::PostTool).unwrap().len(), 1);
}

#[test]
fn applies_default_timeout_mode_async() {
    let global = toml(
        r#"
        [[hooks.pre_tool]]
        command = "test.sh"
        "#,
    );
    let result = load_hooks(&global, &empty());
    let h = &result.get(&HookEvent::PreTool).unwrap()[0];
    assert_eq!(h.timeout, 10_000);
    assert_eq!(h.mode, HookMode::Both);
    assert!(!h.r#async);
}

#[test]
fn ignores_invalid_event_keys() {
    let global = toml(
        r#"
        [[hooks.invalid_event]]
        command = "x.sh"
        "#,
    );
    let result = load_hooks(&global, &empty());
    assert!(result.is_empty());
}

#[test]
fn filters_out_entries_without_command() {
    let global = toml(
        r#"
        [[hooks.pre_tool]]
        command = "valid.sh"

        [[hooks.pre_tool]]
        timeout = 5000
        "#,
    );
    let result = load_hooks(&global, &empty());
    let pre = result.get(&HookEvent::PreTool).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].command, "valid.sh");
}

fn def(cmd: &str) -> ResolvedHookDefinition {
    ResolvedHookDefinition {
        command: cmd.to_string(),
        timeout: 10_000,
        mode: HookMode::Both,
        r#async: false,
        matchers: None,
    }
}

#[test]
fn ipc_hooks_override_per_event() {
    let mut file_hooks = ResolvedHooksConfig::new();
    file_hooks.insert(HookEvent::PreTool, vec![def("file-hook.sh")]);
    let mut ipc_hooks = ResolvedHooksConfig::new();
    ipc_hooks.insert(HookEvent::PreTool, vec![def("ipc-hook.sh")]);
    let result = merge_hooks_with_ipc(file_hooks, ipc_hooks);
    let pre = result.get(&HookEvent::PreTool).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].command, "ipc-hook.sh");
}

#[test]
fn preserves_file_hooks_for_events_not_in_ipc() {
    let mut file_hooks = ResolvedHooksConfig::new();
    file_hooks.insert(HookEvent::PreTool, vec![def("file-pre.sh")]);
    file_hooks.insert(HookEvent::PostTool, vec![def("file-post.sh")]);
    let mut ipc_hooks = ResolvedHooksConfig::new();
    ipc_hooks.insert(HookEvent::PreTool, vec![def("ipc-pre.sh")]);
    let result = merge_hooks_with_ipc(file_hooks, ipc_hooks);
    assert_eq!(
        result.get(&HookEvent::PreTool).unwrap()[0].command,
        "ipc-pre.sh"
    );
    assert_eq!(
        result.get(&HookEvent::PostTool).unwrap()[0].command,
        "file-post.sh"
    );
}

#[test]
fn empty_ipc_returns_file_hooks_unchanged() {
    let mut file_hooks = ResolvedHooksConfig::new();
    file_hooks.insert(HookEvent::PreTool, vec![def("file.sh")]);
    let snapshot = file_hooks.clone();
    let result = merge_hooks_with_ipc(file_hooks, ResolvedHooksConfig::new());
    assert_eq!(
        result.get(&HookEvent::PreTool).unwrap()[0].command,
        snapshot.get(&HookEvent::PreTool).unwrap()[0].command
    );
}
