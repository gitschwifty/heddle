//! Ratatui frontend over the runtime facade.
//!
//! This first pass keeps terminal concerns local to the TUI and treats
//! `HeddleRuntime` as the turn execution boundary.

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::config::features::Mode;
use crate::runtime::{
    HeddleRuntime, RuntimeConfig, RuntimeEvent, RuntimePermissionRequest,
    RuntimePermissionResolver, RuntimePermissionResponse, RuntimeStatus, TurnOptions, TurnOutcome,
    TurnState, TurnStatus,
};
use crate::session::setup::SessionOptions;

#[derive(Debug, Parser)]
#[command(
    name = "heddle-tui",
    about = "Experimental Ratatui frontend for Heddle"
)]
pub struct TuiArgs {
    #[arg(long)]
    resume: Option<String>,

    #[arg(long)]
    fork: Option<String>,

    #[arg(long)]
    model: Option<String>,

    #[arg(long = "session-name")]
    session_name: Option<String>,
}

pub async fn run_from_args() -> Result<()> {
    let args = TuiArgs::parse();
    run(args).await
}

pub async fn run(args: TuiArgs) -> Result<()> {
    let runtime = HeddleRuntime::init(RuntimeConfig {
        session: SessionOptions {
            mode: Some(Mode::Interactive),
            resume: args.resume,
            fork: args.fork,
            model: args.model,
            session_name: args.session_name,
            ..SessionOptions::default()
        },
    })
    .await?;

    run_terminal(runtime).await
}

async fn run_terminal(runtime: HeddleRuntime) -> Result<()> {
    let mut terminal = TerminalSession::enter()?;
    let (command_tx, command_rx) = mpsc::channel(4);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    tokio::spawn(runtime_worker(runtime, command_rx, event_tx));

    let mut app = TuiApp::new();
    let mut turn_counter = 0_u64;

    loop {
        while let Ok(update) = event_rx.try_recv() {
            app.apply_runtime_update(update);
        }

        terminal.draw(|frame| draw(frame, &mut app))?;

        if app.should_quit && !app.active {
            break;
        }

        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_key(key, &command_tx, &mut turn_counter).await? {
                        break;
                    }
                }
                Event::Mouse(mouse) => app.handle_mouse(mouse.kind),
                _ => {}
            }
        }
    }

    Ok(())
}

async fn runtime_worker(
    mut runtime: HeddleRuntime,
    mut command_rx: mpsc::Receiver<RuntimeCommand>,
    event_tx: mpsc::UnboundedSender<RuntimeUpdate>,
) {
    let _ = event_tx.send(RuntimeUpdate::Status(runtime.status(false)));
    while let Some(command) = command_rx.recv().await {
        match command {
            RuntimeCommand::Send {
                id,
                message,
                cancel,
            } => {
                let permission_resolver =
                    build_tui_permission_resolver(event_tx.clone(), cancel.clone());
                let _ = event_tx.send(RuntimeUpdate::Status(runtime.status(true)));
                let outcome = runtime
                    .send(
                        message,
                        TurnOptions {
                            id,
                            cancel,
                            permission_resolver: Some(permission_resolver),
                        },
                        |event| {
                            let _ = event_tx.send(RuntimeUpdate::Event(event));
                        },
                    )
                    .await;
                let _ = event_tx.send(RuntimeUpdate::Outcome(outcome));
                let _ = event_tx.send(RuntimeUpdate::Status(runtime.status(false)));
            }
        }
    }
}

