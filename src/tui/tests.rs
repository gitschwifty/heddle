use super::*;
use crate::runtime::{RuntimeError, RuntimeStatus, RuntimeUsage};
use crate::types::{FunctionCall, ToolCall, ToolCallKind};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Terminal;

fn runtime_status(
    active: bool,
    messages_count: u64,
    total_input_tokens: u64,
    total_output_tokens: u64,
    cost_usd: Option<f64>,
) -> RuntimeStatus {
    RuntimeStatus {
        session_id: "session".to_string(),
        model: "anthropic/claude-sonnet-4".to_string(),
        last_routed_model: None,
        messages_count,
        active,
        total_input_tokens,
        total_output_tokens,
        cost_usd,
    }
}

fn tool_call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: name.to_string(),
            arguments: "{}".to_string(),
        },
    }
}

fn tool_call_with_args(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

fn add_long_transcript(app: &mut TuiApp, rows: usize) {
    for idx in 0..rows {
        app.transcript.push(TranscriptItem {
            kind: TranscriptKind::Assistant,
            text: format!("transcript row {idx:03}"),
        });
    }
}

#[derive(Debug, Clone)]
struct RenderedScreen {
    width: u16,
    height: u16,
    lines: Vec<String>,
    content: String,
}

impl RenderedScreen {
    fn contains(&self, needle: &str) -> bool {
        self.content.contains(needle)
    }

    fn find(&self, needle: &str) -> Option<usize> {
        self.content.find(needle)
    }

    fn last_line(&self) -> &str {
        self.lines.last().map(String::as_str).unwrap_or("")
    }
}

fn render_app(width: u16, height: u16, app: &mut TuiApp) -> RenderedScreen {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    draw_screen(&mut terminal, app)
}

fn draw_screen(terminal: &mut Terminal<TestBackend>, app: &mut TuiApp) -> RenderedScreen {
    terminal.draw(|frame| draw(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let width = area.width as usize;
    let lines = buffer
        .content()
        .chunks(width.max(1))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let content = lines.join("\n");
    RenderedScreen {
        width: area.width,
        height: area.height,
        lines,
        content,
    }
}

fn insert_input(input: &mut InputBuffer, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            input.insert_newline();
        } else {
            input.insert_char(c);
        }
    }
}

fn insert_prompt(app: &mut TuiApp, prompt: &str) {
    for c in prompt.chars() {
        app.input.insert_char(c);
    }
}

fn ok_outcome() -> TurnOutcome {
    TurnOutcome {
        status: TurnStatus::Ok,
        response: None,
        tool_calls_made: Vec::new(),
        usage: None,
        iterations: 0,
        error: None,
        model_latency_ms: 0,
        tool_latency_ms: 0,
        total_latency_ms: 0,
    }
}

fn error_outcome(message: &str) -> TurnOutcome {
    TurnOutcome {
        status: TurnStatus::Error,
        response: None,
        tool_calls_made: Vec::new(),
        usage: None,
        iterations: 0,
        error: Some(RuntimeError {
            code: "provider_error".to_string(),
            message: message.to_string(),
            retryable: false,
            provider: None,
            details: None,
        }),
        model_latency_ms: 0,
        tool_latency_ms: 0,
        total_latency_ms: 0,
    }
}

#[test]
fn viewport_follows_tail_when_content_or_viewport_changes() {
    let mut viewport = ViewportState::default();

    viewport.set_viewport_height(10);
    viewport.set_content_height(100);
    assert_eq!(viewport.scroll_top, 90);
    assert!(viewport.follow_tail);

    viewport.set_viewport_height(20);
    assert_eq!(viewport.scroll_top, 80);

    viewport.set_content_height(12);
    assert_eq!(viewport.scroll_top, 0);
    assert!(viewport.follow_tail);
}

#[test]
fn viewport_manual_scroll_survives_output_and_clamps_on_resize() {
    let mut viewport = ViewportState::default();
    viewport.set_viewport_height(10);
    viewport.set_content_height(100);

    viewport.scroll_up(30);
    assert_eq!(viewport.scroll_top, 60);
    assert!(!viewport.follow_tail);

    viewport.set_content_height(120);
    viewport.on_new_output();
    assert_eq!(viewport.scroll_top, 60);
    assert!(!viewport.follow_tail);

    viewport.set_viewport_height(80);
    assert_eq!(viewport.scroll_top, 40);
    assert!(!viewport.follow_tail);

    viewport.jump_to_bottom();
    assert_eq!(viewport.scroll_top, 40);
    assert!(viewport.follow_tail);
}

#[test]
fn viewport_scroll_down_reaches_live_tail() {
    let mut viewport = ViewportState::default();
    viewport.set_viewport_height(10);
    viewport.set_content_height(100);
    viewport.scroll_up(50);

    viewport.scroll_down(10);
    assert_eq!(viewport.scroll_top, 50);
    assert!(!viewport.follow_tail);

    viewport.scroll_down(100);
    assert_eq!(viewport.scroll_top, 90);
    assert!(viewport.follow_tail);

    viewport.set_content_height(120);
    assert_eq!(viewport.scroll_top, 110);
    assert!(viewport.follow_tail);
}

#[test]
fn viewport_submit_prompt_returns_to_tail() {
    let mut viewport = ViewportState::default();
    viewport.set_viewport_height(10);
    viewport.set_content_height(100);
    viewport.scroll_up(40);
    assert!(!viewport.follow_tail);

    viewport.on_submit_prompt();
    assert_eq!(viewport.scroll_top, 90);
    assert!(viewport.follow_tail);
}

#[test]
fn content_delta_appends_to_latest_assistant_row() {
    let mut app = TuiApp::new();
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: String::new(),
    });
    app.active_assistant = Some(0);

    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "hello".to_string(),
    });
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: " world".to_string(),
    });

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].text, "hello world");
}

#[test]
fn content_delta_uses_active_assistant_row_not_latest_assistant() {
    let mut app = TuiApp::new();
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: "first".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::User,
        text: "next".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: String::new(),
    });
    app.active_assistant = Some(2);

    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "second".to_string(),
    });

    assert_eq!(app.transcript[0].text, "first");
    assert_eq!(app.transcript[2].text, "second");
    assert!(app.viewport.follow_tail);
}

