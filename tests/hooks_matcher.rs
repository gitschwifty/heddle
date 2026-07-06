use heddle::hooks::matcher::matches_hook;
use heddle::hooks::types::{
    HookContext, HookMatchers, HookMode, ResolvedHookDefinition, ToolMatch,
};

mod common;

fn base_ctx() -> HookContext {
    HookContext {
        session_id: "test-session".into(),
        project: "/test/project".into(),
        model: "test-model".into(),
        event: "pre_tool".into(),
        ..Default::default()
    }
}

fn hook(matchers: Option<HookMatchers>) -> ResolvedHookDefinition {
    ResolvedHookDefinition {
        command: "echo test".into(),
        timeout: 10000,
        mode: HookMode::Both,
        r#async: false,
        matchers,
    }
}

#[test]
fn no_matchers_matches_everything() {
    assert!(matches_hook(&hook(None), &base_ctx()));
    let mut c = base_ctx();
    c.tool_name = Some("read".into());
    assert!(matches_hook(&hook(None), &c));
}

#[test]
fn tool_matcher_exact_string() {
    let h = hook(Some(HookMatchers {
        tool: Some(ToolMatch::Single("read".into())),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_name = Some("read".into());
    assert!(matches_hook(&h, &c));
    c.tool_name = Some("write".into());
    assert!(!matches_hook(&h, &c));
}

#[test]
fn tool_matcher_array_inclusion() {
    let h = hook(Some(HookMatchers {
        tool: Some(ToolMatch::Many(vec!["read".into(), "write".into()])),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_name = Some("read".into());
    assert!(matches_hook(&h, &c));
    c.tool_name = Some("write".into());
    assert!(matches_hook(&h, &c));
    c.tool_name = Some("bash".into());
    assert!(!matches_hook(&h, &c));
}

#[test]
fn tool_matcher_no_tool_name_fails() {
    let h = hook(Some(HookMatchers {
        tool: Some(ToolMatch::Single("read".into())),
        ..Default::default()
    }));
    assert!(!matches_hook(&h, &base_ctx()));
}

#[test]
fn match_path_glob_against_file_path() {
    let h = hook(Some(HookMatchers {
        match_path: Some("**/*.ts".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_args = Some(r#"{"file_path":"src/hooks/types.ts"}"#.into());
    assert!(matches_hook(&h, &c));
}

#[test]
fn match_path_non_matching() {
    let h = hook(Some(HookMatchers {
        match_path: Some("**/*.rs".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_args = Some(r#"{"file_path":"src/hooks/types.ts"}"#.into());
    assert!(!matches_hook(&h, &c));
}

#[test]
fn match_path_no_file_path_fails() {
    let h = hook(Some(HookMatchers {
        match_path: Some("**/*.ts".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_args = Some(r#"{"command":"ls"}"#.into());
    assert!(!matches_hook(&h, &c));
}

#[test]
fn match_path_no_args_fails() {
    let h = hook(Some(HookMatchers {
        match_path: Some("**/*.ts".into()),
        ..Default::default()
    }));
    assert!(!matches_hook(&h, &base_ctx()));
}

#[test]
fn match_args_glob() {
    let h = hook(Some(HookMatchers {
        match_args: Some("*secret*".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_args = Some(r#"{"command":"cat secret.txt"}"#.into());
    assert!(matches_hook(&h, &c));
}

#[test]
fn match_args_non_matching() {
    let h = hook(Some(HookMatchers {
        match_args: Some("*secret*".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.tool_args = Some(r#"{"command":"cat readme.md"}"#.into());
    assert!(!matches_hook(&h, &c));
}

#[test]
fn match_input_glob_against_user_input() {
    let h = hook(Some(HookMatchers {
        match_input: Some("*deploy*".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.user_input = Some("please deploy to production".into());
    assert!(matches_hook(&h, &c));
}

#[test]
fn match_input_non_matching() {
    let h = hook(Some(HookMatchers {
        match_input: Some("*deploy*".into()),
        ..Default::default()
    }));
    let mut c = base_ctx();
    c.user_input = Some("write some tests".into());
    assert!(!matches_hook(&h, &c));
}

#[test]
fn match_input_no_user_input_fails() {
    let h = hook(Some(HookMatchers {
        match_input: Some("*deploy*".into()),
        ..Default::default()
    }));
    assert!(!matches_hook(&h, &base_ctx()));
}

#[test]
fn combined_matchers_and_logic() {
    let h = hook(Some(HookMatchers {
        tool: Some(ToolMatch::Single("write".into())),
        match_path: Some("**/*.ts".into()),
        ..Default::default()
    }));

    // Both match
    let mut c1 = base_ctx();
    c1.tool_name = Some("write".into());
    c1.tool_args = Some(r#"{"file_path":"src/index.ts"}"#.into());
    assert!(matches_hook(&h, &c1));

    // Tool matches but path doesn't
    let mut c2 = base_ctx();
    c2.tool_name = Some("write".into());
    c2.tool_args = Some(r#"{"file_path":"src/index.rs"}"#.into());
    assert!(!matches_hook(&h, &c2));

    // Path matches but tool doesn't
    let mut c3 = base_ctx();
    c3.tool_name = Some("read".into());
    c3.tool_args = Some(r#"{"file_path":"src/index.ts"}"#.into());
    assert!(!matches_hook(&h, &c3));
}
