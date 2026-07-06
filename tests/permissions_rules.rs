use heddle::permissions::rules::{
    evaluate_rules, match_rule, merge_configs, parse_rule, ParsedRule, PermissionConfig,
    PermissionRule, RuleDecision,
};
use serde_json::json;

mod common;

fn rule(tool: &str, pattern: Option<&str>) -> PermissionRule {
    PermissionRule {
        tool: tool.to_string(),
        pattern: pattern.map(String::from),
    }
}

fn one(p: ParsedRule) -> PermissionRule {
    match p {
        ParsedRule::One(r) => r,
        ParsedRule::Many(_) => panic!("expected One"),
    }
}

fn many(p: ParsedRule) -> Vec<PermissionRule> {
    match p {
        ParsedRule::Many(r) => r,
        ParsedRule::One(_) => panic!("expected Many"),
    }
}

// ── parse_rule ──

#[test]
fn parse_bare_tool_name() {
    assert_eq!(one(parse_rule("Read").unwrap()), rule("read_file", None));
}

#[test]
fn parse_tool_with_pattern() {
    assert_eq!(
        one(parse_rule("Write(src/**)").unwrap()),
        rule("write_file", Some("src/**"))
    );
}

#[test]
fn parse_env_pattern() {
    assert_eq!(
        one(parse_rule("Write(.env*)").unwrap()),
        rule("write_file", Some(".env*"))
    );
}

#[test]
fn parse_bash_with_command() {
    assert_eq!(
        one(parse_rule("Bash(rm *)").unwrap()),
        rule("bash", Some("rm *"))
    );
}

#[test]
fn parse_bare_bash() {
    assert_eq!(one(parse_rule("Bash").unwrap()), rule("bash", None));
}

#[test]
fn parse_edit_to_edit_file() {
    assert_eq!(
        one(parse_rule("Edit(*.ts)").unwrap()),
        rule("edit_file", Some("*.ts"))
    );
}

#[test]
fn parse_glob_grep() {
    assert_eq!(one(parse_rule("Glob").unwrap()), rule("glob", None));
    assert_eq!(one(parse_rule("Grep").unwrap()), rule("grep", None));
}

#[test]
fn parse_webfetch_to_web_fetch() {
    assert_eq!(
        one(parse_rule("WebFetch(*.npmjs.org)").unwrap()),
        rule("web_fetch", Some("*.npmjs.org"))
    );
}

#[test]
fn parse_write_category_expands() {
    let rules = many(parse_rule("write").unwrap());
    let tools: Vec<&str> = rules.iter().map(|r| r.tool.as_str()).collect();
    assert!(tools.contains(&"write_file"));
    assert!(tools.contains(&"edit_file"));
    assert!(tools.contains(&"save_memory"));
}

#[test]
fn parse_category_with_pattern() {
    let rules = many(parse_rule("write(src/**)").unwrap());
    for r in rules {
        assert_eq!(r.pattern.as_deref(), Some("src/**"));
    }
}

#[test]
fn parse_read_category_expands() {
    let rules = many(parse_rule("read").unwrap());
    let tools: Vec<&str> = rules.iter().map(|r| r.tool.as_str()).collect();
    assert!(tools.contains(&"read_file"));
    assert!(tools.contains(&"glob"));
    assert!(tools.contains(&"grep"));
}

#[test]
fn parse_execute_category_expands_to_bash() {
    let rules = many(parse_rule("execute").unwrap());
    assert_eq!(rules, vec![rule("bash", None)]);
}

#[test]
fn parse_network_category_expands_to_web_fetch() {
    let rules = many(parse_rule("network").unwrap());
    assert_eq!(rules, vec![rule("web_fetch", None)]);
}

#[test]
fn parse_wildcard() {
    assert_eq!(one(parse_rule("*").unwrap()), rule("*", None));
}

#[test]
fn parse_invalid_returns_none() {
    assert!(parse_rule("").is_none());
}

#[test]
fn parse_unclosed_paren_returns_none() {
    assert!(parse_rule("Write(src/**").is_none());
}

#[test]
fn parse_case_insensitive_tool_name() {
    assert_eq!(
        one(parse_rule("write_file(src/**)").unwrap()),
        rule("write_file", Some("src/**"))
    );
}

