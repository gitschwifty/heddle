use std::path::PathBuf;

use heddle::config::loader::{ApprovalMode, PermissionsLayer};
use heddle::permissions::checker::{read_only_tool_filter, Decision, PermissionChecker};
use heddle::types::{ToolCallKind, ToolDefinition, ToolFunction};
use serde_json::json;

mod common;

fn layer(allow: &[&str], deny: &[&str], ask: &[&str]) -> PermissionsLayer {
    PermissionsLayer {
        allow: allow.iter().map(|s| s.to_string()).collect(),
        deny: deny.iter().map(|s| s.to_string()).collect(),
        ask: ask.iter().map(|s| s.to_string()).collect(),
    }
}

// ── suggest mode ──

#[test]
fn suggest_allows_read() {
    let c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    assert_eq!(c.check("read_file", None).decision, Decision::Allow);
    assert_eq!(c.check("glob", None).decision, Decision::Allow);
    assert_eq!(c.check("grep", None).decision, Decision::Allow);
}

#[test]
fn suggest_asks_for_write() {
    let c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    assert_eq!(c.check("write_file", None).decision, Decision::Ask);
}

#[test]
fn suggest_asks_for_execute() {
    let c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    assert_eq!(c.check("bash", None).decision, Decision::Ask);
}

#[test]
fn suggest_allows_network() {
    let c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    assert_eq!(c.check("web_fetch", None).decision, Decision::Allow);
}

// ── auto-edit mode ──

#[test]
fn auto_edit_allows_read_write() {
    let c = PermissionChecker::new(ApprovalMode::AutoEdit, None, None);
    assert_eq!(c.check("read_file", None).decision, Decision::Allow);
    assert_eq!(c.check("write_file", None).decision, Decision::Allow);
    assert_eq!(c.check("edit_file", None).decision, Decision::Allow);
}

#[test]
fn auto_edit_asks_for_execute() {
    let c = PermissionChecker::new(ApprovalMode::AutoEdit, None, None);
    assert_eq!(c.check("bash", None).decision, Decision::Ask);
}

// ── full-auto mode ──

#[test]
fn full_auto_allows_all() {
    let c = PermissionChecker::new(ApprovalMode::FullAuto, None, None);
    assert_eq!(c.check("read_file", None).decision, Decision::Allow);
    assert_eq!(c.check("write_file", None).decision, Decision::Allow);
    assert_eq!(c.check("bash", None).decision, Decision::Allow);
    assert_eq!(c.check("web_fetch", None).decision, Decision::Allow);
}

// ── plan mode ──

#[test]
fn plan_denies_write_and_execute() {
    let c = PermissionChecker::new(ApprovalMode::Plan, None, None);
    let r = c.check("write_file", None);
    assert_eq!(r.decision, Decision::Deny);
    assert!(r.reason.is_some());
    let r = c.check("bash", None);
    assert_eq!(r.decision, Decision::Deny);
}

#[test]
fn plan_allows_read_and_network() {
    let c = PermissionChecker::new(ApprovalMode::Plan, None, None);
    assert_eq!(c.check("read_file", None).decision, Decision::Allow);
    assert_eq!(c.check("web_fetch", None).decision, Decision::Allow);
}

// ── yolo mode ──

#[test]
fn yolo_allows_everything() {
    let c = PermissionChecker::new(ApprovalMode::Yolo, None, None);
    assert_eq!(c.check("bash", None).decision, Decision::Allow);
    assert_eq!(c.check("write_file", None).decision, Decision::Allow);
}

