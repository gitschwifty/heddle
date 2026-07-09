use heddle::config::loader::{load_config, ApprovalMode};

mod common;
use common::Sandbox;

fn write_global(sb: &Sandbox, contents: &str) {
    std::fs::write(sb.heddle_home.join("config.toml"), contents).unwrap();
}

fn write_local(sb: &Sandbox, contents: &str) {
    let local_dir = sb.project.join(".heddle");
    std::fs::create_dir_all(&local_dir).unwrap();
    std::fs::write(local_dir.join("config.toml"), contents).unwrap();
}

fn clear_env() {
    for k in [
        "HEDDLE_MODEL",
        "OPENROUTER_API_KEY",
        "HEDDLE_BASE_URL",
        "HEDDLE_MAX_TOKENS",
        "HEDDLE_TEMPERATURE",
        "HEDDLE_WEAK_MODEL",
        "HEDDLE_APPROVAL_MODE",
        "HEDDLE_TOOLS",
        "HEDDLE_WEB_FETCH_ALLOW_PRIVATE_ADDRESSES",
    ] {
        std::env::remove_var(k);
    }
}

#[test]
fn defaults_when_no_config_files() {
    let _sb = Sandbox::new("loader-defaults");
    clear_env();
    let cfg = load_config(None);
    assert_eq!(cfg.model, "openrouter/free");
    assert!(cfg.api_key.is_none());
}