#[test]
fn parse_already_snake_case() {
    assert_eq!(
        one(parse_rule("read_file").unwrap()),
        rule("read_file", None)
    );
}

// ── match_rule ──

#[test]
fn match_exact_tool_no_pattern() {
    assert!(match_rule(&rule("bash", None), "bash", None));
}

#[test]
fn match_exact_tool_no_match() {
    assert!(!match_rule(&rule("bash", None), "read_file", None));
}

#[test]
fn match_wildcard_matches_any() {
    assert!(match_rule(&rule("*", None), "bash", None));
    assert!(match_rule(&rule("*", None), "write_file", None));
}

#[test]
fn match_glob_pattern_path() {
    let r = rule("write_file", Some("src/**"));
    let args = json!({"path": "src/foo/bar.ts"});
    assert!(match_rule(&r, "write_file", Some(&args)));
}

#[test]
fn match_glob_pattern_path_no_match() {
    let r = rule("write_file", Some("src/**"));
    let args = json!({"path": "test/foo.ts"});
    assert!(!match_rule(&r, "write_file", Some(&args)));
}

#[test]
fn match_basename_env() {
    let r = rule("write_file", Some(".env*"));
    let args = json!({"path": "/project/dir/.env.local"});
    assert!(match_rule(&r, "write_file", Some(&args)));
}

#[test]
fn match_basename_pem() {
    let r = rule("write_file", Some("*.pem"));
    let args = json!({"path": "/home/user/cert.pem"});
    assert!(match_rule(&r, "write_file", Some(&args)));
}

#[test]
fn match_command_pattern_bash() {
    let r = rule("bash", Some("rm *"));
    let args = json!({"command": "rm -rf /tmp"});
    assert!(match_rule(&r, "bash", Some(&args)));
}

#[test]
fn match_command_no_match() {
    let r = rule("bash", Some("rm *"));
    let args = json!({"command": "ls -la"});
    assert!(!match_rule(&r, "bash", Some(&args)));
}

#[test]
fn match_bare_rm() {
    let r = rule("bash", Some("rm"));
    let args = json!({"command": "rm"});
    assert!(match_rule(&r, "bash", Some(&args)));
}

#[test]
fn match_bare_rm_no_match_with_args() {
    let r = rule("bash", Some("rm"));
    let args = json!({"command": "rm file.txt"});
    assert!(!match_rule(&r, "bash", Some(&args)));
}

#[test]
fn match_pattern_no_args_fails() {
    let r = rule("write_file", Some("src/**"));
    assert!(!match_rule(&r, "write_file", None));
}

#[test]
fn match_host_pattern_web_fetch() {
    let r = rule("web_fetch", Some("*.npmjs.org"));
    let args = json!({"url": "https://registry.npmjs.org/package"});
    assert!(match_rule(&r, "web_fetch", Some(&args)));
}

#[test]
fn match_host_no_match() {
    let r = rule("web_fetch", Some("*.npmjs.org"));
    let args = json!({"url": "https://example.com/foo"});
    assert!(!match_rule(&r, "web_fetch", Some(&args)));
}

#[test]
fn match_sudo_pattern() {
    let r = rule("bash", Some("sudo *"));
    let args = json!({"command": "sudo rm -rf /"});
    assert!(match_rule(&r, "bash", Some(&args)));
}

#[test]
fn match_chmod_pattern() {
    let r = rule("bash", Some("chmod *"));
    let args = json!({"command": "chmod 777 file"});
    assert!(match_rule(&r, "bash", Some(&args)));
}

// ── evaluate_rules ──

#[test]
fn deny_wins_over_allow_same_layer() {
    let cfg = PermissionConfig {
        allow: vec![rule("write_file", None)],
        deny: vec![rule("write_file", Some(".env*"))],
        ask: vec![],
    };
    let args = json!({"path": ".env"});
    assert_eq!(
        evaluate_rules(&cfg, "write_file", Some(&args)),
        Some(RuleDecision::Deny)
    );
}

#[test]
fn allow_applies_when_no_deny() {
    let cfg = PermissionConfig {
        allow: vec![rule("write_file", None)],
        deny: vec![rule("write_file", Some(".env*"))],
        ask: vec![],
    };
    let args = json!({"path": "src/foo.ts"});
    assert_eq!(
        evaluate_rules(&cfg, "write_file", Some(&args)),
        Some(RuleDecision::Allow)
    );
}

