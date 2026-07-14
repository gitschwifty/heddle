//! Core agent loop. Two variants: streaming and non-streaming. Both append to
//! `messages` in place, yield events, and stop on text-only response, error,
//! cancellation, doom loop, or max-iterations.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use futures::Stream;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::types::AgentEvent;
use crate::debug::debug;
use crate::hooks::runner::HooksRunner;
use crate::hooks::types::{HookContext, HookEvent};
use crate::permissions::checker::{Decision, PermissionChecker};
use crate::provider::types::Provider;
use crate::tools::registry::ToolRegistry;
use crate::tools::types::ExecOptions;
use crate::types::{
    AssistantMessage, FunctionCall, Message, ToolCall, ToolCallKind, ToolDefinition, ToolMessage,
};

const DEFAULT_MAX_ITERATIONS: u32 = 20;
const DEFAULT_DOOM_LOOP_THRESHOLD: u32 = 3;
const DEFAULT_EMPTY_RESPONSE_RETRIES: u32 = 1;

/// Returned by the permission resolver callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionResponse {
    Allow,
    Deny,
    Always,
}

pub type PermissionResolver = Arc<
    dyn Fn(
            String,
            ToolCall,
            Option<String>,
        ) -> Pin<Box<dyn Future<Output = PermissionResponse> + Send>>
        + Send
        + Sync,
>;

pub type ToolFilter = Arc<dyn Fn(&[ToolDefinition]) -> Vec<ToolDefinition> + Send + Sync>;

#[derive(Default, Clone)]
pub struct AgentLoopOptions {
    pub max_iterations: Option<u32>,
    pub doom_loop_threshold: Option<u32>,
    pub request_overrides: Option<Value>,
    pub permission_checker: Option<Arc<Mutex<PermissionChecker>>>,
    pub permission_resolver: Option<PermissionResolver>,
    pub tool_filter: Option<ToolFilter>,
    pub plan_mode: bool,
    pub signal: Option<CancellationToken>,
    pub hooks_runner: Option<Arc<HooksRunner>>,
}

