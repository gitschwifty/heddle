use crate::runtime::RuntimeStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TranscriptKind {
    User,
    Assistant,
    Tool,
    Error,
    System,
    Divider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptItem {
    pub(super) kind: TranscriptKind,
    pub(super) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptTurn {
    pub(super) items: Vec<TurnTranscriptItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TurnTranscriptItem {
    Row(TranscriptItem),
    Tool(ToolTranscript),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolTranscript {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: String,
    pub(super) result: Option<String>,
    pub(super) state: ToolState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolState {
    Running,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptLocation {
    pub(super) turn: usize,
    pub(super) item: usize,
}

pub(super) fn wrap_message_lines(message: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for line in message.split('\n') {
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            lines.push(String::new());
            continue;
        }
        for chunk in chars.chunks(width) {
            lines.push(chunk.iter().collect());
        }
    }
    lines
}

pub(super) fn divider_line(label: &str, width: u16) -> String {
    let width = width.max(12) as usize;
    let label = if label.is_empty() {
        String::new()
    } else {
        format!("- {label} ")
    };
    if label.chars().count() >= width {
        return abbreviate(&label, width);
    }
    let remaining = width - label.chars().count();
    format!("{label}{}", "-".repeat(remaining))
}

pub(super) fn display_model(status: &RuntimeStatus) -> String {
    match status.last_routed_model.as_deref() {
        Some(routed) if routed != status.model => format!("{}:{routed}", status.model),
        _ => status.model.clone(),
    }
}

pub(super) fn flatten_transcript_turns(turns: &[TranscriptTurn]) -> Vec<TranscriptItem> {
    let mut rows = Vec::new();
    for turn in turns {
        let mut idx = 0;
        while idx < turn.items.len() {
            match &turn.items[idx] {
                TurnTranscriptItem::Row(item) => {
                    rows.push(item.clone());
                    idx += 1;
                }
                TurnTranscriptItem::Tool(tool) if is_exploration_tool(&tool.name) => {
                    let mut group = Vec::new();
                    while idx < turn.items.len() {
                        match &turn.items[idx] {
                            TurnTranscriptItem::Tool(tool) if is_exploration_tool(&tool.name) => {
                                group.push(exploration_tool_line(tool));
                                idx += 1;
                            }
                            _ => break,
                        }
                    }
                    rows.push(TranscriptItem {
                        kind: TranscriptKind::Tool,
                        text: format!("Explored\n{}", group.join("\n")),
                    });
                }
                TurnTranscriptItem::Tool(tool) => {
                    rows.push(TranscriptItem {
                        kind: TranscriptKind::Tool,
                        text: action_tool_row(tool),
                    });
                    idx += 1;
                }
            }
        }
    }
    rows
}

pub(super) fn turn_logical_text(turn: &TranscriptTurn) -> String {
    let mut out = Vec::new();
    for item in &turn.items {
        match item {
            TurnTranscriptItem::Row(row) => match row.kind {
                TranscriptKind::User => out.push(format!("## User\n\n{}", row.text.trim())),
                TranscriptKind::Assistant => {
                    out.push(format!("## Assistant\n\n{}", row.text.trim()))
                }
                TranscriptKind::Error => out.push(format!("## Error\n\n{}", row.text.trim())),
                TranscriptKind::System | TranscriptKind::Divider => {}
                TranscriptKind::Tool => out.push(row.text.trim().to_string()),
            },
            TurnTranscriptItem::Tool(tool) => {
                out.push(format!(
                    "## Tool: {}\n\n{}",
                    tool.name,
                    action_tool_row(tool)
                ));
            }
        }
    }
    out.into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn logical_transcript_markdown(turns: &[TranscriptTurn]) -> String {
    turns
        .iter()
        .enumerate()
        .filter_map(|(idx, turn)| {
            let text = turn_logical_text(turn);
            if text.is_empty() {
                None
            } else {
                Some(format!("# Turn {}\n\n{text}", idx + 1))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn is_exploration_tool(name: &str) -> bool {
    matches!(name, "read_file" | "grep" | "glob")
}

fn exploration_tool_line(tool: &ToolTranscript) -> String {
    let args_value = serde_json::from_str::<serde_json::Value>(&tool.arguments).ok();
    let summary = match (tool.name.as_str(), args_value.as_ref()) {
        ("read_file", Some(args)) => {
            let path = json_str(args, &["file_path", "path"]).unwrap_or("?");
            format!("Read {path}")
        }
        ("grep", Some(args)) => {
            let pattern = json_str(args, &["pattern"]).unwrap_or("?");
            let path = json_str(args, &["path"]).unwrap_or(".");
            format!("Search {pattern:?} in {path}")
        }
        ("glob", Some(args)) => {
            let pattern = json_str(args, &["pattern"]).unwrap_or("?");
            let path = json_str(args, &["path"]).unwrap_or(".");
            format!("Explore {pattern} in {path}")
        }
        _ => format!("{} {}", tool.name, summarize_arguments(&tool.arguments, 80)),
    };

    match (&tool.state, tool.result.as_deref()) {
        (ToolState::Running, _) => format!("{summary} running"),
        (ToolState::Finished, Some(result)) if is_error_result(result) => {
            format!("{summary} error: {}", abbreviate(result.trim(), 120))
        }
        (ToolState::Finished, _) => summary,
    }
}

fn action_tool_row(tool: &ToolTranscript) -> String {
    let state = match tool.state {
        ToolState::Running => "running",
        ToolState::Finished => "finished",
    };
    format_tool_row(
        &tool.name,
        state,
        Some(&tool.arguments),
        tool.result.as_deref(),
    )
}

pub(super) fn format_cost(cost: f64) -> String {
    if cost == 0.0 {
        return " | $0.0000".to_string();
    }
    if cost.abs() < 0.0001 {
        return format!(" | ${cost:.6}");
    }
    format!(" | ${cost:.4}")
}

pub(super) fn summarize_arguments(arguments: &str, max_chars: usize) -> String {
    let compact = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_else(|| arguments.to_string());
    abbreviate(&compact, max_chars)
}

fn format_tool_row(
    name: &str,
    state: &str,
    arguments: Option<&str>,
    result: Option<&str>,
) -> String {
    let args_value =
        arguments.and_then(|args| serde_json::from_str::<serde_json::Value>(args).ok());
    let summary = match (name, args_value.as_ref()) {
        ("read_file", Some(args)) => {
            let path = json_str(args, &["file_path", "path"]).unwrap_or("?");
            format!("Read {path}")
        }
        ("grep", Some(args)) => {
            let pattern = json_str(args, &["pattern"]).unwrap_or("?");
            let path = json_str(args, &["path"]).unwrap_or(".");
            format!("Search {pattern:?} in {path}")
        }
        ("glob", Some(args)) => {
            let pattern = json_str(args, &["pattern"]).unwrap_or("?");
            let path = json_str(args, &["path"]).unwrap_or(".");
            format!("Explore {pattern} in {path}")
        }
        ("bash", Some(args)) => {
            let command = json_str(args, &["command"]).unwrap_or("?");
            format!("Run {command}")
        }
        ("edit_file", Some(args)) => {
            let path = json_str(args, &["file_path", "path"]).unwrap_or("?");
            format!("Edit {path}")
        }
        ("write_file", Some(args)) => {
            let path = json_str(args, &["file_path", "path"]).unwrap_or("?");
            format!("Write {path}")
        }
        _ => {
            let args = arguments
                .map(|args| format!(" {}", summarize_arguments(args, 120)))
                .unwrap_or_default();
            format!("{name}{args}")
        }
    };

    let result = match name {
        "read_file" | "grep" | "glob" if !result.is_some_and(is_error_result) => String::new(),
        _ => result
            .map(|result| format!(" - {}", abbreviate(result.trim(), 160)))
            .unwrap_or_default(),
    };

    format!("{summary} {state}{result}")
}

fn json_str<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn is_error_result(result: &str) -> bool {
    result.trim_start().starts_with("Error:")
}

pub(super) fn abbreviate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", value.chars().take(keep).collect::<String>())
}
