use heddle::permissions::defaults::{generate_default_permissions_toml, DEFAULT_DENY_RULES};
use heddle::permissions::rules::parse_rule;

mod common;

#[test]
fn contains_env_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Write(.env*)"));
}

#[test]
fn contains_pem_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Write(*.pem)"));
}

#[test]
fn contains_key_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Write(*.key)"));
}

#[test]
fn contains_credentials_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Write(*credentials*)"));
}

#[test]
fn contains_rm_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Bash(rm *)"));
    assert!(DEFAULT_DENY_RULES.contains(&"Bash(rm)"));
}

#[test]
fn contains_sudo_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Bash(sudo *)"));
}

#[test]
fn contains_chmod_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Bash(chmod *)"));
}

#[test]
fn contains_config_self_protection() {
    assert!(DEFAULT_DENY_RULES.contains(&"Write(.heddle/config.toml)"));
}

#[test]
fn all_rules_parse() {
    for rule in DEFAULT_DENY_RULES {
        assert!(parse_rule(rule).is_some(), "could not parse rule: {rule}");
    }
}

#[test]
fn toml_fragment_valid() {
    let toml = generate_default_permissions_toml();
    assert!(toml.contains("[permissions]"));
    assert!(toml.contains("deny = ["));
    assert!(toml.contains("Write(.env*)"));
    assert!(toml.contains("Bash(rm *)"));
}

#[test]
fn toml_includes_all_rules() {
    let toml = generate_default_permissions_toml();
    for rule in DEFAULT_DENY_RULES {
        assert!(toml.contains(rule), "TOML missing rule: {rule}");
    }
}