fn build_tui_permission_resolver(
    event_tx: mpsc::UnboundedSender<RuntimeUpdate>,
    cancel: CancellationToken,
) -> RuntimePermissionResolver {
    Arc::new(move |request| {
        let event_tx = event_tx.clone();
        let cancel = cancel.clone();
        Box::pin(async move {
            if cancel.is_cancelled() {
                return RuntimePermissionResponse::Deny;
            }

            let (respond_to, response_rx) = oneshot::channel();
            if event_tx
                .send(RuntimeUpdate::PermissionPrompt(PermissionPrompt {
                    request,
                    respond_to,
                }))
                .is_err()
            {
                return RuntimePermissionResponse::Deny;
            }

            tokio::select! {
                _ = cancel.cancelled() => RuntimePermissionResponse::Deny,
                response = response_rx => response.unwrap_or(RuntimePermissionResponse::Deny),
            }
        })
    })
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn draw<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Frame),
    {
        self.terminal.draw(f)?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

enum RuntimeCommand {
    Send {
        id: String,
        message: String,
        cancel: CancellationToken,
    },
}

#[derive(Debug)]
enum RuntimeUpdate {
    Event(RuntimeEvent),
    Outcome(TurnOutcome),
    Status(RuntimeStatus),
    PermissionPrompt(PermissionPrompt),
}

#[derive(Debug)]
struct PermissionPrompt {
    request: RuntimePermissionRequest,
    respond_to: oneshot::Sender<RuntimePermissionResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionPromptView {
    name: String,
    call_id: String,
    arguments: String,
    reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptKind {
    User,
    Assistant,
    Tool,
    Error,
    System,
    Divider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptItem {
    kind: TranscriptKind,
    text: String,
}

#[derive(Debug, Default)]
struct TuiApp {
    input: InputBuffer,
    transcript: Vec<TranscriptItem>,
    tool_rows: HashMap<String, usize>,
    active_assistant: Option<usize>,
    pending_work_row: Option<usize>,
    turn_started_at: Option<Instant>,
    transcript_scroll: u16,
    max_transcript_scroll: u16,
    follow_tail: bool,
    status: Option<RuntimeStatus>,
    active: bool,
    should_quit: bool,
    active_cancel: Option<CancellationToken>,
    permission_prompt: Option<PermissionPrompt>,
    permission_prompt_view: Option<PermissionPromptView>,
    cwd: String,
}

impl TuiApp {
    fn new() -> Self {
        Self {
            follow_tail: true,
            cwd: std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            ..Self::default()
        }
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        command_tx: &mpsc::Sender<RuntimeCommand>,
        turn_counter: &mut u64,
    ) -> Result<bool> {
        if self.permission_prompt.is_some() {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    if let Some(cancel) = &self.active_cancel {
                        cancel.cancel();
                    }
                    self.answer_permission_prompt(RuntimePermissionResponse::Deny);
                }
                (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) => {
                    self.answer_permission_prompt(RuntimePermissionResponse::Allow);
                }
                (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) | (KeyCode::Esc, _) => {
                    self.answer_permission_prompt(RuntimePermissionResponse::Deny);
                }
                (KeyCode::Char('a'), _) | (KeyCode::Char('A'), _) => {
                    self.answer_permission_prompt(RuntimePermissionResponse::Always);
                }
                _ => {}
            }
            return Ok(false);
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            (KeyCode::Esc, _) => {
                if let Some(cancel) = &self.active_cancel {
                    cancel.cancel();
                } else if !self.follow_tail {
                    self.follow_tail = true;
                    self.transcript_scroll = self.max_transcript_scroll;
                } else {
                    return Ok(true);
                }
            }
            (KeyCode::PageUp, _) => {
                let current = if self.follow_tail {
                    self.max_transcript_scroll
                } else {
                    self.transcript_scroll
                };
                self.follow_tail = false;
                self.transcript_scroll = current.saturating_sub(5);
            }
            (KeyCode::PageDown, _) => {
                self.follow_tail = false;
                self.transcript_scroll = self
                    .transcript_scroll
                    .saturating_add(5)
                    .min(self.max_transcript_scroll);
                self.follow_tail = self.transcript_scroll == self.max_transcript_scroll;
            }
            (KeyCode::End, KeyModifiers::CONTROL) => {
                self.follow_tail = true;
            }
            (KeyCode::Up, _) if !self.active => self.input.move_up(),
            (KeyCode::Down, _) if !self.active => self.input.move_down(),
            (KeyCode::Left, _) if !self.active => self.input.move_left(),
            (KeyCode::Right, _) if !self.active => self.input.move_right(),
            (KeyCode::Home, _) if !self.active => self.input.move_line_start(),
            (KeyCode::End, _) if !self.active => self.input.move_line_end(),
            (KeyCode::Backspace, _) => {
                if !self.active {
                    self.input.backspace();
                }
            }
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                if !self.active {
                    self.input.insert_newline();
                }
            }
            (KeyCode::Enter, _) if self.input.consume_trailing_backslash() => {
                if !self.active {
                    self.input.insert_newline();
                }
            }
            (KeyCode::Enter, _) => {
                self.submit(command_tx, turn_counter).await?;
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                if !self.active {
                    self.input.insert_char(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_mouse(&mut self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::ScrollUp => {
                let current = if self.follow_tail {
                    self.max_transcript_scroll
                } else {
                    self.transcript_scroll
                };
                self.follow_tail = false;
                self.transcript_scroll = current.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                self.transcript_scroll = self
                    .transcript_scroll
                    .saturating_add(3)
                    .min(self.max_transcript_scroll);
                self.follow_tail = self.transcript_scroll == self.max_transcript_scroll;
            }
            _ => {}
        }
    }

    async fn submit(
        &mut self,
        command_tx: &mpsc::Sender<RuntimeCommand>,
        turn_counter: &mut u64,
    ) -> Result<()> {
        let message = self.input.text().trim().to_string();
        if message.is_empty() || self.active {
            return Ok(());
        }

        if matches!(message.as_str(), "/quit" | "/exit") {
            self.should_quit = true;
            return Ok(());
        }

        self.input.clear();
        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::User,
            text: message.clone(),
        });
        let pending_row = self.transcript.len();
        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::System,
            text: "working for 0s - Esc to interrupt".to_string(),
        });
        self.pending_work_row = Some(pending_row);
        self.turn_started_at = Some(Instant::now());
        self.follow_tail = true;

        *turn_counter += 1;
        let cancel = CancellationToken::new();
        self.active_cancel = Some(cancel.clone());
        self.active = true;
        command_tx
            .send(RuntimeCommand::Send {
                id: format!("tui-turn-{turn_counter}"),
                message,
                cancel,
            })
            .await?;
        Ok(())
    }

    fn apply_runtime_update(&mut self, update: RuntimeUpdate) {
        match update {
            RuntimeUpdate::Event(event) => self.apply_runtime_event(event),
            RuntimeUpdate::Outcome(outcome) => self.apply_turn_outcome(outcome),
            RuntimeUpdate::Status(status) => {
                self.active = status.active;
                if !status.active {
                    self.active_cancel = None;
                }
                self.status = Some(status);
            }
            RuntimeUpdate::PermissionPrompt(prompt) => self.set_permission_prompt(prompt),
        }
    }

    fn apply_runtime_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::TurnStateChanged { state, .. } => {
                self.active = matches!(state, TurnState::Running | TurnState::Cancelling);
                if matches!(state, TurnState::Completed) {
                    self.active_cancel = None;
                }
            }
            RuntimeEvent::ContentDelta { text } => {
                self.clear_pending_work();
                self.append_assistant_delta(&text);
            }
            RuntimeEvent::ToolStarted { name, call } => {
                self.clear_pending_work();
                let row = self.transcript.len();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Tool,
                    text: format_tool_row(&name, "running", Some(&call.function.arguments), None),
                });
                self.tool_rows.insert(call.id, row);
            }
            RuntimeEvent::ToolFinished { name, result, call } => {
                self.clear_pending_work();
                let text = format_tool_row(
                    &name,
                    "finished",
                    Some(&call.function.arguments),
                    Some(&result),
                );
                if let Some(row) = self.tool_rows.remove(&call.id) {
                    self.transcript[row].text = text;
                } else {
                    self.transcript.push(TranscriptItem {
                        kind: TranscriptKind::Tool,
                        text,
                    });
                }
            }
            RuntimeEvent::UsageUpdated { .. } => {}
            RuntimeEvent::Error { error } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Error,
                    text: error.message,
                });
            }
            RuntimeEvent::PermissionRequested { name, reason, .. } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!(
                        "permission requested: {name} {}",
                        reason.unwrap_or_default()
                    )
                    .trim()
                    .to_string(),
                });
            }
            RuntimeEvent::PermissionDenied { name, reason, .. } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Error,
                    text: format!("permission denied: {name}: {reason}"),
                });
            }
            RuntimeEvent::PlanCompleted { plan } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!("plan completed\n{plan}"),
                });
            }
            RuntimeEvent::ContextPruned {
                messages_pruned, ..
            } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!("context pruned: {messages_pruned} messages"),
                });
            }
            RuntimeEvent::ContextCompacted => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "context compacted".to_string(),
                });
            }
            RuntimeEvent::ContextHandoff => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "context handoff".to_string(),
                });
            }
            RuntimeEvent::AssistantMessage { message, .. } => {
                if let Some(content) = message.content {
                    self.clear_pending_work();
                    self.set_assistant_message(content);
                }
            }
        }
    }

    fn apply_turn_outcome(&mut self, outcome: TurnOutcome) {
        let worked_for = self
            .turn_started_at
            .take()
            .map(|started| format_duration(started.elapsed()))
            .unwrap_or_else(|| "0s".to_string());

        self.active = false;
        self.active_cancel = None;
        self.active_assistant = None;
        self.clear_pending_work();
        self.permission_prompt = None;
        self.permission_prompt_view = None;
        let status = outcome.status;
        match &status {
            TurnStatus::Ok => {}
            TurnStatus::Cancelled => self.transcript.push(TranscriptItem {
                kind: TranscriptKind::System,
                text: "turn cancelled".to_string(),
            }),
            TurnStatus::Error => {
                if let Some(error) = outcome.error {
                    self.transcript.push(TranscriptItem {
                        kind: TranscriptKind::Error,
                        text: error.message,
                    });
                }
            }
        }
        self.push_turn_footer(&status, &worked_for);
    }

    fn append_assistant_delta(&mut self, text: &str) {
        let row = self.active_assistant.unwrap_or_else(|| {
            let row = self.transcript.len();
            self.transcript.push(TranscriptItem {
                kind: TranscriptKind::Assistant,
                text: String::new(),
            });
            self.active_assistant = Some(row);
            row
        });
        self.transcript[row].text.push_str(text);
    }

    fn set_assistant_message(&mut self, text: String) {
        let row = self.active_assistant.unwrap_or_else(|| {
            let row = self.transcript.len();
            self.transcript.push(TranscriptItem {
                kind: TranscriptKind::Assistant,
                text: String::new(),
            });
            self.active_assistant = Some(row);
            row
        });
        if self.transcript[row].text.is_empty() {
            self.transcript[row].text = text;
        }
    }

    fn set_permission_prompt(&mut self, prompt: PermissionPrompt) {
        self.clear_pending_work();
        self.permission_prompt_view = Some(PermissionPromptView::from_request(&prompt.request));
        self.permission_prompt = Some(prompt);
        self.follow_tail = true;
    }

    fn answer_permission_prompt(&mut self, response: RuntimePermissionResponse) {
        if let Some(prompt) = self.permission_prompt.take() {
            let _ = prompt.respond_to.send(response);
        }
        self.permission_prompt_view = None;
    }

    fn clear_pending_work(&mut self) {
        let Some(row) = self.pending_work_row.take() else {
            return;
        };
        if row < self.transcript.len() && self.transcript[row].kind == TranscriptKind::System {
            self.transcript.remove(row);
            self.tool_rows.retain(|_, tool_row| {
                if *tool_row > row {
                    *tool_row -= 1;
                }
                true
            });
            if let Some(active_row) = self.active_assistant.as_mut() {
                if *active_row > row {
                    *active_row -= 1;
                }
            }
        }
    }

    fn refresh_pending_work(&mut self) {
        let Some(row) = self.pending_work_row else {
            return;
        };
        let Some(started) = self.turn_started_at else {
            return;
        };
        if row < self.transcript.len() {
            self.transcript[row].text = format!(
                "working for {} - Esc to interrupt",
                format_duration(started.elapsed())
            );
        }
    }

    fn push_turn_footer(&mut self, status: &TurnStatus, worked_for: &str) {
        let suffix = match status {
            TurnStatus::Ok => "",
            TurnStatus::Cancelled => " - cancelled",
            TurnStatus::Error => " - error",
        };
        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::Divider,
            text: format!("Worked for {worked_for}{suffix}"),
        });
    }
}

