use heddle::tools::ask_user::{create_ask_user_tool, AskCallback};
use heddle::tools::types::ExecOptions;
use serde_json::json;
use std::sync::{Arc, Mutex};

fn fixed_response(reply: &'static str) -> AskCallback {
    Arc::new(move |_q, _opts| Box::pin(async move { reply.to_string() }))
}

#[tokio::test]
async fn returns_callback_result() {
    let tool = create_ask_user_tool(fixed_response("user said yes"));
    let result = tool
        .execute(json!({ "question": "Continue?" }), ExecOptions::default())
        .await;
    assert_eq!(result, "user said yes");
}

#[tokio::test]
async fn passes_options_to_callback() {
    let captured: Arc<Mutex<(String, Option<Vec<String>>)>> =
        Arc::new(Mutex::new((String::new(), None)));
    let captured_for_cb = captured.clone();
    let cb: AskCallback = Arc::new(move |q, opts| {
        let captured = captured_for_cb.clone();
        Box::pin(async move {
            *captured.lock().unwrap() = (q, opts);
            "option A".to_string()
        })
    });
    let tool = create_ask_user_tool(cb);
    tool.execute(
        json!({ "question": "Pick one", "options": ["A", "B", "C"] }),
        ExecOptions::default(),
    )
    .await;

    let (q, opts) = captured.lock().unwrap().clone();
    assert_eq!(q, "Pick one");
    assert_eq!(
        opts,
        Some(vec!["A".to_string(), "B".to_string(), "C".to_string()])
    );
}

#[tokio::test]
async fn handles_callback_error_string() {
    // Rust callbacks return String not Result; convention is "Error: ..." string.
    let tool = create_ask_user_tool(fixed_response("Error: readline broken"));
    let result = tool
        .execute(json!({ "question": "Hello?" }), ExecOptions::default())
        .await;
    assert!(result.contains("Error"), "got: {result}");
    assert!(result.contains("readline broken"), "got: {result}");
}

#[tokio::test]
async fn options_undefined_when_omitted() {
    let captured: Arc<Mutex<Option<Vec<String>>>> =
        Arc::new(Mutex::new(Some(vec!["should be overwritten".to_string()])));
    let captured_for_cb = captured.clone();
    let cb: AskCallback = Arc::new(move |_q, opts| {
        let captured = captured_for_cb.clone();
        Box::pin(async move {
            *captured.lock().unwrap() = opts;
            "ok".to_string()
        })
    });
    let tool = create_ask_user_tool(cb);
    tool.execute(json!({ "question": "What?" }), ExecOptions::default())
        .await;
    assert!(captured.lock().unwrap().is_none());
}
