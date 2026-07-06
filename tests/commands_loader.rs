use heddle::commands::loader::load_custom_commands;
use heddle::commands::types::CommandContext;
use heddle::config::loader::HeddleConfig;
use heddle::cost::tracker::CostTracker;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::types::{ChatCompletionResponse, Message, ToolDefinition};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

mod common;
use common::Sandbox;

// ─── Minimal Provider stub for context construction ─────────────────────
struct NoopProvider;
#[async_trait::async_trait]
impl Provider for NoopProvider {
    async fn send(
        &self,
        _m: &[Message],
        _t: Option<&[ToolDefinition]>,
        _o: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        unimplemented!()
    }
    fn stream(&self, _m: Vec<Message>, _t: Option<Vec<ToolDefinition>>, _o: Value) -> ChunkStream {
        unimplemented!()
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        Arc::new(NoopProvider)
    }
}

fn write_md(dir: &PathBuf, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), body).unwrap();
}

#[test]
fn loads_md_files_from_commands_directory() {
    let sb = Sandbox::new("cmd-loader-basic");
    let cmds = sb.heddle_home.join("commands");
    write_md(&cmds, "deploy.md", "Run deployment steps");
    write_md(&cmds, "review.md", "Review the code");

    let commands = load_custom_commands(None);
    let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"deploy"));
    assert!(names.contains(&"review"));
}

#[test]
fn subdirectory_namespacing_with_colon() {
    let sb = Sandbox::new("cmd-loader-ns");
    let sub = sb.heddle_home.join("commands").join("posts");
    write_md(&sub, "new.md", "Create a new post");

    let commands = load_custom_commands(None);
    let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"posts:new"));
}

#[test]
fn skips_non_md_files() {
    let sb = Sandbox::new("cmd-loader-skip");
    let cmds = sb.heddle_home.join("commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(cmds.join("valid.md"), "Valid").unwrap();
    std::fs::write(cmds.join("ignored.txt"), "Not a command").unwrap();
    std::fs::write(cmds.join("also-ignored.js"), "Not a command").unwrap();

    let commands = load_custom_commands(None);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "valid");
}

#[test]
fn handles_missing_directories_gracefully() {
    let _sb = Sandbox::new("cmd-loader-missing");
    // Don't create any commands/skills dirs.
    let commands = load_custom_commands(None);
    assert!(commands.is_empty());
}

#[test]
fn loads_from_skills_directory_too() {
    let sb = Sandbox::new("cmd-loader-skills");
    let skills = sb.heddle_home.join("skills");
    write_md(&skills, "refactor.md", "Refactor the code");

    let commands = load_custom_commands(None);
    let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"refactor"));
}

#[tokio::test]
async fn execute_injects_content_as_user_message() {
    let sb = Sandbox::new("cmd-loader-exec");
    let cmds = sb.heddle_home.join("commands");
    write_md(&cmds, "greet.md", "Say hello to the user");

    let commands = load_custom_commands(None);
    let greet = commands.iter().find(|c| c.name == "greet").unwrap();

    let mut config = HeddleConfig::default();
    let mut messages: Vec<Message> = vec![];
    let registry = ToolRegistry::new();
    let agent_defs = HashMap::new();
    let mut ctx = CommandContext {
        config: &mut config,
        messages: &mut messages,
        registry: &registry,
        cost_tracker: Arc::new(Mutex::new(CostTracker::default())),
        session_file: sb.project.join("session.jsonl"),
        session_id: "test-session".to_string(),
        provider: Arc::new(NoopProvider),
        weak_provider: None,
        editor_provider: None,
        discovery: None,
        agent_definitions: &agent_defs,
        paste_cache: None,
    };

    (greet.execute)("", &mut ctx).await;

    assert_eq!(ctx.messages.len(), 1);
    match &ctx.messages[0] {
        Message::User(m) => assert_eq!(m.content, "Say hello to the user"),
        _ => panic!("expected user message"),
    }
}

#[tokio::test]
async fn execute_appends_args_to_content() {
    let sb = Sandbox::new("cmd-loader-args");
    let cmds = sb.heddle_home.join("commands");
    write_md(&cmds, "prompt.md", "Base prompt content");

    let commands = load_custom_commands(None);
    let prompt = commands.iter().find(|c| c.name == "prompt").unwrap();

    let mut config = HeddleConfig::default();
    let mut messages: Vec<Message> = vec![];
    let registry = ToolRegistry::new();
    let agent_defs = HashMap::new();
    let mut ctx = CommandContext {
        config: &mut config,
        messages: &mut messages,
        registry: &registry,
        cost_tracker: Arc::new(Mutex::new(CostTracker::default())),
        session_file: sb.project.join("session.jsonl"),
        session_id: "test".to_string(),
        provider: Arc::new(NoopProvider),
        weak_provider: None,
        editor_provider: None,
        discovery: None,
        agent_definitions: &agent_defs,
        paste_cache: None,
    };

    (prompt.execute)("extra arguments here", &mut ctx).await;

    match &ctx.messages[0] {
        Message::User(m) => assert_eq!(m.content, "Base prompt content\n\nextra arguments here"),
        _ => panic!("expected user message"),
    }
}

#[tokio::test]
async fn local_commands_override_global_commands() {
    let sb = Sandbox::new("cmd-loader-override");
    write_md(
        &sb.heddle_home.join("commands"),
        "deploy.md",
        "Global deploy",
    );
    write_md(
        &sb.project.join(".heddle").join("commands"),
        "deploy.md",
        "Local deploy",
    );

    let commands = load_custom_commands(None);
    let deploy = commands.iter().find(|c| c.name == "deploy").unwrap();

    let mut config = HeddleConfig::default();
    let mut messages: Vec<Message> = vec![];
    let registry = ToolRegistry::new();
    let agent_defs = HashMap::new();
    let mut ctx = CommandContext {
        config: &mut config,
        messages: &mut messages,
        registry: &registry,
        cost_tracker: Arc::new(Mutex::new(CostTracker::default())),
        session_file: sb.project.join("session.jsonl"),
        session_id: "t".to_string(),
        provider: Arc::new(NoopProvider),
        weak_provider: None,
        editor_provider: None,
        discovery: None,
        agent_definitions: &agent_defs,
        paste_cache: None,
    };

    (deploy.execute)("", &mut ctx).await;
    match &ctx.messages[0] {
        Message::User(m) => assert_eq!(m.content, "Local deploy"),
        _ => panic!("expected user message"),
    }
}