impl PermissionPromptView {
    fn from_request(request: &RuntimePermissionRequest) -> Self {
        Self {
            name: request.name.clone(),
            call_id: request.call.id.clone(),
            arguments: summarize_arguments(&request.call.function.arguments, 240),
            reason: request.reason.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputBuffer {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl Default for InputBuffer {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
        }
    }
}

impl InputBuffer {
    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn insert_char(&mut self, c: char) {
        let idx = char_to_byte(&self.lines[self.row], self.col);
        self.lines[self.row].insert(idx, c);
        self.col += 1;
    }

    fn insert_newline(&mut self) {
        let idx = char_to_byte(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(idx);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.col = 0;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            let end = char_to_byte(&self.lines[self.row], self.col);
            let start = char_to_byte(&self.lines[self.row], self.col - 1);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
            self.lines[self.row].push_str(&current);
        }
    }

    fn consume_trailing_backslash(&mut self) -> bool {
        if self.col == 0 {
            return false;
        }
        let line = &self.lines[self.row];
        let mut chars = line.chars();
        if chars.nth(self.col - 1) != Some('\\') {
            return false;
        }
        self.backspace();
        true
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        }
    }

    fn move_right(&mut self) {
        let line_len = self.lines[self.row].chars().count();
        if self.col < line_len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.clamp_col();
        }
    }

    fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.clamp_col();
        }
    }

    fn move_line_start(&mut self) {
        self.col = 0;
    }

    fn move_line_end(&mut self) {
        self.col = self.lines[self.row].chars().count();
    }

    fn clamp_col(&mut self) {
        self.col = self.col.min(self.lines[self.row].chars().count());
    }

    fn cursor_position(&self, origin: Position, width: u16, height: u16) -> Position {
        let inner_width = width.max(1) as usize;
        let visible_height = height.max(1);
        let row = self
            .lines
            .iter()
            .take(self.row)
            .map(|line| visual_line_count(line, inner_width) as usize)
            .sum::<usize>()
            + (self.col / inner_width);
        let col = self.col % inner_width;
        Position::new(
            origin.x.saturating_add(col as u16),
            origin
                .y
                .saturating_add((row as u16).min(visible_height.saturating_sub(1))),
        )
    }

    fn visual_height(&self, width: u16) -> u16 {
        let width = width.max(1) as usize;
        self.lines
            .iter()
            .map(|line| visual_line_count(line, width))
            .sum::<u16>()
            .max(1)
    }
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| s.len())
}

fn visual_line_count(line: &str, width: usize) -> u16 {
    let chars = line.chars().count();
    ((chars / width) + 1).max(1) as u16
}