fn hash_tool_calls(tool_calls: &[ToolCall]) -> String {
    tool_calls
        .iter()
        .map(|tc| {
            let normalized = serde_json::from_str::<Value>(&tc.function.arguments)
                .ok()
                .map(|v| v.to_string())
                .unwrap_or_else(|| tc.function.arguments.clone());
            format!("{}:{}", tc.function.name, normalized)
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn is_doom_loop(hashes: &[String], threshold: u32) -> bool {
    if (hashes.len() as u32) < threshold {
        return false;
    }
    let last = match hashes.last() {
        Some(l) => l,
        None => return false,
    };
    hashes
        .iter()
        .rev()
        .take(threshold as usize)
        .all(|h| h == last)
}

struct PermissionOutcome {
    events: Vec<AgentEvent>,
    tool_message: Option<ToolMessage>,
}

async fn check_permission(
    checker: &Arc<Mutex<PermissionChecker>>,
    resolver: &Option<PermissionResolver>,
    call: &ToolCall,
) -> PermissionOutcome {
    let tool_name = call.function.name.clone();
    let args: Option<Value> = serde_json::from_str(&call.function.arguments).ok();
    let result = checker.lock().check(&tool_name, args.as_ref());

    match result.decision {
        Decision::Allow => PermissionOutcome {
            events: Vec::new(),
            tool_message: None,
        },
        Decision::Deny => {
            let reason = result.reason.unwrap_or_else(|| "Permission denied".into());
            PermissionOutcome {
                events: vec![AgentEvent::PermissionDenied {
                    name: tool_name.clone(),
                    call: call.clone(),
                    reason: reason.clone(),
                }],
                tool_message: Some(ToolMessage {
                    tool_call_id: call.id.clone(),
                    content: format!("Error: Permission denied — {reason}"),
                }),
            }
        }
        Decision::Ask => {
            let reason = result
                .reason
                .unwrap_or_else(|| format!("{tool_name} requires approval"));
            let resolver = match resolver {
                Some(r) => r,
                None => {
                    return PermissionOutcome {
                        events: vec![AgentEvent::PermissionDenied {
                            name: tool_name.clone(),
                            call: call.clone(),
                            reason: reason.clone(),
                        }],
                        tool_message: Some(ToolMessage {
                            tool_call_id: call.id.clone(),
                            content: format!("Error: Permission denied — {reason}"),
                        }),
                    };
                }
            };
            let mut events = vec![AgentEvent::PermissionRequest {
                name: tool_name.clone(),
                call: call.clone(),
                reason: Some(reason.clone()),
            }];
            let response = resolver(tool_name.clone(), call.clone(), Some(reason.clone())).await;
            match response {
                PermissionResponse::Always => {
                    checker.lock().allow_always(&tool_name);
                    PermissionOutcome {
                        events,
                        tool_message: None,
                    }
                }
                PermissionResponse::Allow => PermissionOutcome {
                    events,
                    tool_message: None,
                },
                PermissionResponse::Deny => {
                    events.push(AgentEvent::PermissionDenied {
                        name: tool_name.clone(),
                        call: call.clone(),
                        reason: reason.clone(),
                    });
                    PermissionOutcome {
                        events,
                        tool_message: Some(ToolMessage {
                            tool_call_id: call.id.clone(),
                            content: format!("Error: Permission denied — {reason}"),
                        }),
                    }
                }
            }
        }
    }
}

fn aborted(signal: &Option<CancellationToken>) -> bool {
    signal.as_ref().is_some_and(|s| s.is_cancelled())
}

pub fn run_agent_loop<'a>(
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    messages: &'a mut Vec<Message>,
    options: AgentLoopOptions,
) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send + 'a>> {
    Box::pin(stream! {
        let max_iterations = options.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS);
        let doom_threshold = options.doom_loop_threshold.unwrap_or(DEFAULT_DOOM_LOOP_THRESHOLD);
        let mut tools = registry.definitions();
        if let Some(filter) = &options.tool_filter {
            tools = filter(&tools);
        }
        let mut recent_hashes: Vec<String> = Vec::new();
        let mut empty_response_retries = 0;
        let overrides = options.request_overrides.clone().unwrap_or(json!({}));

        #[allow(unused_assignments)]
        let mut last_assistant_content: Option<String> = None;

        for _iteration in 0..max_iterations {
            if aborted(&options.signal) { return; }

            let tools_arg: Option<&[ToolDefinition]> = if tools.is_empty() { None } else { Some(&tools) };
            let response = match provider.send(messages, tools_arg, &overrides).await {
                Ok(r) => r,
                Err(e) => {
                    yield AgentEvent::Error { message: e.to_string() };
                    return;
                }
            };
            if aborted(&options.signal) { return; }

            let response_usage = response.usage.clone();
            let choice = match response.choices.into_iter().next() {
                Some(c) => c,
                None => {
                    yield AgentEvent::Error { message: "No choice in response".to_string() };
                    return;
                }
            };
            let assistant_msg = AssistantMessage {
                content: choice.message.content.clone(),
                tool_calls: choice.message.tool_calls.clone().filter(|tcs| !tcs.is_empty()),
            };
            if assistant_msg
                .content
                .as_deref()
                .is_none_or(|c| c.trim().is_empty())
                && assistant_msg.tool_calls.is_none()
            {
                let finish_reason = choice.finish_reason.as_deref().unwrap_or("unknown");
                let usage = response_usage
                    .as_ref()
                    .map(|u| format!(
                        "prompt_tokens={}, completion_tokens={}, total_tokens={}",
                        u.prompt_tokens, u.completion_tokens, u.total_tokens
                    ))
                    .unwrap_or_else(|| "usage unavailable".to_string());
                if empty_response_retries < DEFAULT_EMPTY_RESPONSE_RETRIES {
                    empty_response_retries += 1;
                    yield AgentEvent::Error {
                        message: format!(
                            "Empty assistant response from provider (finish_reason={finish_reason}, {usage}); retrying once"
                        ),
                    };
                    continue;
                }
                yield AgentEvent::Error {
                    message: format!(
                        "Empty assistant response from provider (finish_reason={finish_reason}, {usage})"
                    ),
                };
                return;
            }
            empty_response_retries = 0;
            if let Some(usage) = response_usage.clone() {
                yield AgentEvent::Usage { usage };
            }
            yield AgentEvent::AssistantMessage {
                message: assistant_msg.clone(),
                finish_reason: choice.finish_reason.clone(),
            };
            messages.push(Message::Assistant(assistant_msg.clone()));
            last_assistant_content = assistant_msg.content.clone();

            let tool_calls = match choice.message.tool_calls {
                Some(tcs) if !tcs.is_empty() => tcs,
                _ => {
                    if options.plan_mode {
                        if let Some(c) = last_assistant_content {
                            yield AgentEvent::PlanComplete { plan: c };
                        }
                    }
                    return;
                }
            };

            let mut tool_messages: Vec<ToolMessage> = Vec::new();
            for call in &tool_calls {
                if aborted(&options.signal) { return; }

                if let Some(checker) = &options.permission_checker {
                    let outcome = check_permission(checker, &options.permission_resolver, call).await;
                    for ev in outcome.events {
                        yield ev;
                    }
                    if let Some(tm) = outcome.tool_message {
                        tool_messages.push(tm);
                        continue;
                    }
                }

                // Pre-tool hooks
                if let Some(runner) = &options.hooks_runner {
                    let ctx = HookContext {
                        tool_name: Some(call.function.name.clone()),
                        tool_args: Some(call.function.arguments.clone()),
                        ..Default::default()
                    };
                    let results = runner.run(HookEvent::PreTool, ctx).await;
                    if let Some(blocked) = results.iter().find(|r| r.blocked) {
                        tool_messages.push(ToolMessage {
                            tool_call_id: call.id.clone(),
                            content: format!(
                                "Error: Blocked by hook — {}",
                                blocked.reason.as_deref().unwrap_or("hook rejected")
                            ),
                        });
                        continue;
                    }
                }

                yield AgentEvent::ToolStart { name: call.function.name.clone(), call: call.clone() };
                let exec_opts = ExecOptions { signal: options.signal.clone() };
                let result = registry
                    .execute(&call.function.name, &call.function.arguments, exec_opts)
                    .await;
                yield AgentEvent::ToolEnd {
                    name: call.function.name.clone(),
                    result: result.clone(),
                    call: call.clone(),
                };

                let mut final_result = result.clone();
                if let Some(runner) = &options.hooks_runner {
                    let ctx = HookContext {
                        tool_name: Some(call.function.name.clone()),
                        tool_args: Some(call.function.arguments.clone()),
                        tool_result: Some(result.clone()),
                        ..Default::default()
                    };
                    let results = runner.run(HookEvent::PostTool, ctx).await;
                    let feedback: Vec<&str> = results
                        .iter()
                        .filter_map(|r| r.feedback.as_deref())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !feedback.is_empty() {
                        final_result = format!("{result}\n\n[hook feedback] {}", feedback.join("\n"));
                    }
                }
                tool_messages.push(ToolMessage {
                    tool_call_id: call.id.clone(),
                    content: final_result,
                });
            }
            for tm in tool_messages {
                messages.push(Message::Tool(tm));
            }

            let hash = hash_tool_calls(&tool_calls);
            recent_hashes.push(hash);
            if recent_hashes.len() as u32 > doom_threshold {
                recent_hashes.remove(0);
            }
            if is_doom_loop(&recent_hashes, doom_threshold) {
                yield AgentEvent::LoopDetected { count: doom_threshold };
                return;
            }
        }

        yield AgentEvent::Error {
            message: format!("Max iterations ({max_iterations}) reached — possible infinite loop"),
        };
    })
}

