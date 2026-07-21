use heddle::ipc::codec::{
    build_error, build_result, decode_request, encode_response, wrap_event, BuildResultArgs,
    CorrelationContext, DecodeResult,
};
use heddle::ipc::errors::ErrorEnvelope;
use heddle::ipc::types::{
    InitConfig, IpcRequest, IpcResponse, ToolCallSummary, UsageSummary, WorkerEvent,
};
use serde_json::{json, Value};

mod common;

#[test]
fn encode_response_no_trailing_newline() {
    let res = IpcResponse::ShutdownOk { id: "1".into() };
    let encoded = encode_response(&res);
    assert_eq!(encoded, r#"{"type":"shutdown_ok","id":"1"}"#);
    assert!(!encoded.ends_with('\n'));
}

#[test]
fn decode_valid_init_request() {
    let line = serde_json::to_string(&IpcRequest::Init {
        id: "1".into(),
        protocol_version: None,
        config: Box::new(InitConfig {
            model: "m".into(),
            system_prompt: "s".into(),
            tools: vec![],
            max_iterations: None,
            task_id: None,
            worker_id: None,
            app_attribution: None,
            permissions: None,
            hooks: None,
        }),
    })
    .unwrap();
    match decode_request(&line) {
        DecodeResult::Ok(IpcRequest::Init { .. }) => {}
        _ => panic!("expected init"),
    }
}

#[test]
fn decode_invalid_json() {
    let r = decode_request("not json{");
    match r {
        DecodeResult::Err(e) => assert_eq!(e, "Invalid JSON"),
        _ => panic!("expected err"),
    }
}

#[test]
fn decode_missing_type() {
    let r = decode_request(r#"{"id":"1"}"#);
    match r {
        DecodeResult::Err(e) => assert_eq!(e, "Missing 'type' field"),
        _ => panic!("expected err"),
    }
}

#[test]
fn decode_missing_id() {
    let r = decode_request(r#"{"type":"send"}"#);
    match r {
        DecodeResult::Err(e) => assert_eq!(e, "Missing 'id' field"),
        _ => panic!("expected err"),
    }
}

#[test]
fn decode_non_object() {
    let r = decode_request(r#""hello""#);
    match r {
        DecodeResult::Err(e) => assert_eq!(e, "Expected JSON object"),
        _ => panic!("expected err"),
    }
}

#[test]
fn wrap_event_with_seq_and_send_id() {
    let ev = WorkerEvent::ContentDelta { text: "hi".into() };
    let wrapped = wrap_event(ev, "send-1", 0, None);
    let v: Value = serde_json::to_value(&wrapped).unwrap();
    assert_eq!(v["type"], "event");
    assert_eq!(v["event"]["event"], "content_delta");
    assert_eq!(v["event"]["text"], "hi");
    assert_eq!(v["send_id"], "send-1");
    assert_eq!(v["event_seq"], 0);
}

#[test]
fn wrap_event_higher_seq() {
    let ev = WorkerEvent::ContentDelta { text: "x".into() };
    let wrapped = wrap_event(ev, "s2", 5, None);
    let v: Value = serde_json::to_value(&wrapped).unwrap();
    assert_eq!(v["event_seq"], 5);
    assert_eq!(v["send_id"], "s2");
}

#[test]
fn wrap_event_with_correlation() {
    let ev = WorkerEvent::ContentDelta { text: "hi".into() };
    let ctx = CorrelationContext {
        session_id: Some("sess-1".into()),
        task_id: Some("task-1".into()),
        worker_id: Some("worker-0".into()),
    };
    let wrapped = wrap_event(ev, "send-1", 0, Some(&ctx));
    let v: Value = serde_json::to_value(&wrapped).unwrap();
    assert_eq!(v["session_id"], "sess-1");
    assert_eq!(v["task_id"], "task-1");
    assert_eq!(v["worker_id"], "worker-0");
}

#[test]
fn wrap_event_omits_undefined_correlation_fields() {
    let ev = WorkerEvent::ContentDelta { text: "hi".into() };
    let ctx = CorrelationContext {
        session_id: Some("sess-1".into()),
        ..Default::default()
    };
    let wrapped = wrap_event(ev, "send-1", 0, Some(&ctx));
    let v: Value = serde_json::to_value(&wrapped).unwrap();
    assert_eq!(v["session_id"], "sess-1");
    assert!(v.get("task_id").is_none() || v["task_id"].is_null());
    assert!(v.get("worker_id").is_none() || v["worker_id"].is_null());
}

#[test]
fn wrap_event_no_correlation() {
    let ev = WorkerEvent::ContentDelta { text: "hi".into() };
    let wrapped = wrap_event(ev, "send-1", 0, None);
    let s = encode_response(&wrapped);
    assert!(!s.contains("session_id"));
    assert!(!s.contains("task_id"));
    assert!(!s.contains("worker_id"));
}

#[test]
fn routed_model_event_serializes_model() {
    let ev = WorkerEvent::RoutedModel {
        model: "openai/gpt-oss-120b".into(),
    };
    let wrapped = wrap_event(ev, "send-1", 0, None);
    let v: Value = serde_json::to_value(&wrapped).unwrap();

    assert_eq!(v["event"]["event"], "routed_model");
    assert_eq!(v["event"]["model"], "openai/gpt-oss-120b");
}

#[test]
fn status_ok_can_include_last_routed_model() {
    let res = IpcResponse::StatusOk {
        id: "status-1".into(),
        model: "openrouter/free".into(),
        last_routed_model: Some("openai/gpt-oss-120b".into()),
        messages_count: 2,
        session_id: "sess-1".into(),
        active: false,
    };
    let v: Value = serde_json::to_value(&res).unwrap();

    assert_eq!(v["type"], "status_ok");
    assert_eq!(v["model"], "openrouter/free");
    assert_eq!(v["last_routed_model"], "openai/gpt-oss-120b");
}

#[test]
fn build_result_ok() {
    let res = build_result(
        "2",
        BuildResultArgs {
            status: "ok".into(),
            response: Some("Hello!".into()),
            tool_calls_made: vec![ToolCallSummary {
                name: "glob".into(),
                args: json!({"pattern":"*"}),
            }],
            usage: Some(UsageSummary {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            iterations: 1,
            ..Default::default()
        },
    );
    let v: Value = serde_json::to_value(&res).unwrap();
    assert_eq!(v["type"], "result");
    assert_eq!(v["id"], "2");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["response"], "Hello!");
    assert_eq!(v["iterations"], 1);
    assert_eq!(v["usage"]["total_tokens"], 15);
    assert_eq!(v["tool_calls_made"][0]["name"], "glob");
}

#[test]
fn build_result_with_error_envelope() {
    let res = build_result(
        "3",
        BuildResultArgs {
            status: "error".into(),
            error: Some(ErrorEnvelope {
                code: "provider_error".into(),
                message: "something broke".into(),
                retryable: true,
                details: None,
            }),
            iterations: 0,
            ..Default::default()
        },
    );
    let v: Value = serde_json::to_value(&res).unwrap();
    assert_eq!(v["status"], "error");
    assert_eq!(v["error"]["code"], "provider_error");
}

#[test]
fn build_result_with_correlation_and_latency() {
    let res = build_result(
        "2",
        BuildResultArgs {
            status: "ok".into(),
            response: Some("hello".into()),
            iterations: 1,
            correlation: Some(CorrelationContext {
                session_id: Some("sess-1".into()),
                task_id: Some("task-1".into()),
                worker_id: Some("worker-0".into()),
            }),
            total_latency_ms: Some(200),
            model_latency_ms: Some(150),
            tool_latency_ms: Some(50),
            ..Default::default()
        },
    );
    let v: Value = serde_json::to_value(&res).unwrap();
    assert_eq!(v["session_id"], "sess-1");
    assert_eq!(v["total_latency_ms"], 200);
    assert_eq!(v["model_latency_ms"], 150);
    assert_eq!(v["tool_latency_ms"], 50);
}

#[test]
fn build_result_omits_when_not_provided() {
    let res = build_result(
        "2",
        BuildResultArgs {
            status: "ok".into(),
            iterations: 1,
            ..Default::default()
        },
    );
    let s = encode_response(&res);
    assert!(!s.contains("session_id"));
    assert!(!s.contains("task_id"));
    assert!(!s.contains("worker_id"));
    assert!(!s.contains("total_latency_ms"));
    assert!(!s.contains("model_latency_ms"));
    assert!(!s.contains("tool_latency_ms"));
}

#[test]
fn build_error_envelope() {
    let res = build_error(
        Some("5"),
        ErrorEnvelope {
            code: "protocol_error".into(),
            message: "bad request".into(),
            retryable: false,
            details: None,
        },
        None,
    );
    let v: Value = serde_json::to_value(&res).unwrap();
    assert_eq!(v["type"], "result");
    assert_eq!(v["id"], "5");
    assert_eq!(v["status"], "error");
    assert_eq!(v["error"]["code"], "protocol_error");
    assert_eq!(v["iterations"], 0);
}

#[test]
fn build_error_unknown_id_default() {
    let res = build_error(
        None,
        ErrorEnvelope {
            code: "protocol_error".into(),
            message: "parse error".into(),
            retryable: false,
            details: None,
        },
        None,
    );
    let v: Value = serde_json::to_value(&res).unwrap();
    assert_eq!(v["id"], "unknown");
}

#[test]
fn build_error_with_correlation() {
    let res = build_error(
        Some("5"),
        ErrorEnvelope {
            code: "protocol_error".into(),
            message: "bad".into(),
            retryable: false,
            details: None,
        },
        Some(&CorrelationContext {
            session_id: Some("sess-1".into()),
            task_id: Some("task-1".into()),
            ..Default::default()
        }),
    );
    let v: Value = serde_json::to_value(&res).unwrap();
    assert_eq!(v["session_id"], "sess-1");
    assert_eq!(v["task_id"], "task-1");
}