#[test]
fn yolo_ignores_deny_rules() {
    let l = layer(&[], &["Write(.env*)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::Yolo, Some(&[l]), None);
    let args = json!({"path": ".env"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Allow);
}

#[test]
fn yolo_ignores_ask_rules() {
    let l = layer(&[], &[], &["Bash(git push *)"]);
    let c = PermissionChecker::new(ApprovalMode::Yolo, Some(&[l]), None);
    let args = json!({"command": "git push origin main"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Allow);
}

// ── deny rules ──

#[test]
fn env_deny_blocks_writes() {
    let l = layer(&[], &["Write(.env*)", "Edit(.env*)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"path": "/project/.env"});
    let r = c.check("write_file", Some(&args));
    assert_eq!(r.decision, Decision::Deny);
    assert!(r.reason.is_some());
}

#[test]
fn env_deny_glob_match() {
    let l = layer(&[], &["Write(.env*)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"path": "/project/.env.local"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Deny);
}

#[test]
fn env_deny_allows_other_files() {
    let l = layer(&[], &["Write(.env*)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"path": "/project/not-env-file.txt"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Allow);
}

#[test]
fn rm_deny_blocks_rm() {
    let l = layer(&[], &["Bash(rm *)", "Bash(rm)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"command": "rm file.txt"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Deny);
}

#[test]
fn rm_deny_allows_other_bash() {
    let l = layer(&[], &["Bash(rm *)", "Bash(rm)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"command": "ls -la"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Allow);
}

// ── ask rules ──

#[test]
fn ask_rule_forces_prompt_in_full_auto() {
    let l = layer(&[], &[], &["Bash(git push *)"]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"command": "git push origin main"});
    let r = c.check("bash", Some(&args));
    assert_eq!(r.decision, Decision::Ask);
    assert!(r.reason.is_some());
}

#[test]
fn ask_rule_does_not_affect_unmatched_command() {
    let l = layer(&[], &[], &["Bash(git push *)"]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"command": "git status"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Allow);
}

#[test]
fn deny_priority_over_ask() {
    let l = layer(&[], &["Bash(rm *)"], &["Bash(rm *)"]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"command": "rm foo"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Deny);
}

// ── allow rules ──

#[test]
fn allow_overrides_mode_ask() {
    let l = layer(&["Bash(bun *)"], &[], &[]);
    let c = PermissionChecker::new(ApprovalMode::Suggest, Some(&[l]), None);
    let args = json!({"command": "bun test"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Allow);
}

#[test]
fn allow_does_not_override_deny() {
    let l = layer(&["Write(.env*)"], &["Write(.env*)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"path": ".env"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Deny);
}

// ── unknown tools ──

#[test]
fn unknown_tool_defaults_to_execute_suggest() {
    let c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    assert_eq!(c.check("unknown_future_tool", None).decision, Decision::Ask);
}

#[test]
fn unknown_tool_defaults_to_execute_plan_deny() {
    let c = PermissionChecker::new(ApprovalMode::Plan, None, None);
    assert_eq!(
        c.check("unknown_future_tool", None).decision,
        Decision::Deny
    );
}

// ── allow_always ──

#[test]
fn allow_always_bypasses_ask() {
    let mut c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    assert_eq!(c.check("bash", None).decision, Decision::Ask);
    c.allow_always("bash");
    assert_eq!(c.check("bash", None).decision, Decision::Allow);
}

#[test]
fn allow_always_does_not_bypass_deny() {
    let l = layer(&[], &["Write(.env*)"], &[]);
    let mut c = PermissionChecker::new(ApprovalMode::Suggest, Some(&[l]), None);
    c.allow_always("write_file");
    let args = json!({"path": "foo.txt"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Allow);
    let env_args = json!({"path": ".env"});
    assert_eq!(
        c.check("write_file", Some(&env_args)).decision,
        Decision::Deny
    );
}

// ── reasons ──

#[test]
fn deny_returns_a_reason() {
    let c = PermissionChecker::new(ApprovalMode::Plan, None, None);
    let r = c.check("write_file", None);
    assert_eq!(r.decision, Decision::Deny);
    assert!(r.reason.unwrap().len() > 0);
}

#[test]
fn ask_returns_a_reason() {
    let c = PermissionChecker::new(ApprovalMode::Suggest, None, None);
    let r = c.check("bash", None);
    assert_eq!(r.decision, Decision::Ask);
    assert!(r.reason.is_some());
}

// ── layer merge ──

#[test]
fn project_allow_overrides_global_deny() {
    let global = layer(&[], &["Write(.env*)"], &[]);
    let project = layer(&["Write(.env*)"], &[], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[global, project]), None);
    let args = json!({"path": ".env.local"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Allow);
}

#[test]
fn project_deny_overrides_global_allow() {
    let global = layer(&["Bash"], &[], &[]);
    let project = layer(&[], &["Bash(rm *)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[global, project]), None);
    let args = json!({"command": "rm foo"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Deny);
}

#[test]
fn within_same_layer_deny_wins() {
    let l = layer(&["Write(.env*)"], &["Write(.env*)"], &[]);
    let c = PermissionChecker::new(ApprovalMode::FullAuto, Some(&[l]), None);
    let args = json!({"path": ".env"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Deny);
}

// ── readOnlyToolFilter ──

fn td(name: &str) -> ToolDefinition {
    ToolDefinition {
        kind: ToolCallKind::Function,
        function: ToolFunction {
            name: name.to_string(),
            description: name.to_string(),
            parameters: json!({}),
        },
    }
}

#[test]
fn read_only_filter_keeps_read_and_network() {
    let all = vec![
        td("read_file"),
        td("glob"),
        td("grep"),
        td("ask_user"),
        td("web_fetch"),
        td("write_file"),
        td("edit_file"),
        td("bash"),
    ];
    let filtered = read_only_tool_filter(&all);
    let names: Vec<&str> = filtered.iter().map(|t| t.function.name.as_str()).collect();
    for kept in ["read_file", "glob", "grep", "ask_user", "web_fetch"] {
        assert!(names.contains(&kept), "missing {kept}");
    }
}

#[test]
fn read_only_filter_removes_write_and_execute() {
    let all = vec![
        td("read_file"),
        td("write_file"),
        td("edit_file"),
        td("bash"),
    ];
    let filtered = read_only_tool_filter(&all);
    let names: Vec<&str> = filtered.iter().map(|t| t.function.name.as_str()).collect();
    assert!(!names.contains(&"write_file"));
    assert!(!names.contains(&"edit_file"));
    assert!(!names.contains(&"bash"));
}

// ── directory scoping (uses real filesystem under TMPDIR) ──

#[test]
fn dir_scope_allow_inside_project() {
    let tmp = std::env::temp_dir().join(format!("heddle-perm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let project = std::fs::canonicalize(&tmp).unwrap();
    let inside = project.join("src/foo.ts");
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    std::fs::write(&inside, "").unwrap();

    let c = PermissionChecker::new(ApprovalMode::FullAuto, None, Some(project.clone()));
    let args = json!({"path": inside.to_string_lossy()});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Allow);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn dir_scope_downgrades_outside() {
    let tmp = std::env::temp_dir().join(format!("heddle-perm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let project = std::fs::canonicalize(&tmp).unwrap();
    let outside = std::env::temp_dir().join("not-the-project.txt");
    std::fs::write(&outside, "").unwrap();

    let c = PermissionChecker::new(ApprovalMode::FullAuto, None, Some(project.clone()));
    let args = json!({"path": outside.to_string_lossy()});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Ask);
    let _ = std::fs::remove_dir_all(&project);
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn no_dir_scope_for_bash() {
    let c = PermissionChecker::new(
        ApprovalMode::FullAuto,
        None,
        Some(PathBuf::from("/project")),
    );
    let args = json!({"command": "ls /etc"});
    assert_eq!(c.check("bash", Some(&args)).decision, Decision::Allow);
}

#[test]
fn no_dir_scope_for_web_fetch() {
    let c = PermissionChecker::new(
        ApprovalMode::FullAuto,
        None,
        Some(PathBuf::from("/project")),
    );
    let args = json!({"url": "https://example.com"});
    assert_eq!(c.check("web_fetch", Some(&args)).decision, Decision::Allow);
}

#[test]
fn no_project_dir_no_scoping() {
    let c = PermissionChecker::new(ApprovalMode::FullAuto, None, None);
    let args = json!({"path": "/random/path/foo.ts"});
    assert_eq!(c.check("write_file", Some(&args)).decision, Decision::Allow);
}