pub fn run_agent_loop_streaming<'a>(
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    messages: &'a mut Vec<Message>,
    options: AgentLoopOptions,
) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send + 'a>> {
    Box::pin(stream! {
        let max_iterations = options.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS);
        let doom_threshold = options.doom_loop_threshold.unwrap_or(DEFAULT_DOOM_LOOP_THRESHOLD);
        let mut tools = registry.definitions();
        if let Some(filter) = &options.tool_filter {
            tools = filter(&tools);
        }
        let mut recent_hashes: Vec<String> = Vec::new();
        let mut empty_response_retries = 0;
        let overrides = options.request_overrides.clone().unwrap_or(json!({}));

        #[allow(unused_assignments)]
        let mut last_assistant_content: Option<String> = None;

        for _iteration in 0..max_iterations {
            if aborted(&options.signal) { return; }

            let messages_clone = messages.clone();
            let tools_clone = if tools.is_empty() { None } else { Some(tools.clone()) };
            let mut chunk_stream = provider.stream(messages_clone, tools_clone, overrides.clone());

            let mut content_parts = String::new();
            let mut assembled: HashMap<u32, (String, String, String)> = HashMap::new(); // index → (id, name, args)
            let mut stream_usage = None;
            let mut finish_reasons: Vec<String> = Vec::new();

            loop {
                let chunk_res = match &options.signal {
                    Some(token) => {
                        tokio::select! {
                            _ = token.cancelled() => return,
                            next = chunk_stream.next() => next,
                        }
                    }
                    None => chunk_stream.next().await,
                };
                let chunk_res = match chunk_res {
                    Some(c) => c,
                    None => break,
                };
                let chunk = match chunk_res {
                    Ok(c) => c,
                    Err(e) => {
                        yield AgentEvent::Error { message: e.to_string() };
                        return;
                    }
                };
                let choice = match chunk.choices.into_iter().next() {
                    Some(c) => c,
                    None => continue,
                };
                if let Some(reason) = choice.finish_reason.clone() {
                    finish_reasons.push(reason);
                }
                if let Some(text) = choice.delta.content.clone() {
                    yield AgentEvent::ContentDelta { text: text.clone() };
                    content_parts.push_str(&text);
                }
                if let Some(tcs) = choice.delta.tool_calls {
                    for tc in tcs {
                        let entry = assembled.entry(tc.index).or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(id) = tc.id {
                            entry.0 = id;
                        }
                        if let Some(func) = tc.function {
                            if let Some(name) = func.name {
                                entry.1.push_str(&name);
                            }
                            if let Some(args) = func.arguments {
                                entry.2.push_str(&args);
                            }
                        }
                    }
                }
                if let Some(u) = chunk.usage {
                    debug("agent", "usage chunk received");
                    stream_usage = Some(u);
                }
            }
            if aborted(&options.signal) { return; }

            let mut indices: Vec<u32> = assembled.keys().copied().collect();
            indices.sort();
            let tool_calls: Vec<ToolCall> = indices
                .into_iter()
                .map(|i| {
                    let (id, name, args) = assembled.get(&i).cloned().unwrap_or_default();
                    ToolCall { id, kind: ToolCallKind::Function, function: FunctionCall { name, arguments: args } }
                })
                .collect();

            let assistant_msg = AssistantMessage {
                content: if content_parts.is_empty() { None } else { Some(content_parts) },
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls.clone()) },
            };
            if assistant_msg
                .content
                .as_deref()
                .is_none_or(|c| c.trim().is_empty())
                && assistant_msg.tool_calls.is_none()
            {
                let finish_reason = if finish_reasons.is_empty() {
                    "unknown".to_string()
                } else {
                    finish_reasons.join(",")
                };
                let usage = stream_usage
                    .as_ref()
                    .map(|u| format!(
                        "prompt_tokens={}, completion_tokens={}, total_tokens={}",
                        u.prompt_tokens, u.completion_tokens, u.total_tokens
                    ))
                    .unwrap_or_else(|| "usage unavailable".to_string());
                if empty_response_retries < DEFAULT_EMPTY_RESPONSE_RETRIES {
                    empty_response_retries += 1;
                    yield AgentEvent::Error {
                        message: format!(
                            "Empty assistant response from provider (finish_reason={finish_reason}, {usage}); retrying once"
                        ),
                    };
                    continue;
                }
                yield AgentEvent::Error {
                    message: format!(
                        "Empty assistant response from provider (finish_reason={finish_reason}, {usage})"
                    ),
                };
                return;
            }
            empty_response_retries = 0;
            yield AgentEvent::AssistantMessage {
                message: assistant_msg.clone(),
                finish_reason: if finish_reasons.is_empty() {
                    None
                } else {
                    Some(finish_reasons.join(","))
                },
            };
            if let Some(u) = stream_usage.take() {
                yield AgentEvent::Usage { usage: u };
            }
            messages.push(Message::Assistant(assistant_msg.clone()));
            last_assistant_content = assistant_msg.content.clone();

            if tool_calls.is_empty() {
                if options.plan_mode {
                    if let Some(c) = last_assistant_content {
                        yield AgentEvent::PlanComplete { plan: c };
                    }
                }
                return;
            }

            // Same per-tool-call permission/hooks/execute flow as non-streaming
            let mut tool_messages: Vec<ToolMessage> = Vec::new();
            for call in &tool_calls {
                if aborted(&options.signal) { return; }

                if let Some(checker) = &options.permission_checker {
                    let outcome = check_permission(checker, &options.permission_resolver, call).await;
                    for ev in outcome.events {
                        yield ev;
                    }
                    if let Some(tm) = outcome.tool_message {
                        tool_messages.push(tm);
                        continue;
                    }
                }
                if let Some(runner) = &options.hooks_runner {
                    let ctx = HookContext {
                        tool_name: Some(call.function.name.clone()),
                        tool_args: Some(call.function.arguments.clone()),
                        ..Default::default()
                    };
                    let results = runner.run(HookEvent::PreTool, ctx).await;
                    if let Some(blocked) = results.iter().find(|r| r.blocked) {
                        tool_messages.push(ToolMessage {
                            tool_call_id: call.id.clone(),
                            content: format!(
                                "Error: Blocked by hook — {}",
                                blocked.reason.as_deref().unwrap_or("hook rejected")
                            ),
                        });
                        continue;
                    }
                }
                yield AgentEvent::ToolStart { name: call.function.name.clone(), call: call.clone() };
                let exec_opts = ExecOptions { signal: options.signal.clone() };
                let result = registry
                    .execute(&call.function.name, &call.function.arguments, exec_opts)
                    .await;
                yield AgentEvent::ToolEnd {
                    name: call.function.name.clone(),
                    result: result.clone(),
                    call: call.clone(),
                };

                let mut final_result = result.clone();
                if let Some(runner) = &options.hooks_runner {
                    let ctx = HookContext {
                        tool_name: Some(call.function.name.clone()),
                        tool_args: Some(call.function.arguments.clone()),
                        tool_result: Some(result.clone()),
                        ..Default::default()
                    };
                    let results = runner.run(HookEvent::PostTool, ctx).await;
                    let feedback: Vec<&str> = results
                        .iter()
                        .filter_map(|r| r.feedback.as_deref())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !feedback.is_empty() {
                        final_result = format!("{result}\n\n[hook feedback] {}", feedback.join("\n"));
                    }
                }
                tool_messages.push(ToolMessage {
                    tool_call_id: call.id.clone(),
                    content: final_result,
                });
            }
            for tm in tool_messages {
                messages.push(Message::Tool(tm));
            }

            let hash = hash_tool_calls(&tool_calls);
            recent_hashes.push(hash);
            if recent_hashes.len() as u32 > doom_threshold {
                recent_hashes.remove(0);
            }
            if is_doom_loop(&recent_hashes, doom_threshold) {
                yield AgentEvent::LoopDetected { count: doom_threshold };
                return;
            }
        }

        yield AgentEvent::Error {
            message: format!("Max iterations ({max_iterations}) reached — possible infinite loop"),
        };
    })
}
