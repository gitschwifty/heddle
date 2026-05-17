use heddle::tools::bash::create_bash_tool;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::ExecOptions;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn returns_early_if_signal_already_aborted() {
    let tool = create_bash_tool();
    let token = CancellationToken::new();
    token.cancel();
    let result = tool
        .execute(
            json!({ "command": "echo hello" }),
            ExecOptions {
                signal: Some(token),
            },
        )
        .await;
    assert_eq!(result, "Error: Aborted");
}

#[tokio::test]
async fn kills_running_process_on_abort() {
    let tool = create_bash_tool();
    let token = CancellationToken::new();
    let token_for_cancel = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        token_for_cancel.cancel();
    });
    let start = Instant::now();
    let result = tool
        .execute(
            json!({ "command": "sleep 30" }),
            ExecOptions {
                signal: Some(token),
            },
        )
        .await;
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5), "took {:?}", elapsed);
    assert_eq!(result, "Error: Aborted");
}

#[tokio::test]
async fn works_normally_without_signal() {
    let tool = create_bash_tool();
    let result = tool
        .execute(json!({ "command": "echo hello" }), ExecOptions::default())
        .await;
    assert_eq!(result, "hello\n");
}

#[tokio::test]
async fn works_normally_with_non_aborted_signal() {
    let tool = create_bash_tool();
    let token = CancellationToken::new();
    let result = tool
        .execute(
            json!({ "command": "echo hello" }),
            ExecOptions {
                signal: Some(token),
            },
        )
        .await;
    assert_eq!(result, "hello\n");
}

#[tokio::test]
async fn registry_forwards_signal_to_tool() {
    use async_trait::async_trait;
    use heddle::tools::types::HeddleTool;
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct SignalTool {
        saw_signal: Arc<AtomicBool>,
    }
    #[async_trait]
    impl HeddleTool for SignalTool {
        fn name(&self) -> &str {
            "test_signal"
        }
        fn description(&self) -> &str {
            "test"
        }
        fn parameters(&self) -> Value {
            json!({ "type": "object", "properties": {} })
        }
        async fn execute(&self, _params: Value, options: ExecOptions) -> String {
            if options.signal.is_some() {
                self.saw_signal.store(true, Ordering::SeqCst);
            }
            "ok".to_string()
        }
    }

    let saw = Arc::new(AtomicBool::new(false));
    let mut r = ToolRegistry::new();
    r.register(Arc::new(SignalTool {
        saw_signal: saw.clone(),
    }))
    .unwrap();

    let token = CancellationToken::new();
    let result = r
        .execute(
            "test_signal",
            "{}",
            ExecOptions {
                signal: Some(token),
            },
        )
        .await;
    assert_eq!(result, "ok");
    assert!(saw.load(Ordering::SeqCst), "tool did not see the signal");
}

#[tokio::test]
async fn registry_works_without_signal() {
    use async_trait::async_trait;
    use heddle::tools::types::HeddleTool;
    use serde_json::Value;

    struct NoSignalTool;
    #[async_trait]
    impl HeddleTool for NoSignalTool {
        fn name(&self) -> &str {
            "test_no_signal"
        }
        fn description(&self) -> &str {
            "test"
        }
        fn parameters(&self) -> Value {
            json!({ "type": "object", "properties": {} })
        }
        async fn execute(&self, _params: Value, _options: ExecOptions) -> String {
            "ok".to_string()
        }
    }

    let mut r = ToolRegistry::new();
    r.register(std::sync::Arc::new(NoSignalTool)).unwrap();
    let result = r
        .execute("test_no_signal", "{}", ExecOptions::default())
        .await;
    assert_eq!(result, "ok");
}
