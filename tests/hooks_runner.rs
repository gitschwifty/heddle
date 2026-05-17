use heddle::hooks::runner::HooksRunner;
use heddle::hooks::types::{
    HookContext, HookEvent, HookMode, ResolvedHookDefinition, ResolvedHooksConfig,
};
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;
use tempfile::TempDir;

fn make_script(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path.to_string_lossy().into_owned()
}

fn hook(command: &str, timeout: u64, mode: HookMode, r#async: bool) -> ResolvedHookDefinition {
    ResolvedHookDefinition {
        command: command.to_string(),
        timeout,
        mode,
        r#async,
        matchers: None,
    }
}

fn make_runner(config: ResolvedHooksConfig, mode: HookMode) -> HooksRunner {
    HooksRunner::new(
        config,
        mode,
        "sess-123".to_string(),
        "/test/project".to_string(),
        "test-model".to_string(),
    )
}

fn config_with(event: HookEvent, h: ResolvedHookDefinition) -> ResolvedHooksConfig {
    let mut c = ResolvedHooksConfig::new();
    c.insert(event, vec![h]);
    c
}

#[tokio::test]
async fn successful_hook_returns_stdout_as_feedback() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "ok.sh", "#!/bin/sh\necho \"all good\"");
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 5000, HookMode::Both, false),
        ),
        HookMode::Interactive,
    );
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert_eq!(results.len(), 1);
    assert!(!results[0].blocked);
    assert_eq!(results[0].feedback.as_deref(), Some("all good"));
    assert!(!results[0].timed_out);
}

#[tokio::test]
async fn blocking_hook_returns_stderr_as_reason() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(
        &dir,
        "block.sh",
        "#!/bin/sh\necho \"forbidden\" >&2\nexit 1",
    );
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 5000, HookMode::Both, false),
        ),
        HookMode::Interactive,
    );
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].blocked);
    assert_eq!(results[0].reason.as_deref(), Some("forbidden"));
    assert!(!results[0].timed_out);
}

#[tokio::test]
async fn timeout_sets_timed_out_flag() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "slow.sh", "#!/bin/sh\nsleep 30");
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 200, HookMode::Both, false),
        ),
        HookMode::Interactive,
    );
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].timed_out);
    assert!(!results[0].blocked);
}

#[tokio::test]
async fn interactive_only_hook_skipped_in_headless() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "interactive-only.sh", "#!/bin/sh\necho \"x\"");
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 5000, HookMode::Interactive, false),
        ),
        HookMode::Headless,
    );
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn both_mode_runs_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "both.sh", "#!/bin/sh\necho \"x\"");
    let cfg = config_with(
        HookEvent::PreTool,
        hook(&script, 5000, HookMode::Both, false),
    );
    let interactive = make_runner(cfg.clone(), HookMode::Interactive);
    let headless = make_runner(cfg, HookMode::Headless);
    assert_eq!(
        interactive
            .run(HookEvent::PreTool, HookContext::default())
            .await
            .len(),
        1
    );
    assert_eq!(
        headless
            .run(HookEvent::PreTool, HookContext::default())
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn env_vars_are_set() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(
        &dir,
        "env.sh",
        "#!/bin/sh\necho \"$HEDDLE_HOOK_EVENT|$HEDDLE_HOOK_SESSION_ID|$HEDDLE_HOOK_PROJECT|$HEDDLE_HOOK_MODEL|$HEDDLE_HOOK_TOOL_NAME\"",
    );
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 5000, HookMode::Both, false),
        ),
        HookMode::Interactive,
    );
    let mut ctx = HookContext::default();
    ctx.tool_name = Some("read".to_string());
    let results = runner.run(HookEvent::PreTool, ctx).await;
    assert_eq!(
        results[0].feedback.as_deref(),
        Some("pre_tool|sess-123|/test/project|test-model|read")
    );
}

#[tokio::test]
async fn stdin_pipes_json_payload() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "stdin.sh", "#!/bin/sh\ncat");
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 5000, HookMode::Both, false),
        ),
        HookMode::Interactive,
    );
    let mut ctx = HookContext::default();
    ctx.tool_args = Some(r#"{"file_path":"test.ts"}"#.to_string());
    ctx.tool_result = Some("file contents here".to_string());
    ctx.user_input = Some("read the file".to_string());
    let results = runner.run(HookEvent::PreTool, ctx).await;
    let parsed: serde_json::Value =
        serde_json::from_str(results[0].feedback.as_ref().unwrap()).unwrap();
    assert_eq!(parsed["tool_args"], r#"{"file_path":"test.ts"}"#);
    assert_eq!(parsed["tool_result"], "file contents here");
    assert_eq!(parsed["user_input"], "read the file");
}

#[tokio::test]
async fn async_hooks_return_no_results_and_dont_block() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "async.sh", "#!/bin/sh\nsleep 5");
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 10000, HookMode::Both, true),
        ),
        HookMode::Interactive,
    );
    let start = Instant::now();
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    let elapsed = start.elapsed();
    assert!(results.is_empty());
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "should return fast; took {elapsed:?}"
    );
}

#[tokio::test]
async fn async_hook_with_failure_still_returns_no_results() {
    let dir = tempfile::tempdir().unwrap();
    let script = make_script(&dir, "async-fail.sh", "#!/bin/sh\necho err >&2\nexit 1");
    let runner = make_runner(
        config_with(
            HookEvent::PreTool,
            hook(&script, 5000, HookMode::Both, true),
        ),
        HookMode::Interactive,
    );
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn no_hooks_for_event_returns_empty() {
    let runner = make_runner(ResolvedHooksConfig::new(), HookMode::Interactive);
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn multiple_sync_hooks_run_sequentially() {
    let dir = tempfile::tempdir().unwrap();
    let s1 = make_script(&dir, "seq1.sh", "#!/bin/sh\necho first");
    let s2 = make_script(&dir, "seq2.sh", "#!/bin/sh\necho second");
    let mut cfg = ResolvedHooksConfig::new();
    cfg.insert(
        HookEvent::PreTool,
        vec![
            hook(&s1, 5000, HookMode::Both, false),
            hook(&s2, 5000, HookMode::Both, false),
        ],
    );
    let runner = make_runner(cfg, HookMode::Interactive);
    let results = runner.run(HookEvent::PreTool, HookContext::default()).await;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].feedback.as_deref(), Some("first"));
    assert_eq!(results[1].feedback.as_deref(), Some("second"));
}
