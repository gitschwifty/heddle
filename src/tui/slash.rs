use crate::runtime::RuntimeStatus;

use super::display_model;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlashCommand {
    Clear,
    Status,
    Help,
    CopyLast,
    CopyTurn,
    ExportTranscript,
    Quit,
    Unknown(String),
}

pub(super) fn parse_tui_slash_command(input: &str) -> SlashCommand {
    let mut tokens = input.split_whitespace();
    let token = tokens.next().unwrap_or(input.trim());
    match (token, tokens.next()) {
        ("/copy", Some("last")) => SlashCommand::CopyLast,
        ("/copy", Some("turn")) => SlashCommand::CopyTurn,
        ("/export", Some("transcript")) => SlashCommand::ExportTranscript,
        ("/clear", _) => SlashCommand::Clear,
        ("/status", _) => SlashCommand::Status,
        ("/help", _) => SlashCommand::Help,
        ("/quit" | "/exit", _) => SlashCommand::Quit,
        (other, _) => SlashCommand::Unknown(other.to_string()),
    }
}

pub(super) fn tui_help_text() -> String {
    [
        "TUI commands:",
        "/help - show TUI commands and keybindings",
        "/status - show session, model, message, token, and cost status",
        "/clear - clear conversation context and transcript view",
        "/copy last - write the last assistant response to heddle-copy.md",
        "/copy turn - write the current turn to heddle-copy.md",
        "/export transcript - write the full transcript to heddle-transcript.md",
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
