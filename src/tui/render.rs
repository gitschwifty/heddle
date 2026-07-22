use ratatui::layout::{Constraint, Direction, Layout, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use super::{
    abbreviate, display_model, divider_line, format_cost, wrap_message_lines, InputBuffer,
    PermissionPromptView, TranscriptKind, TuiApp,
};

pub(super) fn draw(frame: &mut Frame, app: &mut TuiApp) {
    app.refresh_pending_work();

    let area = frame.area();
    let input_height = if app.permission_prompt_view.is_some() {
        8
    } else {
        app.input
            .visual_height(area.width.saturating_sub(2))
            .saturating_add(2)
            .clamp(3, 10)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(area);

    let transcript_lines = transcript_text(app, chunks[0].width);
    app.viewport
        .set_content_height(transcript_lines.lines.len());
    app.viewport.set_viewport_height(chunks[0].height as usize);
    let scroll = app.viewport.scroll_top.min(u16::MAX as usize) as u16;
    let transcript = Paragraph::new(transcript_lines).scroll((scroll, 0));
    frame.render_widget(transcript, chunks[0]);

    frame.render_widget(Clear, chunks[1]);
    if let Some(prompt) = &app.permission_prompt_view {
        let input = Paragraph::new(permission_prompt_text(prompt))
            .block(
                Block::default()
                    .title("Permission required")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(input, chunks[1]);
    } else {
        let input_content_width = chunks[1].width.saturating_sub(2);
        let input_content_height = chunks[1].height.saturating_sub(2);
        let input_scroll = app
            .input
            .input_scroll(input_content_width, input_content_height);
        let input = Paragraph::new(input_text(&app.input, input_content_width, input_scroll))
            .style(
                Style::default()
                    .fg(if app.active {
                        Color::DarkGray
                    } else {
                        Color::White
                    })
                    .bg(Color::Rgb(38, 38, 48)),
            );
        frame.render_widget(input, chunks[1]);
    }
    if !app.active && app.permission_prompt_view.is_none() {
        let input_content_width = chunks[1].width.saturating_sub(2);
        let input_content_height = chunks[1].height.saturating_sub(2);
        let input_scroll = app
            .input
            .input_scroll(input_content_width, input_content_height);
        frame.set_cursor_position(app.input.cursor_position(
            Position::new(chunks[1].x.saturating_add(2), chunks[1].y.saturating_add(1)),
            input_content_width,
            input_content_height,
            input_scroll,
        ));
    }

    frame.render_widget(Clear, chunks[2]);
    let status = Paragraph::new(status_line(app, chunks[2].width));
    frame.render_widget(status, chunks[2]);
}

pub(super) fn input_text(input: &InputBuffer, width: u16, scroll: usize) -> Text<'static> {
    let width = width.max(1) as usize;
    let mut lines = Vec::new();
    lines.push(Line::raw(""));

    let mut visual_row = 0_usize;
    for (idx, logical_line) in input.lines.iter().enumerate() {
        for (chunk_idx, chunk) in visual_chunks(logical_line, width).into_iter().enumerate() {
            let prefix = if idx == 0 && chunk_idx == 0 {
                "› "
            } else {
                "  "
            };
            if visual_row >= scroll {
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Cyan)),
                    Span::raw(chunk),
                ]));
            }
            visual_row += 1;
        }
    }
    lines.push(Line::raw(""));
    Text::from(lines)
}

fn visual_chunks(line: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for c in line.chars() {
        current.push(c);
        if current.chars().count() == width {
            chunks.push(current);
            current = String::new();
        }
    }

    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    } else {
        chunks.push(String::new());
    }

    chunks
}

fn permission_prompt_text(prompt: &PermissionPromptView) -> Text<'static> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Tool: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(prompt.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Call: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(prompt.call_id.clone()),
        ]),
    ];

    if let Some(reason) = &prompt.reason {
        lines.push(Line::from(vec![
            Span::styled("Reason: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(abbreviate(reason, 120)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Args: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(prompt.arguments.clone()),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![Span::styled(
        "Y allow  N deny and continue  A always allow  Esc deny/clear",
        Style::default().fg(Color::Yellow),
    )]));

    Text::from(lines)
}

pub(super) fn transcript_text(app: &TuiApp, width: u16) -> Text<'static> {
    let mut lines = startup_text(app).lines;
    if app.transcript.is_empty() {
        return Text::from(lines);
    }

    for (idx, item) in app.transcript.iter().enumerate() {
        if matches!(item.kind, TranscriptKind::User) {
            lines.push(Line::raw(""));
            lines.extend(user_message_text(&item.text, width));
            continue;
        }

        let (marker, style) = match item.kind {
            TranscriptKind::Assistant => ("- ", Style::default().fg(Color::White)),
            TranscriptKind::Tool => ("- ", Style::default().fg(Color::Yellow)),
            TranscriptKind::Error => ("! ", Style::default().fg(Color::Red)),
            TranscriptKind::System => (". ", Style::default().fg(Color::DarkGray)),
            TranscriptKind::Divider => ("", Style::default().fg(Color::DarkGray)),
            TranscriptKind::User => unreachable!("user transcript rows render as prompt blocks"),
        };

        if matches!(item.kind, TranscriptKind::Divider) {
            lines.push(Line::from(Span::styled(
                divider_line(&item.text, width),
                style,
            )));
            if idx + 1 == app.transcript.len() {
                lines.push(Line::raw(""));
            }
            continue;
        }

        lines.extend(transcript_item_lines(&item.text, marker, style, width));
        lines.push(Line::raw(""));
    }
    Text::from(lines)
}