fn draw(frame: &mut Frame, app: &mut TuiApp) {
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
    let tail_scroll = transcript_lines
        .lines
        .len()
        .saturating_sub(chunks[0].height as usize) as u16;
    app.max_transcript_scroll = tail_scroll;
    let scroll = if app.follow_tail {
        tail_scroll
    } else {
        app.transcript_scroll.min(tail_scroll)
    };
    let transcript = Paragraph::new(transcript_lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(transcript, chunks[0]);

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
        let input = Paragraph::new(input_text(&app.input))
            .style(
                Style::default()
                    .fg(if app.active {
                        Color::DarkGray
                    } else {
                        Color::White
                    })
                    .bg(Color::Rgb(38, 38, 48)),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(input, chunks[1]);
    }
    if !app.active && app.permission_prompt_view.is_none() {
        frame.set_cursor_position(app.input.cursor_position(
            Position::new(chunks[1].x.saturating_add(2), chunks[1].y.saturating_add(1)),
            chunks[1].width.saturating_sub(2),
            chunks[1].height.saturating_sub(2),
        ));
    }

    let status = Paragraph::new(status_line(app));
    frame.render_widget(status, chunks[2]);
}

fn input_text(input: &InputBuffer) -> Text<'static> {
    let mut lines = Vec::new();
    lines.push(Line::raw(""));
    for (idx, line) in input.lines.iter().enumerate() {
        let prefix = if idx == 0 { "› " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Cyan)),
            Span::raw(line.clone()),
        ]));
    }
    lines.push(Line::raw(""));
    Text::from(lines)
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

