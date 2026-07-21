//! Live-model multi-turn integration tests.
//!
//! Gated on:
//! - `HEDDLE_INTEGRATION_TESTS=1`
//! - `HEDDLE_SLOW_TESTS=1`
//! - `OPENROUTER_API_KEY` set
//!
//! Set these in `.env.test` (auto-loaded via `common::env::init()`).
//! Without them, tests skip with a `skip:` message and pass.

mod common;

use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::{Provider, ProviderConfig};
use heddle::session::jsonl::{append_message, load_session, write_session_meta, SessionMeta};
use heddle::tools::edit::create_edit_tool;
use heddle::tools::read::create_read_tool;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::write::create_write_tool;
use heddle::types::{Message, SystemMessage, UserMessage};
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

// Free model fallback chain. OpenRouter caps the `models` fallback array at 3.
const FREE_MODELS: &[&str] = &[
    "liquid/lfm-2.5-1.2b-instruct:free",
    "arcee-ai/trinity-large-preview:free",
    "arcee-ai/trinity-mini:free",
    "openrouter/free",
];

fn enabled() -> Option<String> {
    common::env::init();
    if std::env::var("HEDDLE_INTEGRATION_TESTS").ok().as_deref() != Some("1") {
        return None;
    }
    if std::env::var("HEDDLE_SLOW_TESTS").ok().as_deref() != Some("1") {
        return None;
    }
    std::env::var("OPENROUTER_API_KEY").ok()
}

fn make_provider(api_key: String) -> Arc<dyn Provider> {
    let fallback: Vec<&str> = FREE_MODELS.iter().skip(1).copied().collect();
    create_openrouter_provider(ProviderConfig {
        api_key,
        model: FREE_MODELS[0].to_string(),
        base_url: None,
        request_params: Some(json!({ "models": fallback, "route": "fallback" })),
        app_attribution: None,
        retry: None,
    })
}

fn make_registry() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(create_read_tool()).unwrap();
    r.register(create_edit_tool()).unwrap();
    r.register(create_write_tool()).unwrap();
    r
}

async fn collect(
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    messages: &mut Vec<Message>,
) -> Vec<AgentEvent> {
    let stream = run_agent_loop(provider, registry, messages, AgentLoopOptions::default());
    futures::pin_mut!(stream);
    let mut out = Vec::new();
    while let Some(e) = stream.next().await {
        out.push(e);
    }
    out
}

const SYS_PROMPT: &str =
    "You are a helpful assistant. Use tools when asked to interact with files.";

