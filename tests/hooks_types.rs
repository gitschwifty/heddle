use heddle::hooks::types::{HookEvent, HookMode};

#[test]
fn hook_event_from_str_accepts_valid_names() {
    for name in [
        "session_start",
        "session_end",
        "pre_prompt",
        "pre_tool",
        "post_tool",
        "post_turn",
        "error",
    ] {
        assert!(
            HookEvent::from_str(name).is_some(),
            "expected {name} to parse"
        );
    }
}

#[test]
fn hook_event_from_str_rejects_invalid_names() {
    assert!(HookEvent::from_str("invalid").is_none());
    assert!(HookEvent::from_str("").is_none());
}

#[test]
fn hook_event_round_trips_through_as_str() {
    let events = [
        HookEvent::SessionStart,
        HookEvent::SessionEnd,
        HookEvent::PrePrompt,
        HookEvent::PreTool,
        HookEvent::PostTool,
        HookEvent::PostTurn,
        HookEvent::Error,
    ];
    for e in events {
        let s = e.as_str();
        assert_eq!(HookEvent::from_str(s), Some(e));
    }
}

#[test]
fn hook_mode_serde_accepts_valid_values() {
    let m: HookMode = serde_json::from_str(r#""interactive""#).unwrap();
    assert_eq!(m, HookMode::Interactive);
    let m: HookMode = serde_json::from_str(r#""headless""#).unwrap();
    assert_eq!(m, HookMode::Headless);
    let m: HookMode = serde_json::from_str(r#""both""#).unwrap();
    assert_eq!(m, HookMode::Both);
}

#[test]
fn hook_mode_serde_rejects_invalid_values() {
    let r: Result<HookMode, _> = serde_json::from_str(r#""cli""#);
    assert!(r.is_err());
}
