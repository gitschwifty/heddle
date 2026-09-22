use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::architect::{run_architect_pipeline, ArchitectOptions};
use heddle::agent::loop_::{run_agent_loop, run_agent_loop_streaming, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::permissions::checker::read_only_tool_filter;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::Message;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

mod common;
use common::mocks::{
    finish_chunk, text_response, tool_call_chunk, tool_call_response, MockProvider,
};

struct CountingTool(&'static str, Arc<AtomicUsize>);

#[async_trait]
impl HeddleTool for CountingTool {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "Count executions"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    async fn execute(&self, _: Value, _: ExecOptions) -> String {
        self.1.fetch_add(1, Ordering::SeqCst);
        "executed".into()
    }
}

#[tokio::test]
async fn filtered_calls_never_execute_in_either_loop() {
    for streaming in [false, true] {
        for empty_filter in [false, true] {
            let writes = Arc::new(AtomicUsize::new(0));
            let reads = Arc::new(AtomicUsize::new(0));
            let mut registry = ToolRegistry::new();
            registry
                .register(Arc::new(CountingTool("write_file", writes.clone())))
                .unwrap();
            registry
                .register(Arc::new(CountingTool("read_file", reads.clone())))
                .unwrap();
            let provider = MockProvider::new();
            provider.push_response(tool_call_response(&[
                ("write_file", json!({})),
                ("read_file", json!({})),
            ]));
            provider.push_chunks(vec![
                tool_call_chunk(0, Some("call_0"), Some("write_file"), Some("{}")),
                tool_call_chunk(1, Some("call_1"), Some("read_file"), Some("{}")),
                finish_chunk("tool_calls"),
            ]);
            let options = AgentLoopOptions {
                max_iterations: Some(1),
                tool_filter: Some(Arc::new(move |tools| {
                    if empty_filter {
                        vec![]
                    } else {
                        read_only_tool_filter(tools)
                    }
                })),
                ..Default::default()
            };
            let mut messages = vec![];
            let events: Vec<_> = if streaming {
                run_agent_loop_streaming(provider, registry, &mut messages, options)
                    .collect()
                    .await
            } else {
                run_agent_loop(provider, registry, &mut messages, options)
                    .collect()
                    .await
            };
            assert_eq!(
                writes.load(Ordering::SeqCst),
                0,
                "streaming={streaming}, empty={empty_filter}"
            );
            assert_eq!(reads.load(Ordering::SeqCst), usize::from(!empty_filter));
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, AgentEvent::PermissionDenied { .. }))
                    .count(),
                if empty_filter { 2 } else { 1 }
            );
            let results: Vec<_> = messages
                .iter()
                .filter_map(|m| match m {
                    Message::Tool(t) => Some(t),
                    _ => None,
                })
                .collect();
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].tool_call_id, "call_0");
            assert!(results[0].content.contains("not available"));
        }
    }
}

#[tokio::test]
async fn architect_cannot_execute_writes_before_plan_approval() {
    let writes = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(Arc::new(CountingTool("write_file", writes.clone())))
        .unwrap();
    let architect = MockProvider::new();
    architect.push_response(tool_call_response(&[("write_file", json!({}))]));
    architect.push_response(text_response("Proposed plan"));
    let writes_at_approval = writes.clone();
    let mut messages = vec![];
    let events: Vec<_> = run_architect_pipeline(
        architect,
        MockProvider::new(),
        registry,
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions {
            on_plan_ready: Some(Arc::new(move |_| {
                assert_eq!(writes_at_approval.load(Ordering::SeqCst), 0);
                Box::pin(async { false })
            })),
        },
    )
    .collect()
    .await;
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::PermissionDenied { .. })));
    assert!(!events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolStart { .. })));
}