#[test]
fn ask_takes_precedence_over_allow() {
    let cfg = PermissionConfig {
        allow: vec![rule("bash", None)],
        deny: vec![],
        ask: vec![rule("bash", Some("git push *"))],
    };
    let args = json!({"command": "git push origin main"});
    assert_eq!(
        evaluate_rules(&cfg, "bash", Some(&args)),
        Some(RuleDecision::Ask)
    );
}

#[test]
fn deny_takes_precedence_over_ask() {
    let cfg = PermissionConfig {
        allow: vec![],
        deny: vec![rule("bash", Some("rm *"))],
        ask: vec![rule("bash", Some("rm *"))],
    };
    let args = json!({"command": "rm file.txt"});
    assert_eq!(
        evaluate_rules(&cfg, "bash", Some(&args)),
        Some(RuleDecision::Deny)
    );
}

#[test]
fn evaluate_returns_none_when_no_match() {
    let cfg = PermissionConfig {
        allow: vec![rule("read_file", None)],
        deny: vec![],
        ask: vec![],
    };
    let args = json!({"path": "foo.ts"});
    assert_eq!(evaluate_rules(&cfg, "write_file", Some(&args)), None);
}

#[test]
fn evaluate_empty_returns_none() {
    let cfg = PermissionConfig::default();
    let args = json!({"command": "ls"});
    assert_eq!(evaluate_rules(&cfg, "bash", Some(&args)), None);
}

// ── merge_configs ──

#[test]
fn merge_stacks_rules() {
    let global = PermissionConfig {
        allow: vec![rule("read_file", None)],
        deny: vec![rule("write_file", Some(".env*"))],
        ask: vec![],
    };
    let local = PermissionConfig {
        allow: vec![rule("bash", Some("cargo *"))],
        deny: vec![],
        ask: vec![rule("bash", Some("git push *"))],
    };
    let merged = merge_configs(&[global, local]);
    assert_eq!(merged.allow.len(), 2);
    assert_eq!(merged.deny.len(), 1);
    assert_eq!(merged.ask.len(), 1);
}

#[test]
fn merge_more_specific_allow_overrides_deny() {
    let global = PermissionConfig {
        allow: vec![],
        deny: vec![rule("write_file", Some(".env*"))],
        ask: vec![],
    };
    let local = PermissionConfig {
        allow: vec![rule("write_file", Some(".env*"))],
        deny: vec![],
        ask: vec![],
    };
    let merged = merge_configs(&[global, local]);
    let args = json!({"path": ".env.local"});
    assert_eq!(
        evaluate_rules(&merged, "write_file", Some(&args)),
        Some(RuleDecision::Allow)
    );
}

#[test]
fn merge_more_specific_deny_overrides_allow() {
    let global = PermissionConfig {
        allow: vec![rule("bash", None)],
        deny: vec![],
        ask: vec![],
    };
    let local = PermissionConfig {
        allow: vec![],
        deny: vec![rule("bash", Some("rm *"))],
        ask: vec![],
    };
    let merged = merge_configs(&[global, local]);
    let args = json!({"command": "rm file.txt"});
    assert_eq!(
        evaluate_rules(&merged, "bash", Some(&args)),
        Some(RuleDecision::Deny)
    );
}

#[test]
fn merge_single_returns_equivalent() {
    let cfg = PermissionConfig {
        allow: vec![rule("read_file", None)],
        deny: vec![rule("bash", Some("rm *"))],
        ask: vec![],
    };
    let merged = merge_configs(&[cfg]);
    assert_eq!(
        evaluate_rules(&merged, "read_file", None),
        Some(RuleDecision::Allow)
    );
    let args = json!({"command": "rm foo"});
    assert_eq!(
        evaluate_rules(&merged, "bash", Some(&args)),
        Some(RuleDecision::Deny)
    );
}

#[test]
fn merge_empty_returns_empty() {
    let merged = merge_configs(&[]);
    assert!(merged.allow.is_empty());
    assert!(merged.deny.is_empty());
    assert!(merged.ask.is_empty());
}
