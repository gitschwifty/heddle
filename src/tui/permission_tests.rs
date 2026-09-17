use super::*;
use ratatui::{backend::TestBackend, Terminal};

fn prompt(app: &mut TuiApp) -> oneshot::Receiver<RuntimePermissionResponse> {
    let (respond_to, receiver) = oneshot::channel();
    app.set_permission_prompt(PermissionPrompt {
        request: RuntimePermissionRequest {
            name: "write_file".into(),
            call: serde_json::from_value(serde_json::json!({
                "id": "call_guidance", "type": "function",
                "function": {"name": "write_file", "arguments": "{}"}
            }))
            .unwrap(),
            reason: Some("requires approval".into()),
        },
        respond_to,
    });
    receiver
}

async fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    let (tx, _rx) = mpsc::channel(1);
    app.handle_key(KeyEvent::new(code, modifiers), &tx, &mut 0)
        .await
        .unwrap();
}

#[tokio::test]
async fn guidance_accepts_choice_letters_and_multiline_text() {
    let mut app = TuiApp::new();
    app.input.insert_char('x');
    let mut receiver = prompt(&mut app);
    key(&mut app, KeyCode::Char('s'), KeyModifiers::NONE).await;
    for c in "nay\\".chars() {
        key(&mut app, KeyCode::Char(c), KeyModifiers::NONE).await;
    }
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE).await;
    key(&mut app, KeyCode::Char('é'), KeyModifiers::NONE).await;
    key(&mut app, KeyCode::Enter, KeyModifiers::SHIFT).await;
    key(&mut app, KeyCode::Char('g'), KeyModifiers::NONE).await;

    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| render::draw(frame, &mut app))
        .unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("Deny with guidance"));
    assert!(screen.contains("Tool will not run; model will receive your note and continue."));
    assert!(screen.contains("nay"));

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE).await;
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimePermissionResponse::DenyWithGuidance("nay\né\ng".into())
    );
    assert!(app.permission_prompt.is_none());
    assert!(app.permission_guidance.is_none());
    assert_eq!(app.input.text(), "x");
}

#[tokio::test]
async fn escape_returns_to_choices_and_blank_guidance_denies() {
    let mut app = TuiApp::new();
    let mut receiver = prompt(&mut app);
    key(&mut app, KeyCode::Char('g'), KeyModifiers::NONE).await;
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE).await;
    assert!(receiver.try_recv().is_err());
    assert!(app.permission_prompt.is_some());
    assert!(app.permission_guidance.is_none());
    key(&mut app, KeyCode::Char('g'), KeyModifiers::NONE).await;
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE).await;
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE).await;
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimePermissionResponse::Deny
    );
}
