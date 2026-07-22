use crate::runtime::RuntimeStatus;

use super::display_model;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlashCommand {
    Clear,
    Status,
    Help,
    Quit,
    Unknown(String),
}

pub(super) fn parse_tui_slash_command(input: &str) -> SlashCommand {
    let token = input.split_whitespace().next().unwrap_or(input.trim());
    match token {
        "/clear" => SlashCommand::Clear,
        "/status" => SlashCommand::Status,
        "/help" => SlashCommand::Help,
        "/quit" | "/exit" => SlashCommand::Quit,
        other => SlashCommand::Unknown(other.to_string()),
    }
}

pub(super) fn tui_help_text() -> String {
    [
        "TUI commands:",
        "/help - show TUI commands and keybindings",
        "/status - show session, model, message, token, and cost status",
        "/clear - clear conversation context and transcript view",
        "/quit, /exit - exit the TUI",
        "",
        "Keybindings:",
        "Enter submit | Shift-Enter newline | Esc interrupt/exit | Ctrl-C exit",
        "PageUp/PageDown scroll | Ctrl-End follow tail",
    ]
    .join("\n")
}

pub(super) fn tui_status_text(status: Option<&RuntimeStatus>) -> String {
    let Some(status) = status else {
        return "runtime status unavailable: initializing".to_string();
    };
    let cost = status
        .cost_usd
        .map(|cost| format!("${cost:.4}"))
        .unwrap_or_else(|| "n/a".to_string());
    format!(
        "session: {}\nmodel: {}\nmessages: {}\ntokens: {} in / {} out\ncost: {}",
        status.session_id,
        display_model(status),
        status.messages_count,
        status.total_input_tokens,
        status.total_output_tokens,
        cost
    )
}
