use heddle::commands::registry::CommandRegistry;
use heddle::commands::types::SlashCommand;
use std::sync::Arc;

fn make_command(name: &str, description: &str) -> SlashCommand {
    SlashCommand {
        name: name.to_string(),
        description: description.to_string(),
        execute: Arc::new(|_args, _ctx| Box::pin(async move { None })),
    }
}

#[test]
fn register_and_get_a_command() {
    let mut reg = CommandRegistry::new();
    reg.register(make_command("help", "Test: help"));
    assert!(reg.get("help").is_some());
    assert_eq!(reg.get("help").unwrap().name, "help");
}

#[test]
fn get_returns_none_for_unknown_command() {
    let reg = CommandRegistry::new();
    assert!(reg.get("nope").is_none());
}

#[test]
fn all_returns_all_registered_commands() {
    let mut reg = CommandRegistry::new();
    reg.register(make_command("help", "h"));
    reg.register(make_command("exit", "e"));
    reg.register(make_command("cost", "c"));
    let all = reg.all();
    assert_eq!(all.len(), 3);
    let mut names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["cost", "exit", "help"]);
}

#[test]
fn suggest_returns_closest_match_for_typo() {
    let mut reg = CommandRegistry::new();
    reg.register(make_command("help", "h"));
    reg.register(make_command("exit", "e"));
    reg.register(make_command("status", "s"));
    assert_eq!(reg.suggest("halp").as_deref(), Some("help"));
    assert_eq!(reg.suggest("staus").as_deref(), Some("status"));
}

#[test]
fn suggest_returns_none_when_no_close_match() {
    let mut reg = CommandRegistry::new();
    reg.register(make_command("help", "h"));
    assert!(reg.suggest("zzzzzzzzz").is_none());
}

#[test]
fn later_registration_overrides_earlier() {
    let mut reg = CommandRegistry::new();
    reg.register(make_command("deploy", "Global deploy"));
    reg.register(make_command("deploy", "Local deploy"));
    let result = reg.get("deploy").unwrap();
    assert_eq!(result.description, "Local deploy");
}

#[test]
fn override_does_not_duplicate_in_all() {
    let mut reg = CommandRegistry::new();
    reg.register(make_command("deploy", "Global"));
    reg.register(make_command("deploy", "Local"));
    assert_eq!(reg.all().len(), 1);
    assert_eq!(reg.all()[0].description, "Local");
}
