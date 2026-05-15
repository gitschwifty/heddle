//! Mock helpers for non-streaming and streaming OpenRouter-style responses.
//!
//! Mirrors `ts-test/mocks/openrouter.ts`. The mock provider is enough to drive
//! the agent loop without hitting wiremock.

use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::Stream;
use serde_json::Value;

use heddle::provider::types::{ChunkStream, Provider};
use heddle::types::{
    ChatCompletionResponse, Choice, ChoiceMessage, Delta, FunctionCall, FunctionCallDelta, Message,
    StreamChoice, StreamChunk, ToolCall, ToolCallDelta, ToolCallKind, ToolDefinition, Usage,
};

pub fn text_response(content: &str) -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: "chatcmpl-test".to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                content: Some(content.to_string()),
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        }),
    }
}

pub fn tool_call_response(calls: &[(&str, Value)]) -> ChatCompletionResponse {
    let tool_calls: Vec<ToolCall> = calls
        .iter()
        .enumerate()
        .map(|(i, (name, args))| ToolCall {
            id: format!("call_{i}"),
            kind: ToolCallKind::Function,
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        })
        .collect();
    ChatCompletionResponse {
        id: "chatcmpl-test".to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                content: None,
                tool_calls: Some(tool_calls),
            },
            finish_reason: Some("tool_calls".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        }),
    }
}

pub fn text_chunk(content: &str) -> StreamChunk {
    StreamChunk {
        id: "chatcmpl-test".to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta {
                role: None,
                content: Some(content.to_string()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    }
}

pub fn finish_chunk(reason: &str) -> StreamChunk {
    StreamChunk {
        id: "chatcmpl-test".to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta::default(),
            finish_reason: Some(reason.to_string()),
        }],
        usage: None,
    }
}

pub fn tool_call_chunk(
    index: u32,
    id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
) -> StreamChunk {
    StreamChunk {
        id: "chatcmpl-test".to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta {
                role: None,
                content: None,
                tool_calls: Some(vec![ToolCallDelta {
                    index,
                    id: id.map(String::from),
                    kind: id.map(|_| ToolCallKind::Function),
                    function: if name.is_some() || arguments.is_some() {
                        Some(FunctionCallDelta {
                            name: name.map(String::from),
                            arguments: arguments.map(String::from),
                        })
                    } else {
                        None
                    },
                }]),
            },
            finish_reason: None,
        }],
        usage: None,
    }
}

pub fn usage_chunk(prompt: u64, completion: u64, total: u64, cost: Option<f64>) -> StreamChunk {
    StreamChunk {
        id: "chatcmpl-test".to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta::default(),
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: total,
            cost,
            ..Default::default()
        }),
    }
}

// ── MockProvider ───────────────────────────────────────────────────────

/// A scripted provider — pops responses/chunks for each call. Useful for
/// agent-loop tests without spinning up wiremock.
pub struct MockProvider {
    responses: Mutex<Vec<ChatCompletionResponse>>,
    chunks: Mutex<Vec<Vec<StreamChunk>>>,
}

impl MockProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(Vec::new()),
            chunks: Mutex::new(Vec::new()),
        })
    }
    pub fn push_response(self: &Arc<Self>, r: ChatCompletionResponse) -> Arc<Self> {
        self.responses.lock().unwrap().push(r);
        self.clone()
    }
    pub fn push_chunks(self: &Arc<Self>, c: Vec<StreamChunk>) -> Arc<Self> {
        self.chunks.lock().unwrap().push(c);
        self.clone()
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> Result<ChatCompletionResponse> {
        let mut v = self.responses.lock().unwrap();
        if v.is_empty() {
            return Err(anyhow!("MockProvider: no more responses scripted"));
        }
        Ok(v.remove(0))
    }

    fn stream(
        &self,
        _messages: Vec<Message>,
        _tools: Option<Vec<ToolDefinition>>,
        _overrides: Value,
    ) -> ChunkStream {
        let chunks = self.chunks.lock().unwrap().pop().unwrap_or_default();
        let s: std::pin::Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>> =
            Box::pin(try_stream! {
                for chunk in chunks {
                    yield chunk;
                }
            });
        s
    }

    fn with(&self, _overrides: Value) -> Arc<dyn Provider> {
        Arc::new(MockProvider {
            responses: Mutex::new(self.responses.lock().unwrap().clone()),
            chunks: Mutex::new(self.chunks.lock().unwrap().clone()),
        })
    }
}