#[test]
fn tool_events_collapse_started_row_to_finished_row() {
    let mut app = TuiApp::new();
    let call = tool_call("call-1", "read_file");

    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "read_file".to_string(),
        call: call.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "read_file".to_string(),
        result: "ok".to_string(),
        call,
    });

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::Tool);
    assert_eq!(app.transcript[0].text, "Explored\nRead ?");
}

#[test]
fn empty_assistant_delta_before_tool_does_not_render_blank_assistant_row() {
    let mut app = TuiApp::new();
    let call = tool_call("call-1", "read_file");

    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: String::new(),
    });
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "\n".to_string(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "read_file".to_string(),
        call: call.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "read_file".to_string(),
        result: "ok".to_string(),
        call,
    });

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::Tool);
    assert_eq!(app.transcript[0].text, "Explored\nRead ?");
}

#[tokio::test]
async fn pending_work_row_stays_at_turn_tail_until_outcome() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;
    let call = tool_call("call-1", "read_file");

    insert_prompt(&mut app, "inspect files");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("submit");
    let _ = command_rx.try_recv().expect("command");

    assert_eq!(
        app.transcript.last().expect("pending").kind,
        TranscriptKind::System
    );
    assert!(app
        .transcript
        .last()
        .expect("pending")
        .text
        .starts_with("Working..."));

    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "read_file".to_string(),
        call: call.clone(),
    });
    assert_eq!(app.transcript[1].kind, TranscriptKind::Tool);
    assert_eq!(
        app.transcript.last().expect("pending").kind,
        TranscriptKind::System
    );
    assert!(app
        .transcript
        .last()
        .expect("pending")
        .text
        .starts_with("Working..."));

    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "read_file".to_string(),
        result: "ok".to_string(),
        call,
    });
    assert_eq!(app.transcript[1].kind, TranscriptKind::Tool);
    assert_eq!(
        app.transcript.last().expect("pending").kind,
        TranscriptKind::System
    );
    assert!(app
        .transcript
        .last()
        .expect("pending")
        .text
        .starts_with("Working..."));

    app.apply_turn_outcome(ok_outcome());

    assert_eq!(
        app.transcript.last().expect("footer").kind,
        TranscriptKind::Divider
    );
    assert!(app
        .transcript
        .last()
        .expect("footer")
        .text
        .starts_with("Worked for "));
}

#[tokio::test]
async fn stream_error_stays_in_active_turn_until_error_outcome() {
    let (command_tx, mut command_rx) = mpsc::channel(2);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;

    insert_prompt(&mut app, "first prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("first submit");
    let _ = command_rx.try_recv().expect("first command");

    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "partial answer".to_string(),
    });
    assert_eq!(app.transcript.len(), 2);
    assert_eq!(app.transcript[1].kind, TranscriptKind::Assistant);
    assert_eq!(app.transcript[1].text, "partial answer");

    app.apply_runtime_event(RuntimeEvent::Error {
        error: RuntimeError {
            code: "provider_error".to_string(),
            message: "stream body broke".to_string(),
            retryable: false,
            provider: Some("openrouter".to_string()),
            details: None,
        },
    });

    assert_eq!(app.transcript[0].kind, TranscriptKind::User);
    assert_eq!(app.transcript[1].kind, TranscriptKind::Assistant);
    assert_eq!(app.transcript[1].text, "partial answer");
    assert_eq!(app.transcript[2].kind, TranscriptKind::Error);
    assert_eq!(app.transcript[2].text, "stream body broke");
    assert_eq!(app.transcript[3].kind, TranscriptKind::System);
    assert!(app.transcript[3].text.starts_with("Working..."));
    assert!(app.active);

    insert_prompt(&mut app, "second prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("active submit ignored");
    assert_eq!(turn_counter, 1);
    assert!(command_rx.try_recv().is_err());

    app.apply_turn_outcome(error_outcome("stream body broke"));

    assert_eq!(app.transcript.len(), 4);
    assert_eq!(app.transcript[2].kind, TranscriptKind::Error);
    assert_eq!(app.transcript[2].text, "stream body broke");
    assert_eq!(app.transcript[3].kind, TranscriptKind::Divider);
    assert!(app.transcript[3].text.ends_with(" - error"));
    assert!(!app.active);

    app.input.clear();
    insert_prompt(&mut app, "next prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("second submit");
    let _ = command_rx.try_recv().expect("second command");

    assert_eq!(app.transcript[4].kind, TranscriptKind::User);
    assert_eq!(app.transcript[4].text, "next prompt");
}

#[tokio::test]
async fn transcript_groups_exploration_tools_inside_their_turns() {
    let backend = TestBackend::new(96, 28);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let (command_tx, mut command_rx) = mpsc::channel(2);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;

    insert_prompt(&mut app, "first prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("first submit");
    let _ = command_rx.try_recv().expect("first command");
    let read = tool_call_with_args("read-1", "read_file", r#"{"file_path":"src/tui.rs"}"#);
    let grep = tool_call_with_args(
        "grep-1",
        "grep",
        r#"{"pattern":"RuntimeEvent","path":"src"}"#,
    );
    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "read_file".to_string(),
        call: read.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "read_file".to_string(),
        result: "contents".to_string(),
        call: read,
    });
    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "grep".to_string(),
        call: grep.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "grep".to_string(),
        result: "matches".to_string(),
        call: grep,
    });
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "first answer".to_string(),
    });
    app.apply_turn_outcome(ok_outcome());

    insert_prompt(&mut app, "second prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("second submit");
    let _ = command_rx.try_recv().expect("second command");
    let glob = tool_call_with_args("glob-1", "glob", r#"{"pattern":"*.rs","path":"src"}"#);
    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "glob".to_string(),
        call: glob.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "glob".to_string(),
        result: "src/tui.rs".to_string(),
        call: glob,
    });
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "second answer".to_string(),
    });

    assert_eq!(app.transcript[0].kind, TranscriptKind::User);
    assert_eq!(app.transcript[1].kind, TranscriptKind::Tool);
    assert!(app.transcript[1].text.contains("Explored\nRead src/tui.rs"));
    assert!(app.transcript[1]
        .text
        .contains("Search \"RuntimeEvent\" in src"));
    assert_eq!(app.transcript[2].text, "first answer");
    assert_eq!(app.transcript[4].kind, TranscriptKind::User);
    assert_eq!(app.transcript[5].text, "Explored\nExplore *.rs in src");
    assert_eq!(app.transcript[6].text, "second answer");

    let screen = draw_screen(&mut terminal, &mut app);
    assert!(screen.contains("Explored"));
    assert!(screen.contains("Read src/tui.rs"));
    assert!(screen.contains("Search \"RuntimeEvent\" in src"));
    assert!(screen.contains("second answer"));
}

