use heddle::context::pruning::{estimate_tokens, prune_tool_results, PruningOptions};
use heddle::types::{AssistantMessage, Message, SystemMessage, ToolMessage, UserMessage};

mod common;

fn sys(s: &str) -> Message {
    Message::System(SystemMessage {
        content: s.to_string(),
    })
}
fn user(s: &str) -> Message {
    Message::User(UserMessage {
        content: s.to_string(),
    })
}
fn assistant(s: &str) -> Message {
    Message::Assistant(AssistantMessage {
        content: Some(s.to_string()),
        tool_calls: None,
    })
}
fn tool(id: &str, content: &str) -> Message {
    Message::Tool(ToolMessage {
        tool_call_id: id.to_string(),
        content: content.to_string(),
    })
}

#[test]
fn estimate_tokens_roughly_length_over_4() {
    let messages = vec![user("hello world")];
    let serialized = serde_json::to_string(&messages).unwrap();
    let expected = serialized.len().div_ceil(4) as u64;
    assert_eq!(estimate_tokens(&messages), expected);
}

#[test]
fn estimate_tokens_zero_for_empty() {
    assert_eq!(estimate_tokens(&[]), 0);
}

#[test]
fn prune_returns_struct_fields() {
    let mut messages = vec![
        sys("sys"),
        tool("t1", &"x".repeat(500)),
        user("recent"),
        assistant("recent"),
    ];
    let r = prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: false,
        },
    );
    // Just check the fields exist with sensible values.
    let _ = r.messages_pruned;
    let _ = r.tokens_before;
    let _ = r.tokens_after;
}

#[test]
fn before_gt_after_when_pruned() {
    let mut messages = vec![
        sys("sys"),
        tool("t1", &"x".repeat(1000)),
        tool("t2", &"x".repeat(1000)),
        user("recent"),
        assistant("recent"),
    ];
    let r = prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: false,
        },
    );
    assert!(r.messages_pruned > 0);
    assert!(r.tokens_before > r.tokens_after);
}

#[test]
fn before_eq_after_when_nothing_pruned() {
    let mut messages = vec![sys("sys"), user("hi"), assistant("hello")];
    let r = prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(999_999),
            protect_window_tokens: None,
            is_compaction_output: false,
        },
    );
    assert_eq!(r.messages_pruned, 0);
    assert_eq!(r.tokens_before, r.tokens_after);
}

#[test]
fn skips_when_compaction_output() {
    let original = "x".repeat(1000);
    let mut messages = vec![
        sys("sys"),
        tool("t1", &original),
        user("recent"),
        assistant("recent"),
    ];
    let r = prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: true,
        },
    );
    assert_eq!(r.messages_pruned, 0);
    if let Message::Tool(t) = &messages[1] {
        assert_eq!(t.content, original);
    } else {
        panic!("expected tool");
    }
}

#[test]
fn prunes_old_tool_messages_beyond_protection() {
    let long = "x".repeat(1000);
    let mut messages = vec![
        sys("system"),
        user("q1"),
        assistant("a1"),
        tool("t1", &long),
        user("q2"),
        assistant("a2"),
        tool("t2", &long),
        user("recent"),
        assistant("recent response"),
    ];
    let r = prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: false,
        },
    );
    assert!(r.messages_pruned > 0);
    if let Message::Tool(t) = &messages[3] {
        assert!(t.content.starts_with("[pruned"));
    } else {
        panic!("expected tool at index 3");
    }
}

#[test]
fn preserves_system_at_index_0() {
    let long = "x".repeat(1000);
    let mut messages = vec![
        sys(&long),
        user("q"),
        assistant("a"),
        tool("t1", &long),
        user("recent"),
    ];
    prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: false,
        },
    );
    if let Message::System(s) = &messages[0] {
        assert_eq!(s.content, long);
    } else {
        panic!("expected system");
    }
}

#[test]
fn placeholder_format() {
    let content = "a".repeat(500);
    let mut messages = vec![
        sys("sys"),
        tool("t1", &content),
        user("recent q"),
        assistant("recent a"),
    ];
    prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: false,
        },
    );
    if let Message::Tool(t) = &messages[1] {
        assert_eq!(
            t.content,
            format!("[pruned — original: {} chars]", content.len())
        );
    } else {
        panic!("expected tool");
    }
}

#[test]
fn idempotent_rerun() {
    let content = "x".repeat(500);
    let mut messages = vec![
        sys("sys"),
        tool("t1", &content),
        tool("t2", &content),
        user("recent"),
        assistant("recent"),
    ];
    let opts = PruningOptions {
        prune_threshold_tokens: Some(1),
        protect_window_tokens: Some(50),
        is_compaction_output: false,
    };
    let r1 = prune_tool_results(&mut messages, &opts);
    assert_eq!(r1.messages_pruned, 2);
    let r2 = prune_tool_results(&mut messages, &opts);
    assert_eq!(r2.messages_pruned, 0);
}

#[test]
fn no_tool_messages_returns_zero() {
    let mut messages = vec![
        sys("sys"),
        user("hello"),
        assistant("hi"),
        user("how are you"),
        assistant("good"),
    ];
    let r = prune_tool_results(
        &mut messages,
        &PruningOptions {
            prune_threshold_tokens: Some(1),
            protect_window_tokens: Some(50),
            is_compaction_output: false,
        },
    );
    assert_eq!(r.messages_pruned, 0);
}
