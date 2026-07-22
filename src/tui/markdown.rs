use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::wrap_message_lines;

pub(super) fn assistant_markdown_lines(
    text: &str,
    first_prefix: &str,
    style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    let width = width.max(4) as usize;
    let indent = " ".repeat(first_prefix.chars().count());
    let text_width = width.saturating_sub(first_prefix.chars().count()).max(1);
    let mut lines = Vec::new();
    let mut first = true;
    let mut in_code = false;

    for raw_line in text.trim_matches('\n').lines() {
        let line = raw_line.trim_end();

        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }

        if line.trim().is_empty() {
            lines.push(Line::raw(""));
            continue;
        }

        if in_code {
            push_plain_wrapped(
                &mut lines,
                &mut first,
                first_prefix,
                &indent,
                line,
                text_width,
                Style::default().fg(Color::White),
            );
            continue;
        }

        if is_table_separator(line) {
            continue;
        }

        if let Some(cells) = table_cells(line) {
            push_inline_line(
                &mut lines,
                &mut first,
                first_prefix,
                &indent,
                table_spans(&cells, style),
            );
            continue;
        }

        if let Some(quote) = blockquote(line) {
            let quote_style = style.fg(Color::Gray).add_modifier(Modifier::ITALIC);
            let mut spans = vec![Span::styled(
                "| ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::ITALIC),
            )];
            spans.extend(inline_spans(quote, quote_style));
            push_inline_line(&mut lines, &mut first, first_prefix, &indent, spans);
            continue;
        }

        if let Some(heading) = markdown_heading(line) {
            push_inline_line(
                &mut lines,
                &mut first,
                first_prefix,
                &indent,
                inline_spans(
                    heading,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            );
            continue;
        }

        let rendered = markdown_bullet(line).unwrap_or(line);
        if has_inline_markdown(rendered) {
            push_inline_line(
                &mut lines,
                &mut first,
                first_prefix,
                &indent,
                inline_spans(rendered, style),
            );
        } else {
            push_plain_wrapped(
                &mut lines,
                &mut first,
                first_prefix,
                &indent,
                rendered,
                text_width,
                style,
            );
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            first_prefix.to_string(),
            style,
        )]));
    }
    lines
}

fn markdown_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
        Some(trimmed[hashes + 1..].trim())
    } else {
        None
    }
}

fn markdown_bullet(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        Some(trimmed)
    } else {
        None
    }
}

fn blockquote(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix('>').map(str::trim_start)
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
        return None;
    }
    let cells = trimmed
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    if cells.len() >= 2 {
        Some(cells)
    } else {
        None
    }
}

fn is_table_separator(line: &str) -> bool {
    let Some(cells) = table_cells(line) else {
        return false;
    };
    cells.iter().all(|cell| {
        let trimmed = cell.trim();
        trimmed.len() >= 3
            && trimmed.chars().all(|c| matches!(c, '-' | ':' | ' '))
            && trimmed.chars().any(|c| c == '-')
    })
}

fn has_inline_markdown(line: &str) -> bool {
    line.contains('`')
        || line.contains("**")
        || line.contains('*')
        || (line.contains('[') && line.contains("]("))
}

fn inline_spans(line: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("**") {
            if let Some(end) = stripped.find("**") {
                spans.push(Span::styled(
                    stripped[..end].to_string(),
                    base.add_modifier(Modifier::BOLD),
                ));
                rest = &stripped[end + 2..];
                continue;
            }
        }

        if let Some(stripped) = rest.strip_prefix('*') {
            if let Some(end) = stripped.find('*') {
                spans.push(Span::styled(
                    stripped[..end].to_string(),
                    base.add_modifier(Modifier::ITALIC),
                ));
                rest = &stripped[end + 1..];
                continue;
            }
        }

        if let Some(stripped) = rest.strip_prefix('`') {
            if let Some(end) = stripped.find('`') {
                spans.push(Span::styled(
                    stripped[..end].to_string(),
                    Style::default().fg(Color::Cyan),
                ));
                rest = &stripped[end + 1..];
                continue;
            }
        }

        if let Some((label, url, tail)) = parse_link(rest) {
            spans.push(Span::styled(
                label.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            spans.push(Span::styled(
                format!(" <{url}>"),
                Style::default().fg(Color::DarkGray),
            ));
            rest = tail;
            continue;
        }

        let next = next_marker(rest).unwrap_or(rest.len());
        let (plain, tail) = rest.split_at(next.max(1));
        spans.push(Span::styled(plain.to_string(), base));
        rest = tail;
    }
    spans
}

fn table_spans(cells: &[String], base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (idx, cell) in cells.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled("  ", Style::default().fg(Color::DarkGray)));
        }
        spans.extend(inline_spans(cell, base));
    }
    spans
}

fn parse_link(input: &str) -> Option<(&str, &str, &str)> {
    let label_start = input.strip_prefix('[')?;
    let label_end = label_start.find("](")?;
    let url_start = &label_start[label_end + 2..];
    let url_end = url_start.find(')')?;
    Some((
        &label_start[..label_end],
        &url_start[..url_end],
        &url_start[url_end + 1..],
    ))
}

fn next_marker(input: &str) -> Option<usize> {
    ["**", "*", "`", "["]
        .iter()
        .filter_map(|marker| input.find(marker))
        .min()
}

fn push_plain_wrapped(
    lines: &mut Vec<Line<'static>>,
    first: &mut bool,
    first_prefix: &str,
    indent: &str,
    text: &str,
    width: usize,
    style: Style,
) {
    let mut wrapped = wrap_message_lines(text, width);
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    for chunk in wrapped {
        push_inline_line(
            lines,
            first,
            first_prefix,
            indent,
            vec![Span::styled(chunk, style)],
        );
    }
}

fn push_inline_line(
    lines: &mut Vec<Line<'static>>,
    first: &mut bool,
    first_prefix: &str,
    indent: &str,
    spans: Vec<Span<'static>>,
) {
    let prefix = if *first {
        *first = false;
        first_prefix.to_string()
    } else {
        indent.to_string()
    };
    let mut line_spans = Vec::with_capacity(spans.len() + 1);
    line_spans.push(Span::styled(prefix, Style::default()));
    line_spans.extend(spans);
    lines.push(Line::from(line_spans));
}
