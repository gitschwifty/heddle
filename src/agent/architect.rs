//! Two-phase architect/editor pipeline.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use futures::Stream;
use futures::StreamExt;

use super::loop_::{run_agent_loop, AgentLoopOptions};
use super::types::AgentEvent;
use crate::permissions::checker::read_only_tool_filter;
use crate::provider::types::Provider;
use crate::tools::registry::ToolRegistry;
use crate::types::{Message, UserMessage};

pub type OnPlanReady =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

#[derive(Default, Clone)]
pub struct ArchitectOptions {
    pub on_plan_ready: Option<OnPlanReady>,
}

pub fn run_architect_pipeline<'a>(
    architect_provider: Arc<dyn Provider>,
    editor_provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    messages: &'a mut Vec<Message>,
    loop_options: AgentLoopOptions,
    architect_options: ArchitectOptions,
) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send + 'a>> {
    Box::pin(stream! {
        let mut architect_messages: Vec<Message> = messages.clone();
        let mut plan: Option<String> = None;

        let mut a_opts = loop_options.clone();
        a_opts.plan_mode = true;
        a_opts.tool_filter = Some(Arc::new(|tools: &[crate::types::ToolDefinition]| {
            read_only_tool_filter(tools)
        }));

        {
            let mut stream = run_agent_loop(architect_provider, registry.clone(), &mut architect_messages, a_opts);
            while let Some(event) = stream.next().await {
                if let AgentEvent::PlanComplete { plan: p } = &event {
                    plan = Some(p.clone());
                }
                yield event;
            }
        }

        let plan = match plan {
            Some(p) => p,
            None => {
                yield AgentEvent::Error { message: "Architect phase produced no plan".to_string() };
                return;
            }
        };

        if let Some(cb) = &architect_options.on_plan_ready {
            if !cb(plan.clone()).await {
                yield AgentEvent::Error { message: "Plan was rejected".to_string() };
                return;
            }
        }

        let mut editor_messages: Vec<Message> = messages.clone();
        editor_messages.push(Message::User(UserMessage {
            content: format!("Execute the following plan:\n\n{plan}"),
        }));

        let mut e_opts = loop_options.clone();
        e_opts.plan_mode = false;
        e_opts.tool_filter = None;

        {
            let mut stream = run_agent_loop(editor_provider, registry, &mut editor_messages, e_opts);
            while let Some(event) = stream.next().await {
                yield event;
            }
        }
        *messages = editor_messages;
    })
}
