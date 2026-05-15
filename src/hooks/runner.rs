//! HooksRunner — spawns hook commands and collects their results.
//! Mirrors `ts-src/hooks/runner.ts`.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

use super::matcher::matches_hook;
use super::types::{
    HookContext, HookEvent, HookMode, HookResult, ResolvedHookDefinition, ResolvedHooksConfig,
};
use crate::debug::debug;

pub struct HooksRunner {
    pub config: ResolvedHooksConfig,
    pub mode: HookMode,
    pub session_id: String,
    pub project: String,
    pub model: String,
}

impl HooksRunner {
    pub fn new(
        config: ResolvedHooksConfig,
        mode: HookMode,
        session_id: String,
        project: String,
        model: String,
    ) -> Self {
        Self {
            config,
            mode,
            session_id,
            project,
            model,
        }
    }

    pub async fn run(&self, event: HookEvent, mut context: HookContext) -> Vec<HookResult> {
        let hooks = match self.config.get(&event) {
            Some(h) if !h.is_empty() => h,
            _ => return Vec::new(),
        };

        context.session_id = self.session_id.clone();
        context.project = self.project.clone();
        context.model = self.model.clone();
        context.event = event.as_str().to_string();

        // Filter by mode
        let mode_matched: Vec<&ResolvedHookDefinition> = hooks
            .iter()
            .filter(|h| matches!(h.mode, HookMode::Both) || h.mode == self.mode)
            .collect();

        // Filter by matchers
        let matched: Vec<&ResolvedHookDefinition> = mode_matched
            .into_iter()
            .filter(|h| matches_hook(h, &context))
            .collect();

        // Fire async hooks (fire-and-forget)
        for hook in matched.iter().filter(|h| h.r#async) {
            let hook = (*hook).clone();
            let ctx = context.clone();
            tokio::spawn(async move {
                execute_async(&hook, &ctx).await;
            });
        }

        // Sync hooks sequentially
        let mut results = Vec::new();
        for hook in matched.iter().filter(|h| !h.r#async) {
            let result = execute_sync(hook, &context).await;
            results.push(result);
        }
        results
    }
}

fn build_env(context: &HookContext) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("HEDDLE_HOOK_EVENT".to_string(), context.event.clone());
    env.insert(
        "HEDDLE_HOOK_SESSION_ID".to_string(),
        context.session_id.clone(),
    );
    env.insert("HEDDLE_HOOK_PROJECT".to_string(), context.project.clone());
    env.insert("HEDDLE_HOOK_MODEL".to_string(), context.model.clone());
    if let Some(t) = &context.tool_name {
        env.insert("HEDDLE_HOOK_TOOL_NAME".to_string(), t.clone());
    }
    env
}

fn build_stdin(context: &HookContext) -> Option<String> {
    let mut map = serde_json::Map::new();
    if let Some(v) = &context.tool_args {
        map.insert(
            "tool_args".to_string(),
            serde_json::Value::String(v.clone()),
        );
    }
    if let Some(v) = &context.tool_result {
        map.insert(
            "tool_result".to_string(),
            serde_json::Value::String(v.clone()),
        );
    }
    if let Some(v) = &context.user_input {
        map.insert(
            "user_input".to_string(),
            serde_json::Value::String(v.clone()),
        );
    }
    if map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(map).to_string())
    }
}

async fn execute_sync(hook: &ResolvedHookDefinition, ctx: &HookContext) -> HookResult {
    let env = build_env(ctx);
    let stdin_data = build_stdin(ctx);

    let mut cmd = Command::new("sh");
    cmd.args(["-c", &hook.command])
        .envs(env.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            debug("hooks", &format!("Hook spawn error: {e}"));
            return HookResult::default();
        }
    };

    if let Some(data) = stdin_data {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(data.as_bytes()).await;
        }
    }
    if let Some(stdin) = child.stdin.take() {
        drop(stdin);
    }

    let timeout_dur = Duration::from_millis(hook.timeout);
    let waited = timeout(timeout_dur, child.wait_with_output()).await;

    let output = match waited {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            debug("hooks", &format!("Hook wait error: {e}"));
            return HookResult::default();
        }
        Err(_) => {
            // Timed out — kill is implicit via tokio when child is dropped, but
            // we won't have a kill handle here. The contract says "timed_out".
            return HookResult {
                timed_out: true,
                ..Default::default()
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let code = output.status.code().unwrap_or(-1);

    if code != 0 {
        return HookResult {
            blocked: true,
            reason: Some(if stderr.is_empty() {
                format!("Hook exited with code {code}")
            } else {
                stderr
            }),
            ..Default::default()
        };
    }
    HookResult {
        feedback: if stdout.is_empty() {
            None
        } else {
            Some(stdout)
        },
        ..Default::default()
    }
}

async fn execute_async(hook: &ResolvedHookDefinition, ctx: &HookContext) {
    let env = build_env(ctx);
    let stdin_data = build_stdin(ctx);

    let mut cmd = Command::new("sh");
    cmd.args(["-c", &hook.command])
        .envs(env.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            debug("hooks", &format!("Async hook spawn error: {e}"));
            return;
        }
    };

    if let Some(data) = stdin_data {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(data.as_bytes()).await;
        }
    }
    drop(child.stdin.take());

    let timeout_dur = Duration::from_millis(hook.timeout);
    let _ = timeout(timeout_dur, child.wait_with_output()).await;
}