#[test]
fn transcript_keeps_interleaved_tools_and_assistant_content_ordered() {
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    let read = tool_call_with_args("read-1", "read_file", r#"{"file_path":"src/tui.rs"}"#);
    let bash = tool_call_with_args("bash-1", "bash", r#"{"command":"cargo test tui::tests"}"#);

    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "read_file".to_string(),
        call: read.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "read_file".to_string(),
        result: "contents".to_string(),
        call: read,
    });
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "after read".to_string(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolStarted {
        name: "bash".to_string(),
        call: bash.clone(),
    });
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "bash".to_string(),
        result: "tests passed".to_string(),
        call: bash,
    });
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "after bash".to_string(),
    });

    assert_eq!(app.transcript.len(), 4);
    assert_eq!(app.transcript[0].text, "Explored\nRead src/tui.rs");
    assert_eq!(app.transcript[1].text, "after read");
    assert_eq!(app.transcript[2].kind, TranscriptKind::Tool);
    assert!(app.transcript[2]
        .text
        .contains("Run cargo test tui::tests finished - tests passed"));
    assert_eq!(app.transcript[3].text, "after bash");

    let screen = draw_screen(&mut terminal, &mut app);
    let read_pos = screen.find("Read src/tui.rs").expect("read row");
    let first_answer_pos = screen.find("after read").expect("first answer");
    let bash_pos = screen.find("Run cargo test tui::tests").expect("bash row");
    let second_answer_pos = screen.find("after bash").expect("second answer");
    assert!(read_pos < first_answer_pos);
    assert!(first_answer_pos < bash_pos);
    assert!(bash_pos < second_answer_pos);
}

#[test]
fn usage_stays_out_of_transcript_and_errors_become_rows() {
    let mut app = TuiApp::new();

    app.apply_runtime_event(RuntimeEvent::UsageUpdated {
        usage: RuntimeUsage {
            prompt_tokens: 7,
            completion_tokens: 11,
            total_tokens: 18,
            cost_micros: None,
            cost_currency: None,
            cached_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            generation_id: None,
        },
    });
    app.apply_runtime_event(RuntimeEvent::Error {
        error: RuntimeError {
            code: "provider_error".to_string(),
            message: "bad response".to_string(),
            retryable: false,
            provider: None,
            details: None,
        },
    });

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::Error);
    assert_eq!(app.transcript[0].text, "bad response");
}

#[test]
fn input_backslash_then_enter_inserts_newline() {
    let mut input = InputBuffer::default();
    for c in "hello\\".chars() {
        input.insert_char(c);
    }

    assert!(input.consume_trailing_backslash());
    input.insert_newline();
    input.insert_char('w');

    assert_eq!(input.text(), "hello\nw");
    assert_eq!(input.row, 1);
    assert_eq!(input.col, 1);
}

#[test]
fn input_supports_cursor_editing_across_lines() {
    let mut input = InputBuffer::default();
    for c in "abcd".chars() {
        input.insert_char(c);
    }
    input.move_left();
    input.move_left();
    input.insert_newline();
    input.insert_char('X');
    input.backspace();
    input.move_up();
    input.move_line_end();
    input.insert_char('!');

    assert_eq!(input.text(), "ab!\ncd");
}

#[test]
fn input_cursor_position_wraps_at_exact_content_width() {
    let mut input = InputBuffer::default();
    for c in "abc".chars() {
        input.insert_char(c);
    }

    assert_eq!(
        input.cursor_position(Position::new(2, 1), 3, 4, 0),
        Position::new(2, 2)
    );

    input.move_left();
    assert_eq!(
        input.cursor_position(Position::new(2, 1), 3, 4, 0),
        Position::new(4, 1)
    );
}

#[test]
fn input_cursor_position_accounts_for_wrapped_and_explicit_lines() {
    let mut input = InputBuffer::default();
    for c in "abcd".chars() {
        input.insert_char(c);
    }
    input.insert_newline();
    for c in "xy".chars() {
        input.insert_char(c);
    }

    assert_eq!(
        input.cursor_position(Position::new(10, 5), 3, 6, 0),
        Position::new(12, 7)
    );

    input.move_up();
    assert_eq!(
        input.cursor_position(Position::new(10, 5), 3, 6, 0),
        Position::new(12, 5)
    );
}

#[test]
fn input_cursor_position_updates_after_delete_before_wrap_boundary() {
    let mut input = InputBuffer::default();
    for c in "abcd".chars() {
        input.insert_char(c);
    }
    input.move_left();
    input.move_left();
    input.backspace();

    assert_eq!(input.text(), "acd");
    assert_eq!(
        input.cursor_position(Position::new(0, 0), 3, 3, 0),
        Position::new(1, 0)
    );
}

#[test]
fn input_text_renders_wrapped_prompt_band_with_prefix_gutter() {
    let mut input = InputBuffer::default();
    for c in "abcd".chars() {
        input.insert_char(c);
    }
    input.insert_newline();
    for c in "xy".chars() {
        input.insert_char(c);
    }

    let rendered = input_text(&input, 3, 0)
        .lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(rendered, vec!["", "› abc", "  d", "  xy", ""]);
}

#[test]
fn render_prompt_band_scrolls_to_cursor_when_height_is_capped() {
    let backend = TestBackend::new(40, 14);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    app.input.lines = (0..12).map(|idx| format!("prompt-line-{idx:02}")).collect();
    app.input.row = 11;
    app.input.col = app.input.lines[11].chars().count();

    terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(screen.contains("prompt-line-11"));
    assert!(!screen.contains("prompt-line-00"));
}

