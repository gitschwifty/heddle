//! Tests for the runtime facade without spawning the headless binary.

use tokio_util::sync::CancellationToken;

use heddle::config::features::Mode;
use heddle::runtime::{HeddleRuntime, RuntimeEvent, TurnOptions, TurnStatus};
use heddle::session::setup::{create_session, SessionOptions};

mod common;
use common::mocks::{finish_chunk, text_chunk, usage_chunk, MockProvider};
use common::Sandbox;

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
    assert_eq!(runtime.status(false).messages_count, 3);
}