fn transcript_item_lines(
    text: &str,
    first_prefix: &str,
    style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    let width = width.max(4) as usize;
    let indent = " ".repeat(first_prefix.chars().count());
    let text_width = width.saturating_sub(first_prefix.chars().count()).max(1);
    let display_text = text.trim_matches('\n');
    let mut display_lines = wrap_message_lines(display_text, text_width);
    if display_lines.is_empty() {
        display_lines.push(String::new());
    }

    display_lines
        .into_iter()
        .enumerate()
        .map(|(idx, line)| {
            let prefix = if idx == 0 {
                first_prefix.to_string()
            } else {
                indent.clone()
            };
            Line::from(vec![Span::styled(prefix, style), Span::raw(line)])
        })
        .collect()
}

fn user_message_text(message: &str, width: u16) -> Vec<Line<'static>> {
    let width = width.max(4) as usize;
    let render_width = width.saturating_sub(2).max(4);
    let style = Style::default().fg(Color::White).bg(Color::Rgb(38, 38, 48));
    let mut lines = vec![Line::from(Span::styled(blank_fill(render_width), style))];
    for (idx, line) in wrap_message_lines(message, render_width.saturating_sub(2))
        .into_iter()
        .enumerate()
    {
        let prefix = if idx == 0 { "› " } else { "  " };
        let content_width = render_width.saturating_sub(prefix.chars().count());
        let content = abbreviate(&line, content_width);
        let padding = content_width.saturating_sub(content.chars().count());
        lines.push(Line::from(Span::styled(
            format!("{prefix}{content}{}", blank_fill(padding)),
            style,
        )));
    }
    lines.push(Line::from(Span::styled(blank_fill(render_width), style)));
    lines.push(Line::raw(""));
    lines
}

pub(super) fn blank_fill(width: usize) -> String {
    "\u{00a0}".repeat(width)
}

fn startup_text(app: &TuiApp) -> Text<'static> {
    const CARD_WIDTH: usize = 46;
    const CONTENT_WIDTH: usize = CARD_WIDTH - 4;
    const INDENT: &str = "     ";

    let model = app
        .status
        .as_ref()
        .map(|status| status.model.as_str())
        .unwrap_or("initializing");
    let title = format!("Heddle v{}", env!("CARGO_PKG_VERSION"));
    Text::from(vec![
        Line::raw(""),
        Line::raw(format!("{INDENT}+{}+", "-".repeat(CARD_WIDTH))),
        Line::from(vec![
            Span::raw(format!("{INDENT}|  ")),
            Span::styled(
                abbreviate(&title, CONTENT_WIDTH),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "{:<padding$}  |",
                "",
                padding = CONTENT_WIDTH.saturating_sub(title.chars().count())
            )),
        ]),
        Line::raw(format!("{INDENT}|{}|", " ".repeat(CARD_WIDTH))),
        Line::raw(format!(
            "{INDENT}|  {:<label_width$}{:<value_width$}|",
            "model:",
            abbreviate(model, 33),
            label_width = 11,
            value_width = 33
        )),
        Line::raw(format!(
            "{INDENT}|  {:<label_width$}{:<value_width$}|",
            "directory:",
            abbreviate(&app.cwd, 33),
            label_width = 11,
            value_width = 33
        )),
        Line::raw(format!("{INDENT}+{}+", "-".repeat(CARD_WIDTH))),
    ])
}

pub(super) fn status_line(app: &TuiApp, width: u16) -> String {
    abbreviate(&full_status_line(app), width as usize)
}

fn full_status_line(app: &TuiApp) -> String {
    let Some(status) = &app.status else {
        return "initializing runtime".to_string();
    };

    let cost = status.cost_usd.map(format_cost).unwrap_or_default();
    let tool_count = visible_tool_count(app);
    format!(
        "model: {} | msgs: {} | tools: {} | tokens: {}/{}{}",
        display_model(status),
        status.messages_count,
        tool_count,
        status.total_input_tokens,
        status.total_output_tokens,
        cost,
    )
}

fn visible_tool_count(app: &TuiApp) -> usize {
    app.transcript
        .iter()
        .filter(|item| matches!(item.kind, TranscriptKind::Tool))
        .count()
}