#[test]
fn pending_work_row_refreshes_with_elapsed_hint() {
    let mut app = TuiApp::new();
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::System,
        text: "Working... 0s - Esc to interrupt".to_string(),
    });
    app.pending_work_row = Some(0);
    app.turn_started_at = Some(Instant::now() - Duration::from_secs(65));

    app.refresh_pending_work();

    assert_eq!(
        app.transcript[0].text,
        "Working... 1m 5s - Esc to interrupt"
    );
}

#[test]
fn turn_outcome_appends_worked_footer_and_divider() {
    let mut app = TuiApp::new();
    app.active = true;
    app.turn_started_at = Some(Instant::now() - Duration::from_secs(3));

    app.apply_turn_outcome(TurnOutcome {
        status: TurnStatus::Ok,
        response: None,
        tool_calls_made: Vec::new(),
        usage: None,
        iterations: 0,
        error: None,
        model_latency_ms: 0,
        tool_latency_ms: 0,
        total_latency_ms: 3000,
    });

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::Divider);
    assert!(app.transcript[0].text.starts_with("Worked for "));
}

#[test]
fn transcript_render_trims_leading_assistant_newline_and_indents_wraps() {
    let mut app = TuiApp::new();
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: "\nabcdef ghijkl\n\n".to_string(),
    });

    let rendered = transcript_text(&app, 10)
        .lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(rendered.iter().any(|line| line == "- abcdef g"));
    assert!(rendered.iter().any(|line| line == "  hijkl"));
    assert!(!rendered.iter().any(|line| line == "- "));
    assert!(!rendered.iter().any(|line| line == "  "));
}

#[test]
fn transcript_render_derives_markdown_lines_without_mutating_raw_text() {
    let mut app = TuiApp::new();
    let raw = "# **Plan**\n\n- inspect the **rendering** path\n- keep *logical* text intact\n\n> quoted `inline` note\n\n```rust\nlet value = 42;\n```\nSee [docs](https://example.invalid/docs).\n\n| Feature | `State` |\n| --- | --- |\n| **Tables** | *Basic* |\n\nA very long paragraph that should wrap visually.";
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: raw.to_string(),
    });

    let rendered_lines = transcript_text(&app, 32).lines;
    let rendered = rendered_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(rendered.iter().any(|line| line.contains("- Plan")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("- inspect the rendering path")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("- keep logical text intact")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("| quoted inline note")));
    assert!(!rendered.iter().any(|line| line.contains("```")));
    assert!(rendered.iter().any(|line| line.contains("let value = 42;")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("docs <https://example.invalid/docs>")));
    assert!(rendered.iter().any(|line| line.contains("Feature  State")));
    assert!(rendered.iter().any(|line| line.contains("Tables  Basic")));
    assert!(rendered.iter().any(|line| line.contains("wrap visually")));
    assert!(!rendered.iter().any(|line| line.contains("**")));
    assert!(!rendered.iter().any(|line| line.contains('`')));

    let inline_span = rendered_lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref() == "inline")
        .expect("inline code should render as a styled span");
    assert_eq!(inline_span.style.fg, Some(Color::Cyan));
    assert_eq!(inline_span.style.bg, None);

    let table_inline_span = rendered_lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref() == "State")
        .expect("table inline code should render as a styled span");
    assert_eq!(table_inline_span.style.fg, Some(Color::Cyan));
    assert_eq!(table_inline_span.style.bg, None);

    assert_eq!(app.transcript[0].text, raw);
}

#[test]
fn transcript_renders_readme_sized_markdown_without_perf_regression() {
    let mut markdown = String::from("# README\n\n");
    for idx in 0..120 {
        markdown.push_str(&format!("## Section {idx}\n\n"));
        markdown.push_str(&format!(
            "- parse **bold {idx}** and *italic {idx}* spans with `inline_{idx}` code\n"
        ));
        markdown.push_str(&format!(
            "- keep [link {idx}](https://example.invalid/readme/{idx}) readable\n\n"
        ));
        markdown.push_str("> quoted `inline` note with *emphasis*\n\n");
        markdown.push_str("| Feature | State | Notes |\n");
        markdown.push_str("| --- | --- | --- |\n");
        markdown.push_str(&format!("| **Markdown** | `Ready` | *section {idx}* |\n\n"));
        markdown.push_str("```rust\n");
        markdown.push_str(&format!("let section_{idx} = {idx};\n"));
        markdown.push_str("```\n\n");
    }
    assert!(markdown.len() > 35_000);

    let mut app = TuiApp::new();
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: markdown,
    });

    let started = Instant::now();
    let rendered_lines = transcript_text(&app, 100).lines;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(750),
        "readme-sized markdown render took {elapsed:?}"
    );

    let rendered = rendered_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(rendered.len() > 900);
    assert!(rendered
        .iter()
        .any(|line| line.contains("- parse bold 42 and italic 42 spans with inline_42 code")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("link 42 <https://example.invalid/readme/42>")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("Markdown  Ready  section 42")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("let section_42 = 42;")));
    assert!(!rendered.iter().any(|line| line.contains("```")));
    assert!(!rendered.iter().any(|line| line.contains("**")));
}

#[test]
fn transcript_keeps_divider_close_to_next_user_block() {
    let mut app = TuiApp::new();
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Divider,
        text: "Worked for 1s".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::User,
        text: "next prompt".to_string(),
    });

    let rendered = transcript_text(&app, 24)
        .lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let divider = rendered
        .iter()
        .position(|line| line.starts_with("- Worked for 1s"))
        .expect("divider line");

    assert_eq!(rendered[divider + 1], "");
    assert_eq!(rendered[divider + 2], blank_fill(22));
    assert!(rendered[divider + 3].starts_with("› next prompt"));
    assert_eq!(rendered[divider + 4], blank_fill(22));
    assert_eq!(rendered[divider + 5], "");
}

#[test]
fn empty_transcript_intro_omits_status_line_shortcut_hints() {
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(false, 1, 0, 0, None));

    let intro = transcript_text(&app, 80);
    let rendered = intro
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.contains("Heddle"));
    assert!(rendered.contains("model:"));
    assert!(rendered.contains("directory:"));
    let status = status_line(&app, 80);
    assert!(!status.contains("Enter submit"));
    assert!(!status.contains("\\ then Enter newline"));
    assert!(!status.contains("Esc exit"));
    assert!(status.starts_with("model:"));
}

