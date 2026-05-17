use heddle::hooks::runner::HooksRunner;
use heddle::hooks::types::{
    HookContext, HookEvent, HookMatchers, HookMode, ResolvedHookDefinition, ResolvedHooksConfig,
    ToolMatch,
};
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

fn make_script(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path.to_string_lossy().into_owned()
}

fn hook(command: &str, mode: HookMode, r#async: bool) -> ResolvedHookDefinition {
    ResolvedHookDefinition {
        command: command.to_string(),
        timeout: 5000,
        mode,
        r#async,
        matchers: None,
    }
}

fn make_runner(config: ResolvedHooksConfig) -> HooksRunner {
    HooksRunner::new(
        config,
        HookMode::Interactive,
        "sess-int".to_string(),
        "/test/project".to_string(),
        "test-model".to_string(),
    )
}

#[tokio::test]
async fn pre_tool_blocking_marks_result_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(
        &dir,
        "block.sh",
        "#!/bin/sh\necho \"blocked by policy\" >&2\nexit 1",
    );
    let mut cfg = ResolvedHooksConfig::new();
    cfg.insert(
        HookEvent::PreTool,
        vec![hook(&script, HookMode::Both, false)],
    );
    let runner = make_runner(cfg);

    let mut ctx = HookContext::default();
    ctx.tool_name = Some("bash".to_string());
    ctx.tool_args = Some(r#"{"command":"rm -rf /"}"#.to_string());
    let results = runner.run(HookEvent::PreTool, ctx).await;
    let blocked = results.iter().find(|r| r.blocked).expect("blocked result");
    assert_eq!(blocked.reason.as_deref(), Some("blocked by policy"));
}

#[tokio::test]
async fn post_tool_feedback_collected_from_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(
        &dir,
        "post.sh",
        "#!/bin/sh\necho \"[hook] file was written\"",
    );
    let mut cfg = ResolvedHooksConfig::new();
    cfg.insert(
        HookEvent::PostTool,
        vec![hook(&script, HookMode::Both, false)],
    );
    let runner = make_runner(cfg);

    let mut ctx = HookContext::default();
    ctx.tool_name = Some("write".to_string());
    ctx.tool_result = Some("File written successfully".to_string());
    let results = runner.run(HookEvent::PostTool, ctx).await;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].feedback.as_deref(),
        Some("[hook] file was written")
    );
    assert!(!results[0].blocked);
}

#[tokio::test]
async fn pre_prompt_can_block_user_input() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(
        &dir,
        "block-prompt.sh",
        "#!/bin/sh\ninput=$(cat)\necho \"$input\" | grep -q deploy && { echo \"deployments are disabled\" >&2; exit 1; }\nexit 0",
    );
    let mut cfg = ResolvedHooksConfig::new();
    cfg.insert(
        HookEvent::PrePrompt,
        vec![hook(&script, HookMode::Both, false)],
    );
    let runner = make_runner(cfg);

    let mut blocked_ctx = HookContext::default();
    blocked_ctx.user_input = Some("deploy to production".to_string());
    let blocked = runner.run(HookEvent::PrePrompt, blocked_ctx).await;
    assert!(blocked.iter().any(|r| r.blocked));

    let mut ok_ctx = HookContext::default();
    ok_ctx.user_input = Some("write some tests".to_string());
    let allowed = runner.run(HookEvent::PrePrompt, ok_ctx).await;
    assert!(allowed.iter().all(|r| !r.blocked));
}

#[tokio::test]
async fn mixed_sync_and_async_only_returns_sync_results() {
    let dir = tempfile::tempdir().unwrap();
    let sync = make_script(&dir, "sync.sh", "#!/bin/sh\necho sync feedback");
    let r#async = make_script(&dir, "async.sh", "#!/bin/sh\nsleep 0.1");
    let mut cfg = ResolvedHooksConfig::new();
    cfg.insert(
        HookEvent::PostTool,
        vec![
            hook(&sync, HookMode::Both, false),
            hook(&r#async, HookMode::Both, true),
        ],
    );
    let runner = make_runner(cfg);
    let mut ctx = HookContext::default();
    ctx.tool_name = Some("read".to_string());
    let results = runner.run(HookEvent::PostTool, ctx).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].feedback.as_deref(), Some("sync feedback"));
}

#[tokio::test]
async fn matchers_filter_hooks_before_execution() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "matched.sh", "#!/bin/sh\necho matched");
    let mut cfg = ResolvedHooksConfig::new();
    let mut h = hook(&script, HookMode::Both, false);
    h.matchers = Some(HookMatchers {
        tool: Some(ToolMatch::Single("write".to_string())),
        ..Default::default()
    });
    cfg.insert(HookEvent::PreTool, vec![h]);
    let runner = make_runner(cfg);

    let mut read_ctx = HookContext::default();
    read_ctx.tool_name = Some("read".to_string());
    let read_results = runner.run(HookEvent::PreTool, read_ctx).await;
    assert!(read_results.is_empty(), "read should not match");

    let mut write_ctx = HookContext::default();
    write_ctx.tool_name = Some("write".to_string());
    let write_results = runner.run(HookEvent::PreTool, write_ctx).await;
    assert_eq!(write_results.len(), 1);
    assert_eq!(write_results[0].feedback.as_deref(), Some("matched"));
}