fn transcript_text(app: &TuiApp, width: u16) -> Text<'static> {
    let mut lines = startup_text(app).lines;
    if app.transcript.is_empty() {
        return Text::from(lines);
    }

    for item in &app.transcript {
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

fn blank_fill(width: usize) -> String {
    "\u{00a0}".repeat(width)
}

fn wrap_message_lines(message: &str, width: usize) -> Vec<String> {
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

fn divider_line(label: &str, width: u16) -> String {
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

fn status_line(app: &TuiApp) -> String {
    let Some(status) = &app.status else {
        return "initializing runtime".to_string();
    };
    let cost = status
        .cost_usd
        .map(|cost| format!(" | ${cost:.4}"))
        .unwrap_or_default();
    let message_count = app
        .transcript
        .iter()
        .filter(|item| matches!(item.kind, TranscriptKind::User | TranscriptKind::Assistant))
        .count();
    let tool_count = app
        .transcript
        .iter()
        .filter(|item| matches!(item.kind, TranscriptKind::Tool))
        .count();
    format!(
        "model: {} | msgs: {} | tools: {} | tokens: {} in / {} out{cost}",
        status.model,
        message_count,
        tool_count,
        status.total_input_tokens,
        status.total_output_tokens,
    )
}

fn summarize_arguments(arguments: &str, max_chars: usize) -> String {
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

fn abbreviate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", value.chars().take(keep).collect::<String>())
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        return format!("{seconds}s");
    }

    let minutes = seconds / 60;
    let seconds = seconds % 60;
    if minutes < 60 {
        return format!("{minutes}m {seconds}s");
    }

    let hours = minutes / 60;
    let minutes = minutes % 60;
    format!("{hours}h {minutes}m")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeError, RuntimeStatus, RuntimeUsage};
    use crate::types::{FunctionCall, ToolCall, ToolCallKind};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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
        assert!(app.follow_tail);
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
        assert!(app.transcript[0].text.starts_with("Read ? finished"));
    }

    #[test]
    fn usage_stays_out_of_transcript_and_errors_become_rows() {
        let mut app = TuiApp::new();

        app.apply_runtime_event(RuntimeEvent::UsageUpdated {
            usage: RuntimeUsage {
                prompt_tokens: 7,
                completion_tokens: 11,
                total_tokens: 18,
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
    fn pending_work_row_refreshes_with_elapsed_hint() {
        let mut app = TuiApp::new();
        app.transcript.push(TranscriptItem {
            kind: TranscriptKind::System,
            text: "working for 0s - Esc to interrupt".to_string(),
        });
        app.pending_work_row = Some(0);
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(65));

        app.refresh_pending_work();

        assert_eq!(
            app.transcript[0].text,
            "working for 1m 5s - Esc to interrupt"
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
        app.status = Some(RuntimeStatus {
            session_id: "session".to_string(),
            model: "model".to_string(),
            messages_count: 1,
            active: false,
            total_input_tokens: 0,
            total_output_tokens: 0,
            cost_usd: None,
        });

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
        let status = status_line(&app);
        assert!(!status.contains("Enter submit"));
        assert!(!status.contains("\\ then Enter newline"));
        assert!(!status.contains("Esc exit"));
        assert!(!status.starts_with("idle |"));
        assert!(!status.starts_with("active |"));
        assert!(!status.starts_with("permission |"));
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
}