#[test]
fn status_line_idle_uses_runtime_message_count_and_visible_tool_rows() {
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(false, 2, 0, 0, None));
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::User,
        text: "prompt".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: "answer".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::System,
        text: "working".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Divider,
        text: "Worked for 1s".to_string(),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Tool,
        text: "Read file finished".to_string(),
    });

    let status = status_line(&app, 120);

    assert!(status.starts_with("model:"));
    assert!(status.contains("msgs: 2"));
    assert!(status.contains("tools: 1"));
    assert!(!status.contains("msgs: 4"));
}

#[test]
fn status_line_shows_last_routed_model_when_it_differs_from_configured_model() {
    let mut app = TuiApp::new();
    let mut status = runtime_status(false, 2, 0, 0, None);
    status.model = "openrouter/free".to_string();
    status.last_routed_model = Some("openai/gpt-oss-120b".to_string());
    app.status = Some(status);

    let line = status_line(&app, 120);

    assert!(line.contains("model: openrouter/free:openai/gpt-oss-120b"));
}

#[test]
fn routed_model_event_updates_cached_tui_status() {
    let mut app = TuiApp::new();
    let mut status = runtime_status(false, 2, 0, 0, None);
    status.model = "openrouter/free".to_string();
    app.status = Some(status);

    app.apply_runtime_event(RuntimeEvent::RoutedModel {
        model: "openai/gpt-oss-20b".to_string(),
    });

    assert!(status_line(&app, 120).contains("openrouter/free:openai/gpt-oss-20b"));
}

#[test]
fn status_line_post_usage_shows_tokens_and_cost_without_state_prefix() {
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(false, 4, 1234, 56, Some(0.00125)));
    app.last_turn_status = Some(TurnStatus::Error);

    let status = status_line(&app, 120);

    assert!(status.starts_with("model:"));
    assert!(status.contains("msgs: 4"));
    assert!(status.contains("tokens: 1234/56"));
    assert!(status.contains("$0.0013"));
}

#[test]
fn status_line_truncates_deterministically_to_terminal_width() {
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(false, 42, 123456, 7890, Some(0.1234)));

    let status = status_line(&app, 24);

    assert_eq!(status.chars().count(), 24);
    assert!(status.ends_with("..."));
    assert_eq!(status, status_line(&app, 24));
}

#[test]
fn render_narrow_terminals_keep_prompt_and_status_readable() {
    for (width, height) in [(60, 16), (80, 24)] {
        let mut app = TuiApp::new();
        app.status = Some(RuntimeStatus {
            session_id: "session".to_string(),
            model: "provider/super-long-model-name-for-narrow-terminal".to_string(),
            last_routed_model: None,
            messages_count: 128,
            active: false,
            total_input_tokens: 123_456,
            total_output_tokens: 7_890,
            cost_usd: Some(12.3456),
        });
        insert_input(&mut app.input, "short prompt");
        app.transcript.push(TranscriptItem {
            kind: TranscriptKind::Assistant,
            text: "visible assistant row".to_string(),
        });

        let screen = render_app(width, height, &mut app);

        assert_eq!(screen.width, width);
        assert_eq!(screen.height, height);
        assert_eq!(screen.lines.len(), height as usize);
        assert!(screen
            .lines
            .iter()
            .all(|line| line.chars().count() == width as usize));
        assert!(screen.contains("short prompt"));
        assert!(screen.contains("visible assistant row"));
        assert!(screen.last_line().starts_with("model:"));
        assert_eq!(screen.last_line().chars().count(), width as usize);
    }
}

#[test]
fn render_short_terminal_has_defined_clipped_layout_without_panicking() {
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(false, 1, 2, 3, None));
    insert_input(&mut app.input, "tiny");

    // Current behavior for below-comfort terminals is a clipped normal layout.
    // Keep this explicit until the app grows a dedicated minimum-size message.
    let screen = render_app(24, 4, &mut app);

    assert_eq!(screen.lines.len(), 4);
    assert!(screen.lines.iter().all(|line| line.chars().count() == 24));
    assert!(screen.contains("tiny"));
    assert!(screen.last_line().starts_with("model:"));
}

#[test]
fn render_long_assistant_tool_and_user_text_wraps_or_truncates() {
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(false, 4, 1000, 200, None));
    app.turns.push(TranscriptTurn {
            items: vec![
                TurnTranscriptItem::Row(TranscriptItem {
                    kind: TranscriptKind::User,
                    text: "user first line\nuser second line with enough text to wrap in the transcript band"
                        .to_string(),
                }),
                TurnTranscriptItem::Row(TranscriptItem {
                    kind: TranscriptKind::Assistant,
                    text: "assistant-".repeat(32),
                }),
            ],
        });
    app.refresh_transcript_cache();
    let call = tool_call_with_args(
        "call-long",
        "custom_tool",
        &format!(
            r#"{{"payload":"{}arg-tail-marker"}}"#,
            "argument-".repeat(40)
        ),
    );
    app.apply_runtime_event(RuntimeEvent::ToolFinished {
        name: "custom_tool".to_string(),
        result: format!("{}result-tail-marker", "result-".repeat(60)),
        call,
    });

    app.viewport.follow_tail = false;
    let screen = render_app(72, 40, &mut app);

    assert!(screen.contains("user first line"));
    assert!(screen.contains("user second line"));
    assert!(screen.contains("assistant-assistant"));
    assert!(screen.contains("custom_tool"));
    assert!(screen.contains("..."));
    assert!(!screen.contains("arg-tail-marker"));
    assert!(!screen.contains("result-tail-marker"));
    assert!(screen.last_line().starts_with("model:"));
}

