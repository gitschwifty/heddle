//! Tests for the runtime facade without spawning the headless binary.

use tokio_util::sync::CancellationToken;

use heddle::config::features::Mode;
use heddle::config::loader::ApprovalMode;
use heddle::permissions::checker::PermissionChecker;
use heddle::runtime::{
    HeddleRuntime, RuntimeEvent, RuntimePermissionResponse, TurnOptions, TurnStatus,
};
use heddle::session::jsonl::{load_session, CONTEXT_RESET_MARKER_TYPE};
use heddle::session::setup::{create_session, SessionOptions};
use heddle::types::{AssistantMessage, Message, SystemMessage, ToolMessage, UserMessage};
use heddle::types::{Delta, StreamChoice, StreamChunk};
use parking_lot::Mutex;
use std::sync::Arc;

mod common;
use common::mocks::{finish_chunk, text_chunk, tool_call_chunk, usage_chunk, MockProvider};
use common::Sandbox;

fn routed_model_chunk(model: &str) -> StreamChunk {
    StreamChunk {
        model: Some(model.to_string()),
        id: "chatcmpl-test".to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta::default(),
            finish_reason: None,
        }],
        usage: None,
    }
}

#[tokio::test]
async fn runtime_send_emits_events_and_returns_outcome() {
    let _sb = Sandbox::new("runtime-send");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");

    let mut session = create_session(SessionOptions {
        mode: Some(Mode::Headless),
        ..SessionOptions::default()
    })
    .await
    .expect("create_session");
    let provider = MockProvider::new().push_chunks(vec![
        text_chunk("Hello"),
        text_chunk(", runtime"),
        usage_chunk(11, 7, 18, None),
        finish_chunk("stop"),
    ]);
    session.provider = provider;

    let mut runtime = HeddleRuntime::from_session(session);
    let mut events = Vec::new();
    let outcome = runtime
        .send(
            "hi".to_string(),
            TurnOptions {
                id: "turn-1".to_string(),
                cancel: CancellationToken::new(),
                permission_resolver: None,
            },
            |event| events.push(event),
        )
        .await;

    assert_eq!(outcome.status, TurnStatus::Ok);
    assert_eq!(outcome.response.as_deref(), Some("Hello, runtime"));
    assert_eq!(outcome.iterations, 1);
    assert_eq!(outcome.usage.as_ref().unwrap().total_tokens, 18);
    assert!(events
        .iter()
        .any(|e| matches!(e, RuntimeEvent::ContentDelta { text } if text == "Hello")));
    assert!(events
        .iter()
        .any(|e| matches!(e, RuntimeEvent::UsageUpdated { usage } if usage.total_tokens == 18)));
    let status = runtime.status(false);
    assert_eq!(status.messages_count, 2);
    assert_eq!(status.total_input_tokens, 11);
    assert_eq!(status.total_output_tokens, 7);
}

#[tokio::test]
async fn runtime_status_tracks_last_routed_model_from_stream() {
    let _sb = Sandbox::new("runtime-routed-model");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");

    let mut session = create_session(SessionOptions {
        mode: Some(Mode::Headless),
        model: Some("openrouter/free".to_string()),
        ..SessionOptions::default()
    })
    .await
    .expect("create_session");
    session.provider = MockProvider::new().push_chunks(vec![
        routed_model_chunk("openai/gpt-oss-120b"),
        text_chunk("Hello"),
        finish_chunk("stop"),
    ]);

    let mut runtime = HeddleRuntime::from_session(session);
    let mut events = Vec::new();
    let outcome = runtime
        .send(
            "hi".to_string(),
            TurnOptions {
                id: "turn-1".to_string(),
                cancel: CancellationToken::new(),
                permission_resolver: None,
            },
            |event| events.push(event),
        )
        .await;

    assert_eq!(outcome.status, TurnStatus::Ok);
    assert!(events.iter().any(
        |event| matches!(event, RuntimeEvent::RoutedModel { model } if model == "openai/gpt-oss-120b")
    ));
    let status = runtime.status(false);
    assert_eq!(status.model, "openrouter/free");
    assert_eq!(
        status.last_routed_model.as_deref(),
        Some("openai/gpt-oss-120b")
    );
}