#[tokio::test(flavor = "multi_thread")]
async fn two_turn_tool_chain_read_then_edit() {
    let Some(api_key) = enabled() else {
        eprintln!(
            "skip: HEDDLE_INTEGRATION_TESTS or HEDDLE_SLOW_TESTS != 1, or OPENROUTER_API_KEY unset"
        );
        return;
    };

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("data.txt");
    std::fs::write(&file_path, "count: 0").unwrap();

    let mut messages = vec![Message::System(SystemMessage {
        content: SYS_PROMPT.to_string(),
    })];

    // Turn 1: read the file
    messages.push(Message::User(UserMessage {
        content: format!(
            "Read the file at {} and tell me what's in it.",
            file_path.display()
        ),
    }));
    let turn1 = collect(
        make_provider(api_key.clone()),
        make_registry(),
        &mut messages,
    )
    .await;

    assert!(messages.len() > 2, "messages should grow past system+user");

    let read_starts = turn1
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolStart { name, .. } if name == "read_file"))
        .count();
    assert!(
        read_starts >= 1,
        "expected at least one read_file tool_start, got {read_starts}"
    );

    let saw_file_content = messages.iter().any(|m| match m {
        Message::Tool(t) => t.content.contains("count: 0"),
        _ => false,
    });
    assert!(
        saw_file_content,
        "expected a tool result containing 'count: 0'"
    );

    let after_turn1 = messages.len();

    // Turn 2: edit the file
    messages.push(Message::User(UserMessage {
        content: format!(
            "Edit the file at {} to change \"count: 0\" to \"count: 1\".",
            file_path.display()
        ),
    }));
    let _ = collect(make_provider(api_key), make_registry(), &mut messages).await;

    assert!(
        messages.len() > after_turn1,
        "messages should grow after turn 2"
    );

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(
        content.contains("count: 1"),
        "file should now contain 'count: 1', got: {content}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_turn_chat_only_does_not_call_tools() {
    let Some(api_key) = enabled() else {
        eprintln!(
            "skip: HEDDLE_INTEGRATION_TESTS or HEDDLE_SLOW_TESTS != 1, or OPENROUTER_API_KEY unset"
        );
        return;
    };

    let mut messages = vec![Message::System(SystemMessage {
        content: SYS_PROMPT.to_string(),
    })];

    messages.push(Message::User(UserMessage {
        content: "Hello! Reply with one short sentence and do not inspect files.".to_string(),
    }));
    let turn1 = collect(
        make_provider(api_key.clone()),
        make_registry(),
        &mut messages,
    )
    .await;

    let turn1_tools = turn1
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolStart { .. }))
        .count();
    assert_eq!(turn1_tools, 0, "chat-only turn 1 should not call tools");
    assert!(
        messages
            .last()
            .is_some_and(|m| matches!(m, Message::Assistant(a) if a.content.as_deref().is_some_and(|c| !c.trim().is_empty()))),
        "turn 1 should end with a non-empty assistant message"
    );

    let after_turn1 = messages.len();
    messages.push(Message::User(UserMessage {
        content: "Thanks. In one short sentence, say what you can help with.".to_string(),
    }));
    let turn2 = collect(make_provider(api_key), make_registry(), &mut messages).await;

    let turn2_tools = turn2
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolStart { .. }))
        .count();
    assert_eq!(turn2_tools, 0, "chat-only turn 2 should not call tools");
    assert!(
        messages.len() > after_turn1,
        "messages should grow after turn 2"
    );
    assert!(
        messages
            .last()
            .is_some_and(|m| matches!(m, Message::Assistant(a) if a.content.as_deref().is_some_and(|c| !c.trim().is_empty()))),
        "turn 2 should end with a non-empty assistant message"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_persistence_round_trip() {
    let Some(api_key) = enabled() else {
        eprintln!(
            "skip: HEDDLE_INTEGRATION_TESTS or HEDDLE_SLOW_TESTS != 1, or OPENROUTER_API_KEY unset"
        );
        return;
    };

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("persist.txt");
    std::fs::write(&file_path, "original content").unwrap();

    let mut messages = vec![Message::System(SystemMessage {
        content: SYS_PROMPT.to_string(),
    })];

    // Turn 1: read the file
    messages.push(Message::User(UserMessage {
        content: format!("Read the file at {}.", file_path.display()),
    }));
    let _ = collect(make_provider(api_key), make_registry(), &mut messages).await;

    let message_count = messages.len();
    assert!(
        message_count > 2,
        "expected messages to grow past system+user"
    );

    // Write messages to JSONL
    let session_path = dir.path().join("session.jsonl");
    write_session_meta(
        &session_path,
        &SessionMeta {
            kind: "session_meta".to_string(),
            id: "integ-test-001".to_string(),
            cwd: dir.path().to_string_lossy().into_owned(),
            model: FREE_MODELS[0].to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            heddle_version: "0.0.1-test".to_string(),
            name: None,
            forked_from: None,
            extra: Default::default(),
        },
    )
    .unwrap();

    for msg in &messages {
        append_message(&session_path, msg).unwrap();
    }

    // Load back from JSONL
    let loaded = load_session(&session_path);
    assert_eq!(
        loaded.len(),
        message_count,
        "loaded message count should match"
    );

    let roles: Vec<&str> = loaded
        .iter()
        .map(|m| match m {
            Message::System(_) => "system",
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
            Message::Tool(_) => "tool",
        })
        .collect();

    assert_eq!(roles[0], "system");
    assert_eq!(roles[1], "user");

    for role in &roles[2..] {
        assert!(
            matches!(*role, "assistant" | "tool" | "user"),
            "unexpected role after turn: {role}"
        );
    }

    // No two consecutive user messages (would indicate a bug)
    for i in 1..roles.len() {
        if roles[i] == "user" {
            assert_ne!(
                roles[i - 1],
                "user",
                "two consecutive user messages at index {i}"
            );
        }
    }
}