#[tokio::test]
async fn active_turn_prompt_ignores_input_and_renders_status_without_cursor_panic() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    app.status = Some(runtime_status(true, 3, 10, 20, None));
    app.active = true;
    app.turn_started_at = Some(Instant::now() - Duration::from_secs(2));
    insert_input(&mut app.input, "unchanged draft");
    let mut turn_counter = 0;

    app.handle_key(
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        &command_tx,
        &mut turn_counter,
    )
    .await
    .expect("active key");
    app.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &command_tx,
        &mut turn_counter,
    )
    .await
    .expect("active enter");

    let screen = render_app(60, 14, &mut app);

    assert_eq!(app.input.text(), "unchanged draft");
    assert_eq!(turn_counter, 0);
    assert!(command_rx.try_recv().is_err());
    assert!(screen.contains("unchanged draft"));
    assert!(screen.last_line().starts_with("model:"));
}

#[tokio::test]
async fn ctrl_end_returns_manual_scroll_to_latest_output() {
    let (command_tx, _) = mpsc::channel(1);
    let mut app = TuiApp::new();
    add_long_transcript(&mut app, 90);
    let mut turn_counter = 0;

    let bottom = render_app(80, 24, &mut app);
    assert!(bottom.contains("transcript row 089"));
    let tail_scroll = app.viewport.scroll_top;
    app.handle_key(
        KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        &command_tx,
        &mut turn_counter,
    )
    .await
    .expect("page up");
    let manual = render_app(80, 24, &mut app);
    assert!(!app.viewport.follow_tail);
    assert!(app.viewport.scroll_top < tail_scroll);
    assert!(!manual.contains("transcript row 089"));

    app.handle_key(
        KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL),
        &command_tx,
        &mut turn_counter,
    )
    .await
    .expect("ctrl end");
    let returned = render_app(80, 24, &mut app);

    assert!(app.viewport.follow_tail);
    assert_eq!(app.viewport.scroll_top, app.viewport.max_scroll());
    assert!(returned.contains("transcript row 089"));
}

#[test]
fn render_error_status_rows_and_empty_transcript_hint() {
    let mut empty = TuiApp::new();
    empty.status = Some(runtime_status(false, 0, 0, 0, None));

    let intro = render_app(60, 16, &mut empty);

    assert!(intro.contains("Heddle"));
    assert!(intro.contains("model:"));
    assert!(intro.last_line().starts_with("model:"));

    let mut failed = TuiApp::new();
    failed.status = Some(runtime_status(false, 2, 10, 20, None));
    failed.last_turn_status = Some(TurnStatus::Error);
    failed.apply_runtime_event(RuntimeEvent::Error {
        error: RuntimeError {
            code: "provider_error".to_string(),
            message: "provider said no".to_string(),
            retryable: false,
            provider: None,
            details: None,
        },
    });

    let error_screen = render_app(60, 16, &mut failed);

    assert!(error_screen.contains("! provider said no"));
    assert!(error_screen.last_line().starts_with("model:"));
}

#[test]
fn slash_command_parser_recognizes_tui_local_commands() {
    assert_eq!(parse_tui_slash_command("/clear"), SlashCommand::Clear);
    assert_eq!(parse_tui_slash_command(" /status "), SlashCommand::Status);
    assert_eq!(parse_tui_slash_command("/help"), SlashCommand::Help);
    assert_eq!(
        parse_tui_slash_command("/copy last"),
        SlashCommand::CopyLast
    );
    assert_eq!(
        parse_tui_slash_command("/copy turn"),
        SlashCommand::CopyTurn
    );
    assert_eq!(
        parse_tui_slash_command("/export transcript"),
        SlashCommand::ExportTranscript
    );
    assert_eq!(parse_tui_slash_command("/quit"), SlashCommand::Quit);
    assert_eq!(parse_tui_slash_command("/exit"), SlashCommand::Quit);
}

#[tokio::test]
async fn copy_last_stores_raw_assistant_text_without_wrapped_line_breaks() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let temp = tempfile::tempdir().expect("tempdir");
    let mut app = TuiApp::new();
    app.cwd = temp.path().display().to_string();
    let raw = "# Heading\n\n- first bullet\n- second bullet\n\n```rust\nfn main() {}\n```";
    app.turns.push(TranscriptTurn {
        items: vec![TurnTranscriptItem::Row(TranscriptItem {
            kind: TranscriptKind::Assistant,
            text: raw.to_string(),
        })],
    });
    app.refresh_transcript_cache();

    app.apply_slash_command(SlashCommand::CopyLast, &command_tx)
        .await
        .expect("copy last");

    assert!(command_rx.try_recv().is_err());
    assert_eq!(app.copy_buffer.as_deref(), Some(raw));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("heddle-copy.md")).expect("copy file"),
        raw
    );
    assert!(app
        .transcript
        .last()
        .expect("status row")
        .text
        .contains("heddle-copy.md"));
}

#[tokio::test]
async fn copy_turn_and_export_transcript_use_logical_markdown() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let temp = tempfile::tempdir().expect("tempdir");
    let mut app = TuiApp::new();
    app.cwd = temp.path().display().to_string();
    app.turns.push(TranscriptTurn {
        items: vec![
            TurnTranscriptItem::Row(TranscriptItem {
                kind: TranscriptKind::User,
                text: "summarize".to_string(),
            }),
            TurnTranscriptItem::Row(TranscriptItem {
                kind: TranscriptKind::Assistant,
                text: "done".to_string(),
            }),
        ],
    });
    app.refresh_transcript_cache();

    app.apply_slash_command(SlashCommand::CopyTurn, &command_tx)
        .await
        .expect("copy turn");
    assert!(command_rx.try_recv().is_err());
    let copied = app.copy_buffer.as_ref().expect("copy buffer");
    assert!(copied.contains("## User\n\nsummarize"));
    assert!(copied.contains("## Assistant\n\ndone"));
    let copy_file = std::fs::read_to_string(temp.path().join("heddle-copy.md")).expect("copy file");
    assert_eq!(copy_file, *copied);

    app.apply_slash_command(SlashCommand::ExportTranscript, &command_tx)
        .await
        .expect("export transcript");
    let exported = app.copy_buffer.as_ref().expect("export buffer");
    assert!(exported.starts_with("# Turn 1"));
    assert!(exported.contains("## Assistant\n\ndone"));
    let export_file =
        std::fs::read_to_string(temp.path().join("heddle-transcript.md")).expect("export file");
    assert_eq!(export_file, *exported);
}