#[tokio::test]
async fn runtime_status_counts_user_and_assistant_messages_only() {
    let _sb = Sandbox::new("runtime-status-message-count");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");

    let mut session = create_session(SessionOptions {
        mode: Some(Mode::Headless),
        ..SessionOptions::default()
    })
    .await
    .expect("create_session");
    session.messages.push(Message::System(SystemMessage {
        content: "system".to_string(),
    }));
    session.messages.push(Message::User(UserMessage {
        content: "prompt".to_string(),
    }));
    session.messages.push(Message::Assistant(AssistantMessage {
        content: Some("answer".to_string()),
        tool_calls: None,
    }));
    session.messages.push(Message::Tool(ToolMessage {
        tool_call_id: "call_1".to_string(),
        content: "tool output".to_string(),
    }));

    let runtime = HeddleRuntime::from_session(session);
    let status = runtime.status(false);

    assert_eq!(status.messages_count, 2);
}

#[tokio::test]
async fn runtime_clear_context_keeps_session_id_and_rebuilds_system_prompt() {
    let _sb = Sandbox::new("runtime-clear-context");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");

    let session = create_session(SessionOptions {
        mode: Some(Mode::Headless),
        system_prompt: Some("custom base prompt".to_string()),
        ..SessionOptions::default()
    })
    .await
    .expect("create_session");
    let session_id = session.session_id.clone();
    let session_file = session.session_file.clone();
    let original_system = session.messages[0].content_str().unwrap().to_string();

    let mut runtime = HeddleRuntime::from_session(session);
    runtime
        .session_mut()
        .messages
        .push(Message::User(UserMessage {
            content: "old prompt".to_string(),
        }));
    runtime
        .session_mut()
        .messages
        .push(Message::Assistant(AssistantMessage {
            content: Some("old answer".to_string()),
            tool_calls: None,
        }));

    runtime.clear_context().expect("clear context");

    assert_eq!(runtime.session().session_id, session_id);
    assert_eq!(runtime.status(false).messages_count, 0);
    assert_eq!(runtime.session().messages.len(), 1);
    let reset_system = runtime.session().messages[0].content_str().unwrap();
    assert!(reset_system.contains("custom base prompt"));
    assert_eq!(reset_system, original_system);

    let raw = std::fs::read_to_string(&session_file).expect("session jsonl");
    assert!(raw.contains(&format!(r#""type":"{CONTEXT_RESET_MARKER_TYPE}""#)));
    let loaded = load_session(&session_file);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].content_str(), Some(reset_system));
}

#[tokio::test]
async fn runtime_permission_resolver_denies_and_turn_continues() {
    let _sb = Sandbox::new("runtime-permission-deny");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");

    let mut session = create_session(SessionOptions {
        mode: Some(Mode::Headless),
        ..SessionOptions::default()
    })
    .await
    .expect("create_session");
    session.permission_checker = Some(Arc::new(Mutex::new(PermissionChecker::new(
        ApprovalMode::Suggest,
        None,
        None,
    ))));
    let provider = MockProvider::new()
        .push_chunks(vec![
            text_chunk("I will not write it."),
            finish_chunk("stop"),
        ])
        .push_chunks(vec![
            tool_call_chunk(
                0,
                Some("call_0"),
                Some("write_file"),
                Some(r#"{"file_path":"foo.txt","content":"bar"}"#),
            ),
            finish_chunk("tool_calls"),
        ]);
    session.provider = provider;

    let mut runtime = HeddleRuntime::from_session(session);
    let mut events = Vec::new();
    let outcome = runtime
        .send(
            "write foo".to_string(),
            TurnOptions {
                id: "turn-permission".to_string(),
                cancel: CancellationToken::new(),
                permission_resolver: Some(Arc::new(|request| {
                    Box::pin(async move {
                        assert_eq!(request.name, "write_file");
                        assert_eq!(request.call.id, "call_0");
                        assert!(request.reason.is_some());
                        RuntimePermissionResponse::Deny
                    })
                })),
            },
            |event| events.push(event),
        )
        .await;

    assert_eq!(outcome.status, TurnStatus::Ok);
    assert_eq!(outcome.response.as_deref(), Some("I will not write it."));
    assert!(events.iter().any(
        |e| matches!(e, RuntimeEvent::PermissionRequested { name, .. } if name == "write_file")
    ));
    assert!(events
        .iter()
        .any(|e| matches!(e, RuntimeEvent::PermissionDenied { name, .. } if name == "write_file")));
}