#[test]
fn loads_global_config_toml() {
    let sb = Sandbox::new("loader-global");
    clear_env();
    write_global(&sb, r#"model = "anthropic/claude-sonnet""#);
    let cfg = load_config(None);
    assert_eq!(cfg.model, "anthropic/claude-sonnet");
}

#[test]
fn local_overrides_global() {
    let sb = Sandbox::new("loader-merge");
    clear_env();
    write_global(
        &sb,
        "model = \"global-model\"\nsystem_prompt = \"global prompt\"\n",
    );
    write_local(&sb, "model = \"local-model\"\n");
    let cfg = load_config(None);
    assert_eq!(cfg.model, "local-model");
    assert_eq!(cfg.system_prompt.as_deref(), Some("global prompt"));
}

#[test]
fn env_overrides_config() {
    let sb = Sandbox::new("loader-env");
    clear_env();
    write_global(&sb, "model = \"file-model\"\n");
    std::env::set_var("HEDDLE_MODEL", "env-model");
    std::env::set_var("OPENROUTER_API_KEY", "env-key");
    let cfg = load_config(None);
    assert_eq!(cfg.model, "env-model");
    assert_eq!(cfg.api_key.as_deref(), Some("env-key"));
    clear_env();
}

#[test]
fn malformed_toml_returns_defaults() {
    let sb = Sandbox::new("loader-malformed");
    clear_env();
    write_global(&sb, "this is not valid toml [[[");
    let cfg = load_config(None);
    assert_eq!(cfg.model, "openrouter/free");
}

#[test]
fn empty_config_file_uses_defaults() {
    let sb = Sandbox::new("loader-empty");
    clear_env();
    write_global(&sb, "");
    let cfg = load_config(None);
    assert_eq!(cfg.model, "openrouter/free");
}

#[test]
fn loads_api_key_from_config() {
    let sb = Sandbox::new("loader-apikey");
    clear_env();
    write_global(&sb, r#"api_key = "sk-from-config""#);
    let cfg = load_config(None);
    assert_eq!(cfg.api_key.as_deref(), Some("sk-from-config"));
}

#[test]
fn loads_weak_model() {
    let sb = Sandbox::new("loader-weak");
    clear_env();
    write_global(&sb, r#"weak_model = "openrouter/free""#);
    let cfg = load_config(None);
    assert_eq!(cfg.weak_model.as_deref(), Some("openrouter/free"));
}

#[test]
fn loads_editor_model() {
    let sb = Sandbox::new("loader-editor");
    clear_env();
    write_global(&sb, r#"editor_model = "anthropic/claude-opus""#);
    let cfg = load_config(None);
    assert_eq!(cfg.editor_model.as_deref(), Some("anthropic/claude-opus"));
}

#[test]
fn loads_max_tokens() {
    let sb = Sandbox::new("loader-maxtok");
    clear_env();
    write_global(&sb, "max_tokens = 4096\n");
    let cfg = load_config(None);
    assert_eq!(cfg.max_tokens, Some(4096));
}

#[test]
fn loads_temperature() {
    let sb = Sandbox::new("loader-temp");
    clear_env();
    write_global(&sb, "temperature = 0.7\n");
    let cfg = load_config(None);
    assert_eq!(cfg.temperature, Some(0.7));
}

#[test]
fn temperature_zero_is_valid() {
    let sb = Sandbox::new("loader-temp-zero");
    clear_env();
    write_global(&sb, "temperature = 0.0\n");
    let cfg = load_config(None);
    assert_eq!(cfg.temperature, Some(0.0));
}

#[test]
fn loads_base_url() {
    let sb = Sandbox::new("loader-baseurl");
    clear_env();
    write_global(&sb, r#"base_url = "http://localhost:8080""#);
    let cfg = load_config(None);
    assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:8080"));
}

#[test]
fn loads_doom_loop_threshold() {
    let sb = Sandbox::new("loader-doom");
    clear_env();
    write_global(&sb, "doom_loop_threshold = 5\n");
    let cfg = load_config(None);
    assert_eq!(cfg.doom_loop_threshold, Some(5));
}

#[test]
fn loads_budget_limit() {
    let sb = Sandbox::new("loader-budget");
    clear_env();
    write_global(&sb, "budget_limit = 1.50\n");
    let cfg = load_config(None);
    assert_eq!(cfg.budget_limit, Some(1.5));
}

#[test]
fn approval_mode_yolo_accepted() {
    let sb = Sandbox::new("loader-yolo");
    clear_env();
    write_global(&sb, r#"approval_mode = "yolo""#);
    let cfg = load_config(None);
    assert_eq!(cfg.approval_mode, Some(ApprovalMode::Yolo));
}

#[test]
fn approval_mode_suggest_accepted() {
    let sb = Sandbox::new("loader-suggest");
    clear_env();
    write_global(&sb, r#"approval_mode = "suggest""#);
    let cfg = load_config(None);
    assert_eq!(cfg.approval_mode, Some(ApprovalMode::Suggest));
}

#[test]
fn invalid_approval_mode_dropped() {
    let sb = Sandbox::new("loader-invalid-am");
    clear_env();
    write_global(&sb, r#"approval_mode = "invalid""#);
    let cfg = load_config(None);
    assert!(cfg.approval_mode.is_none());
}

#[test]
fn instructions_array_loaded() {
    let sb = Sandbox::new("loader-instr-arr");
    clear_env();
    write_global(&sb, r#"instructions = ["HEDDLE.md", "AGENTS.md"]"#);
    let cfg = load_config(None);
    assert_eq!(
        cfg.instructions,
        Some(vec!["HEDDLE.md".to_string(), "AGENTS.md".to_string()])
    );
}

#[test]
fn instructions_string_rejected() {
    let sb = Sandbox::new("loader-instr-str");
    clear_env();
    write_global(&sb, r#"instructions = "HEDDLE.md""#);
    let cfg = load_config(None);
    assert!(cfg.instructions.is_none());
}

#[test]
fn empty_config_all_optional_unset() {
    let sb = Sandbox::new("loader-allnone");
    clear_env();
    write_global(&sb, "");
    let cfg = load_config(None);
    assert!(cfg.weak_model.is_none());
    assert!(cfg.editor_model.is_none());
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.base_url.is_none());
    assert!(cfg.approval_mode.is_none());
    assert!(cfg.instructions.is_none());
    assert!(cfg.doom_loop_threshold.is_none());
    assert!(cfg.budget_limit.is_none());
    assert!(!cfg.web_fetch_allow_private_addresses);
}

#[test]
fn heddle_base_url_env_overrides() {
    let sb = Sandbox::new("loader-env-baseurl");
    clear_env();
    write_global(&sb, r#"base_url = "http://toml-url""#);
    std::env::set_var("HEDDLE_BASE_URL", "http://env-url");
    let cfg = load_config(None);
    assert_eq!(cfg.base_url.as_deref(), Some("http://env-url"));
    clear_env();
}

#[test]
fn loads_web_fetch_private_address_policy() {
    let sb = Sandbox::new("loader-webfetch-private");
    clear_env();
    write_global(&sb, "web_fetch_allow_private_addresses = true\n");
    let cfg = load_config(None);
    assert!(cfg.web_fetch_allow_private_addresses);
}

#[test]
fn heddle_web_fetch_private_address_env_overrides() {
    let sb = Sandbox::new("loader-env-webfetch-private");
    clear_env();
    write_global(&sb, "web_fetch_allow_private_addresses = false\n");
    std::env::set_var("HEDDLE_WEB_FETCH_ALLOW_PRIVATE_ADDRESSES", "true");
    let cfg = load_config(None);
    assert!(cfg.web_fetch_allow_private_addresses);
    clear_env();
}

#[test]
fn heddle_max_tokens_env_overrides() {
    let _sb = Sandbox::new("loader-env-maxtok");
    clear_env();
    std::env::set_var("HEDDLE_MAX_TOKENS", "8192");
    let cfg = load_config(None);
    assert_eq!(cfg.max_tokens, Some(8192));
    clear_env();
}

#[test]
fn heddle_temperature_env_overrides() {
    let _sb = Sandbox::new("loader-env-temp");
    clear_env();
    std::env::set_var("HEDDLE_TEMPERATURE", "0.5");
    let cfg = load_config(None);
    assert_eq!(cfg.temperature, Some(0.5));
    clear_env();
}

#[test]
fn heddle_temperature_env_zero_valid() {
    let _sb = Sandbox::new("loader-env-temp-zero");
    clear_env();
    std::env::set_var("HEDDLE_TEMPERATURE", "0");
    let cfg = load_config(None);
    assert_eq!(cfg.temperature, Some(0.0));
    clear_env();
}

#[test]
fn empty_numeric_env_vars_dont_set() {
    let _sb = Sandbox::new("loader-env-empty-num");
    clear_env();
    std::env::set_var("HEDDLE_MAX_TOKENS", "");
    std::env::set_var("HEDDLE_TEMPERATURE", "");
    let cfg = load_config(None);
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    clear_env();
}

#[test]
fn nonnumeric_env_vars_dont_set() {
    let _sb = Sandbox::new("loader-env-nonnum");
    clear_env();
    std::env::set_var("HEDDLE_MAX_TOKENS", "abc");
    std::env::set_var("HEDDLE_TEMPERATURE", "not-a-number");
    let cfg = load_config(None);
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    clear_env();
}

#[test]
fn heddle_weak_model_env_overrides() {
    let sb = Sandbox::new("loader-env-weak");
    clear_env();
    write_global(&sb, r#"weak_model = "toml-weak""#);
    std::env::set_var("HEDDLE_WEAK_MODEL", "env-weak");
    let cfg = load_config(None);
    assert_eq!(cfg.weak_model.as_deref(), Some("env-weak"));
    clear_env();
}

#[test]
fn heddle_approval_mode_env_overrides() {
    let sb = Sandbox::new("loader-env-am");
    clear_env();
    write_global(&sb, r#"approval_mode = "suggest""#);
    std::env::set_var("HEDDLE_APPROVAL_MODE", "full-auto");
    let cfg = load_config(None);
    assert_eq!(cfg.approval_mode, Some(ApprovalMode::FullAuto));
    clear_env();
}

#[test]
fn heddle_approval_mode_env_invalid_dropped() {
    let _sb = Sandbox::new("loader-env-am-invalid");
    clear_env();
    std::env::set_var("HEDDLE_APPROVAL_MODE", "banana");
    let cfg = load_config(None);
    assert!(cfg.approval_mode.is_none());
    clear_env();
}

#[test]
fn loads_tools_array() {
    let sb = Sandbox::new("loader-tools-arr");
    clear_env();
    write_global(&sb, r#"tools = ["read_file", "glob", "grep"]"#);
    let cfg = load_config(None);
    assert_eq!(
        cfg.tools,
        Some(vec![
            "read_file".to_string(),
            "glob".to_string(),
            "grep".to_string()
        ])
    );
}

#[test]
fn tools_string_rejected() {
    let sb = Sandbox::new("loader-tools-str");
    clear_env();
    write_global(&sb, r#"tools = "read_file""#);
    let cfg = load_config(None);
    assert!(cfg.tools.is_none());
}

#[test]
fn heddle_tools_env_csv() {
    let _sb = Sandbox::new("loader-env-tools");
    clear_env();
    std::env::set_var("HEDDLE_TOOLS", "read_file,glob,grep");
    let cfg = load_config(None);
    assert_eq!(
        cfg.tools,
        Some(vec![
            "read_file".to_string(),
            "glob".to_string(),
            "grep".to_string()
        ])
    );
    clear_env();
}

#[test]
fn heddle_tools_env_trims_whitespace() {
    let _sb = Sandbox::new("loader-env-tools-ws");
    clear_env();
    std::env::set_var("HEDDLE_TOOLS", " read_file , glob , grep ");
    let cfg = load_config(None);
    assert_eq!(
        cfg.tools,
        Some(vec![
            "read_file".to_string(),
            "glob".to_string(),
            "grep".to_string()
        ])
    );
    clear_env();
}

#[test]
fn heddle_tools_env_overrides_toml() {
    let sb = Sandbox::new("loader-env-tools-over");
    clear_env();
    write_global(&sb, r#"tools = ["read_file", "write_file"]"#);
    std::env::set_var("HEDDLE_TOOLS", "glob,grep");
    let cfg = load_config(None);
    assert_eq!(
        cfg.tools,
        Some(vec!["glob".to_string(), "grep".to_string()])
    );
    clear_env();
}

#[test]
fn empty_heddle_tools_env_doesnt_set() {
    let _sb = Sandbox::new("loader-env-tools-empty");
    clear_env();
    std::env::set_var("HEDDLE_TOOLS", "");
    let cfg = load_config(None);
    assert!(cfg.tools.is_none());
    clear_env();
}

// ── Permissions layers ──

#[test]
fn loads_permissions_deny_global() {
    let sb = Sandbox::new("loader-perms-global");
    clear_env();
    write_global(
        &sb,
        "[permissions]\ndeny = [\"Write(.env*)\", \"Bash(rm *)\"]\n",
    );
    let cfg = load_config(None);
    let layers = cfg.permissions_layers.unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].deny, vec!["Write(.env*)", "Bash(rm *)"]);
    assert!(layers[0].allow.is_empty());
}

#[test]
fn loads_permissions_two_layers() {
    let sb = Sandbox::new("loader-perms-layers");
    clear_env();
    write_global(&sb, "[permissions]\ndeny = [\"Write(.env*)\"]\n");
    write_local(
        &sb,
        "[permissions]\nallow = [\"Write(src/**)\"]\ndeny = [\"Bash(rm *)\"]\n",
    );
    let cfg = load_config(None);
    let layers = cfg.permissions_layers.unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].deny, vec!["Write(.env*)"]);
    assert_eq!(layers[1].allow, vec!["Write(src/**)"]);
    assert_eq!(layers[1].deny, vec!["Bash(rm *)"]);
}

#[test]
fn loads_permissions_ask() {
    let sb = Sandbox::new("loader-perms-ask");
    clear_env();
    write_global(&sb, "[permissions]\nask = [\"Bash(git push *)\"]\n");
    let cfg = load_config(None);
    let layers = cfg.permissions_layers.unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].ask, vec!["Bash(git push *)"]);
}

#[test]
fn no_permissions_section_no_layers() {
    let sb = Sandbox::new("loader-perms-none");
    clear_env();
    write_global(&sb, r#"model = "test""#);
    let cfg = load_config(None);
    assert!(cfg.permissions_layers.is_none());
}

#[test]
fn empty_permissions_section_no_layers() {
    let sb = Sandbox::new("loader-perms-empty");
    clear_env();
    write_global(&sb, "[permissions]\n");
    let cfg = load_config(None);
    assert!(cfg.permissions_layers.is_none());
}

#[test]
fn non_string_perm_entries_dropped() {
    let sb = Sandbox::new("loader-perms-invalid");
    clear_env();
    write_global(&sb, "[permissions]\ndeny = [\"Write(.env*)\", 42]\n");
    let cfg = load_config(None);
    let layers = cfg.permissions_layers.unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].deny, vec!["Write(.env*)"]);
}