#[tokio::test]
async fn slash_commands_do_not_route_to_runtime_channel() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;
    for c in "/help".chars() {
        app.input.insert_char(c);
    }

    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("submit");

    assert!(command_rx.try_recv().is_err());
    assert_eq!(turn_counter, 0);
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::System);
    assert!(app.transcript[0].text.contains("/clear"));
    assert!(app.transcript[0].text.contains("/copy last"));
}

#[tokio::test]
async fn non_slash_submit_routes_to_runtime_channel() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;
    for c in "hello model".chars() {
        app.input.insert_char(c);
    }

    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("submit");

    let command = command_rx.try_recv().expect("runtime command");
    let RuntimeCommand::Send { id, message, .. } = command else {
        panic!("expected send command");
    };
    assert_eq!(id, "tui-turn-1");
    assert_eq!(message, "hello model");
    assert_eq!(turn_counter, 1);
    assert!(app.active);
}

#[tokio::test]
async fn unknown_slash_command_adds_visible_system_row_without_runtime_send() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;
    for c in "/bogus".chars() {
        app.input.insert_char(c);
    }

    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("submit");

    assert!(command_rx.try_recv().is_err());
    assert_eq!(turn_counter, 0);
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::System);
    assert!(app.transcript[0].text.contains("unknown command: /bogus"));
}

#[tokio::test]
async fn clear_slash_command_resets_view_and_requests_runtime_context_clear() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    app.status = Some(RuntimeStatus {
        session_id: "session-1".to_string(),
        model: "model-a".to_string(),
        last_routed_model: None,
        messages_count: 3,
        active: false,
        total_input_tokens: 13,
        total_output_tokens: 21,
        cost_usd: Some(0.125),
    });
    app.transcript.push(TranscriptItem {
        kind: TranscriptKind::Assistant,
        text: "old visible row".to_string(),
    });
    for c in "/clear".chars() {
        app.input.insert_char(c);
    }
    let mut turn_counter = 0;

    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("submit");

    assert!(matches!(
        command_rx.try_recv().expect("clear context command"),
        RuntimeCommand::ClearContext
    ));
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::System);
    assert!(app.transcript[0].text.contains("Context cleared"));
    assert!(app.status.is_some());
    assert_eq!(turn_counter, 0);
}

#[tokio::test]
async fn status_slash_command_adds_runtime_status_row() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    app.status = Some(RuntimeStatus {
        session_id: "session-1".to_string(),
        model: "model-a".to_string(),
        last_routed_model: None,
        messages_count: 3,
        active: false,
        total_input_tokens: 13,
        total_output_tokens: 21,
        cost_usd: Some(0.125),
    });

    app.apply_slash_command(SlashCommand::Status, &command_tx)
        .await
        .expect("status command");

    assert!(command_rx.try_recv().is_err());
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptKind::System);
    assert!(app.transcript[0].text.contains("session: session-1"));
    assert!(app.transcript[0].text.contains("model: model-a"));
    assert!(app.transcript[0].text.contains("messages: 3"));
    assert!(app.transcript[0].text.contains("tokens: 13 in / 21 out"));
    assert!(app.transcript[0].text.contains("cost: $0.1250"));
}

#[tokio::test]
async fn status_slash_command_shows_last_routed_model_when_present() {
    let (command_tx, mut command_rx) = mpsc::channel(1);
    let mut app = TuiApp::new();
    app.status = Some(RuntimeStatus {
        session_id: "session-1".to_string(),
        model: "openrouter/free".to_string(),
        last_routed_model: Some("openai/gpt-oss-120b".to_string()),
        messages_count: 3,
        active: false,
        total_input_tokens: 13,
        total_output_tokens: 21,
        cost_usd: Some(0.125),
    });

    app.apply_slash_command(SlashCommand::Status, &command_tx)
        .await
        .expect("status command");

    assert!(command_rx.try_recv().is_err());
    assert!(app.transcript[0]
        .text
        .contains("model: openrouter/free:openai/gpt-oss-120b"));
}

#[tokio::test]
async fn help_slash_command_renders_supported_commands() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    let (command_tx, mut command_rx) = mpsc::channel(1);
    app.apply_slash_command(SlashCommand::Help, &command_tx)
        .await
        .expect("help command");
    assert!(command_rx.try_recv().is_err());

    terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(screen.contains("/help"));
    assert!(screen.contains("/status"));
    assert!(screen.contains("/clear"));
    assert!(screen.contains("Ctrl-C"));
}

#[test]
fn render_manual_scroll_is_not_yanked_by_active_output() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    add_long_transcript(&mut app, 90);

    let bottom = draw_screen(&mut terminal, &mut app);
    assert!(bottom.contains("transcript row 089"));
    let tail_scroll = app.viewport.scroll_top;

    app.handle_mouse(MouseEventKind::ScrollUp);
    let _ = draw_screen(&mut terminal, &mut app);
    let manual_scroll = app.viewport.scroll_top;
    assert!(manual_scroll < tail_scroll);
    assert!(!app.viewport.follow_tail);

    app.active = true;
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "streamed tail marker".to_string(),
    });
    let scrolled = draw_screen(&mut terminal, &mut app);

    assert_eq!(app.viewport.scroll_top, manual_scroll);
    assert!(!app.viewport.follow_tail);
    assert!(!scrolled.contains("streamed tail marker"));
}

#[test]
fn render_input_growth_preserves_manual_scroll_and_bottom_remains_reachable() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    add_long_transcript(&mut app, 90);

    let _ = draw_screen(&mut terminal, &mut app);
    app.handle_mouse(MouseEventKind::ScrollUp);
    let _ = draw_screen(&mut terminal, &mut app);
    let manual_scroll = app.viewport.scroll_top;
    let old_viewport_height = app.viewport.viewport_height;

    for _ in 0..5 {
        app.input.insert_newline();
    }
    let grown = draw_screen(&mut terminal, &mut app);

    assert!(app.viewport.viewport_height < old_viewport_height);
    assert_eq!(app.viewport.scroll_top, manual_scroll);
    assert!(app.viewport.scroll_top <= app.viewport.max_scroll());
    assert!(!app.viewport.follow_tail);
    assert!(!grown.contains("transcript row 089"));

    app.viewport.jump_to_bottom();
    let bottom = draw_screen(&mut terminal, &mut app);
    assert!(bottom.contains("transcript row 089"));
    assert!(app.viewport.follow_tail);
}

#[tokio::test]
async fn render_streaming_tail_stays_above_input_across_turns() {
    let (command_tx, mut command_rx) = mpsc::channel(2);
    let mut app = TuiApp::new();
    let mut turn_counter = 0;

    insert_prompt(&mut app, "first prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("first submit");
    let _ = command_rx.try_recv().expect("first command");
    app.apply_runtime_event(RuntimeEvent::ContentDelta {
            text: (0..48)
                .map(|idx| {
                    if idx % 7 == 0 {
                        format!(
                            "first-response-line-{idx:02} **bold markdown marker** `inline code` [link](https://example.invalid/path)"
                        )
                    } else {
                        format!("first-response-line-{idx:02}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        });
    app.apply_turn_outcome(ok_outcome());

    let first_tail = render_app(80, 24, &mut app);
    assert!(first_tail.contains("first-response-line-47"));
    assert!(!first_tail.last_line().contains("first-response-line-47"));

    let fullscreen_tail = render_app(160, 48, &mut app);
    assert!(fullscreen_tail.contains("first-response-line-47"));
    assert!(!fullscreen_tail
        .last_line()
        .contains("first-response-line-47"));

    insert_prompt(&mut app, "second prompt");
    app.submit(&command_tx, &mut turn_counter)
        .await
        .expect("second submit");
    let _ = command_rx.try_recv().expect("second command");
    let submitted = render_app(80, 24, &mut app);
    assert!(submitted.contains("second prompt"));
    assert!(submitted.contains("Working..."));

    app.apply_runtime_event(RuntimeEvent::ContentDelta {
        text: "second response visible".to_string(),
    });
    app.apply_turn_outcome(ok_outcome());
    let second_tail = render_app(80, 24, &mut app);

    assert!(second_tail.contains("second prompt"));
    assert!(second_tail.contains("second response visible"));
    assert!(!second_tail.last_line().contains("second response visible"));
}

#[test]
fn render_resize_clamps_manual_scroll_and_can_jump_to_bottom() {
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    add_long_transcript(&mut app, 90);

    let _ = draw_screen(&mut terminal, &mut app);
    app.handle_mouse(MouseEventKind::ScrollUp);
    app.handle_mouse(MouseEventKind::ScrollUp);
    let _ = draw_screen(&mut terminal, &mut app);
    let manual_scroll = app.viewport.scroll_top;

    terminal.backend_mut().resize(80, 14);
    terminal
        .resize(Rect::new(0, 0, 80, 14))
        .expect("terminal resize");
    let _ = draw_screen(&mut terminal, &mut app);
    assert_eq!(app.viewport.scroll_top, manual_scroll);
    assert!(app.viewport.scroll_top <= app.viewport.max_scroll());
    assert!(!app.viewport.follow_tail);

    terminal.backend_mut().resize(80, 60);
    terminal
        .resize(Rect::new(0, 0, 80, 60))
        .expect("terminal resize");
    let _ = draw_screen(&mut terminal, &mut app);
    assert!(app.viewport.scroll_top <= app.viewport.max_scroll());
    assert!(!app.viewport.follow_tail);

    app.viewport.jump_to_bottom();
    let bottom = draw_screen(&mut terminal, &mut app);
    assert!(bottom.contains("transcript row 089"));
    assert_eq!(app.viewport.scroll_top, app.viewport.max_scroll());
}

#[test]
fn mouse_scroll_down_from_manual_scroll_returns_to_tail() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    add_long_transcript(&mut app, 90);

    let _ = draw_screen(&mut terminal, &mut app);
    app.handle_mouse(MouseEventKind::ScrollUp);
    app.handle_mouse(MouseEventKind::ScrollUp);
    let _ = draw_screen(&mut terminal, &mut app);
    assert!(!app.viewport.follow_tail);

    for _ in 0..20 {
        app.handle_mouse(MouseEventKind::ScrollDown);
    }
    let bottom = draw_screen(&mut terminal, &mut app);

    assert!(app.viewport.follow_tail);
    assert_eq!(app.viewport.scroll_top, app.viewport.max_scroll());
    assert!(bottom.contains("transcript row 089"));
}

#[test]
fn render_includes_multiline_input_and_status() {
    let backend = TestBackend::new(60, 14);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    for c in "first line".chars() {
        app.input.insert_char(c);
    }
    app.input.insert_newline();
    for c in "second line".chars() {
        app.input.insert_char(c);
    }

    terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(screen.contains("first line"));
    assert!(screen.contains("second line"));
    assert!(screen.contains("initializing runtime"));
}

#[test]
fn permission_prompt_renders_tool_details_and_choices() {
    let backend = TestBackend::new(80, 16);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = TuiApp::new();
    app.permission_prompt_view = Some(PermissionPromptView {
        name: "write_file".to_string(),
        call_id: "call_7".to_string(),
        arguments: r#"{"file_path":"src/main.rs","content":"updated"}"#.to_string(),
        reason: Some("write_file requires approval".to_string()),
    });

    terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(screen.contains("Permission required"));
    assert!(screen.contains("write_file"));
    assert!(screen.contains("call_7"));
    assert!(screen.contains("write_file requires approval"));
    assert!(screen.contains("Y allow"));
    assert!(screen.contains("N deny and continue"));
    assert!(screen.contains("A always allow"));
}

#[test]
fn permission_prompt_answer_sends_response_and_clears_prompt() {
    let mut app = TuiApp::new();
    let (respond_to, mut response_rx) = oneshot::channel();
    app.set_permission_prompt(PermissionPrompt {
        request: RuntimePermissionRequest {
            name: "write_file".to_string(),
            call: tool_call_with_args(
                "call_1",
                "write_file",
                r#"{"file_path":"foo.txt","content":"bar"}"#,
            ),
            reason: Some("write_file requires approval".to_string()),
        },
        respond_to,
    });

    app.answer_permission_prompt(RuntimePermissionResponse::Always);

    assert!(app.permission_prompt.is_none());
    assert!(app.permission_prompt_view.is_none());
    assert_eq!(
        response_rx.try_recv().expect("permission response"),
        RuntimePermissionResponse::Always
    );
}
