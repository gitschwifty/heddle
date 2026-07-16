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
    HeddleRuntime, RuntimeConfig, RuntimeError, RuntimeEvent, RuntimePermissionRequest,
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
            RuntimeCommand::ClearContext => match runtime.clear_context() {
                Ok(()) => {
                    let _ = event_tx.send(RuntimeUpdate::Status(runtime.status(false)));
                }
                Err(error) => {
                    let _ = event_tx.send(RuntimeUpdate::Event(RuntimeEvent::Error {
                        error: RuntimeError {
                            code: "clear_context_failed".to_string(),
                            message: error.to_string(),
                            retryable: false,
                            provider: None,
                            details: None,
                        },
                    }));
                }
            },
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
    ClearContext,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTurn {
    items: Vec<TurnTranscriptItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnTranscriptItem {
    Row(TranscriptItem),
    Tool(ToolTranscript),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolTranscript {
    id: String,
    name: String,
    arguments: String,
    result: Option<String>,
    state: ToolState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolState {
    Running,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TranscriptLocation {
    turn: usize,
    item: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlashCommand {
    Clear,
    Status,
    Help,
    Quit,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewportState {
    scroll_top: usize,
    content_height: usize,
    viewport_height: usize,
    follow_tail: bool,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            scroll_top: 0,
            content_height: 0,
            viewport_height: 0,
            follow_tail: true,
        }
    }
}

impl ViewportState {
    fn max_scroll(&self) -> usize {
        self.content_height.saturating_sub(self.viewport_height)
    }

    fn set_content_height(&mut self, height: usize) {
        self.content_height = height;
        self.clamp_scroll();
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height;
        self.clamp_scroll();
    }

    fn scroll_up(&mut self, lines: usize) {
        let current = if self.follow_tail {
            self.max_scroll()
        } else {
            self.scroll_top
        };
        self.follow_tail = false;
        self.scroll_top = current.saturating_sub(lines);
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_top = self.scroll_top.saturating_add(lines).min(self.max_scroll());
        self.follow_tail = self.scroll_top == self.max_scroll();
    }

    fn jump_to_bottom(&mut self) {
        self.follow_tail = true;
        self.scroll_top = self.max_scroll();
    }

    fn on_new_output(&mut self) {
        self.clamp_scroll();
    }

    fn on_submit_prompt(&mut self) {
        self.jump_to_bottom();
    }

    fn clamp_scroll(&mut self) {
        let max_scroll = self.max_scroll();
        if self.follow_tail {
            self.scroll_top = max_scroll;
        } else {
            self.scroll_top = self.scroll_top.min(max_scroll);
        }
    }
}

#[derive(Debug, Default)]
struct TuiApp {
    input: InputBuffer,
    turns: Vec<TranscriptTurn>,
    transcript: Vec<TranscriptItem>,
    tool_rows: HashMap<String, TranscriptLocation>,
    active_assistant: Option<usize>,
    active_assistant_location: Option<TranscriptLocation>,
    pending_work_row: Option<usize>,
    pending_work_location: Option<TranscriptLocation>,
    turn_started_at: Option<Instant>,
    viewport: ViewportState,
    status: Option<RuntimeStatus>,
    last_turn_status: Option<TurnStatus>,
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
                } else if !self.viewport.follow_tail {
                    self.viewport.jump_to_bottom();
                } else {
                    return Ok(true);
                }
            }
            (KeyCode::PageUp, _) => {
                self.viewport.scroll_up(5);
            }
            (KeyCode::PageDown, _) => {
                self.viewport.scroll_down(5);
            }
            (KeyCode::End, KeyModifiers::CONTROL) => {
                self.viewport.jump_to_bottom();
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
            // Crossterm only reports Shift-Enter when the terminal sends a distinct
            // key event. Ghostty/tmux combinations may collapse it to Enter, so the
            // portable multiline path remains backslash followed by Enter.
            (KeyCode::Enter, modifiers) if modifiers.contains(KeyModifiers::SHIFT) => {
                if !self.active {
                    self.input.insert_newline();
                }
            }
            (KeyCode::Enter, _) if !self.active && self.input.consume_trailing_backslash() => {
                self.input.insert_newline();
            }
            (KeyCode::Enter, _) => {
                self.submit(command_tx, turn_counter).await?;
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) if !self.active => {
                self.input.insert_char(c);
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_mouse(&mut self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::ScrollUp => {
                self.viewport.scroll_up(3);
            }
            MouseEventKind::ScrollDown => {
                self.viewport.scroll_down(3);
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

        if message.starts_with('/') {
            self.input.clear();
            self.apply_slash_command(parse_tui_slash_command(&message), command_tx)
                .await?;
            return Ok(());
        }

        self.input.clear();
        let turn = self.turns.len();
        let pending_location = TranscriptLocation { turn, item: 1 };
        self.turns.push(TranscriptTurn {
            items: vec![
                TurnTranscriptItem::Row(TranscriptItem {
                    kind: TranscriptKind::User,
                    text: message.clone(),
                }),
                TurnTranscriptItem::Row(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "working for 0s - Esc to interrupt".to_string(),
                }),
            ],
        });
        self.pending_work_location = Some(pending_location);
        self.refresh_transcript_cache();
        self.pending_work_row = self.flat_index_for_location(pending_location);
        self.turn_started_at = Some(Instant::now());
        self.viewport.on_submit_prompt();

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
                self.viewport.on_new_output();
            }
            RuntimeEvent::ToolStarted { name, call } => {
                self.clear_pending_work();
                self.active_assistant = None;
                self.active_assistant_location = None;
                let location = self.push_tool(call.id.clone(), name, call.function.arguments);
                self.tool_rows.insert(call.id, location);
                self.viewport.on_new_output();
            }
            RuntimeEvent::ToolFinished { name, result, call } => {
                self.clear_pending_work();
                self.active_assistant = None;
                self.active_assistant_location = None;
                if let Some(location) = self.tool_rows.remove(&call.id) {
                    if let Some(TurnTranscriptItem::Tool(tool)) = self.turn_item_mut(location) {
                        tool.name = name;
                        tool.arguments = call.function.arguments;
                        tool.result = Some(result);
                        tool.state = ToolState::Finished;
                        self.refresh_transcript_cache();
                    }
                } else {
                    let location = self.push_tool(call.id, name, call.function.arguments);
                    if let Some(TurnTranscriptItem::Tool(tool)) = self.turn_item_mut(location) {
                        tool.result = Some(result);
                        tool.state = ToolState::Finished;
                        self.refresh_transcript_cache();
                    }
                }
                self.viewport.on_new_output();
            }
            RuntimeEvent::UsageUpdated { .. } => {}
            RuntimeEvent::Error { error } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Error,
                    text: error.message,
                });
                self.viewport.on_new_output();
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
                self.viewport.on_new_output();
            }
            RuntimeEvent::PermissionDenied { name, reason, .. } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Error,
                    text: format!("permission denied: {name}: {reason}"),
                });
                self.viewport.on_new_output();
            }
            RuntimeEvent::PlanCompleted { plan } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!("plan completed\n{plan}"),
                });
                self.viewport.on_new_output();
            }
            RuntimeEvent::ContextPruned {
                messages_pruned, ..
            } => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!("context pruned: {messages_pruned} messages"),
                });
                self.viewport.on_new_output();
            }
            RuntimeEvent::ContextCompacted => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "context compacted".to_string(),
                });
                self.viewport.on_new_output();
            }
            RuntimeEvent::ContextHandoff => {
                self.clear_pending_work();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "context handoff".to_string(),
                });
                self.viewport.on_new_output();
            }
            RuntimeEvent::AssistantMessage { message, .. } => {
                if let Some(content) = message.content {
                    self.clear_pending_work();
                    self.set_assistant_message(content);
                    self.viewport.on_new_output();
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
        self.active_assistant_location = None;
        self.clear_pending_work();
        self.permission_prompt = None;
        self.permission_prompt_view = None;
        let status = outcome.status;
        self.last_turn_status = Some(status.clone());
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
        self.viewport.on_new_output();
    }

    fn append_assistant_delta(&mut self, text: &str) {
        if self.turns.is_empty() && !self.transcript.is_empty() {
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
            return;
        }

        if let Some(location) = self.active_assistant_location {
            if let Some(TurnTranscriptItem::Row(item)) = self.turn_item_mut(location) {
                item.text.push_str(text);
                self.refresh_transcript_cache();
                return;
            }
        }

        if let Some(row) = self.active_assistant {
            if let Some(item) = self.transcript.get_mut(row) {
                item.text.push_str(text);
                return;
            }
        }

        let location = self.push_turn_row(TranscriptKind::Assistant, String::new());
        self.active_assistant_location = Some(location);
        self.active_assistant = self.flat_index_for_location(location);
        if let Some(TurnTranscriptItem::Row(item)) = self.turn_item_mut(location) {
            item.text.push_str(text);
        }
        self.refresh_transcript_cache();
        self.active_assistant = self.flat_index_for_location(location);
    }

    fn set_assistant_message(&mut self, text: String) {
        if self.turns.is_empty() && !self.transcript.is_empty() {
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
            return;
        }

        if let Some(location) = self.active_assistant_location {
            if let Some(TurnTranscriptItem::Row(item)) = self.turn_item_mut(location) {
                if item.text.is_empty() {
                    item.text = text;
                    self.refresh_transcript_cache();
                }
                return;
            }
        }

        if let Some(row) = self.active_assistant {
            if let Some(item) = self.transcript.get_mut(row) {
                if item.text.is_empty() {
                    item.text = text;
                }
                return;
            }
        }

        let location = self.push_turn_row(TranscriptKind::Assistant, text);
        self.active_assistant_location = Some(location);
        self.active_assistant = self.flat_index_for_location(location);
        self.refresh_transcript_cache();
        self.active_assistant = self.flat_index_for_location(location);
    }

    fn push_turn_row(&mut self, kind: TranscriptKind, text: String) -> TranscriptLocation {
        let turn = self.current_turn_index();
        let item = self.turns[turn].items.len();
        self.turns[turn]
            .items
            .push(TurnTranscriptItem::Row(TranscriptItem { kind, text }));
        let location = TranscriptLocation { turn, item };
        self.refresh_transcript_cache();
        location
    }

    fn push_tool(&mut self, id: String, name: String, arguments: String) -> TranscriptLocation {
        let turn = self.current_turn_index();
        let item = self.turns[turn].items.len();
        self.turns[turn]
            .items
            .push(TurnTranscriptItem::Tool(ToolTranscript {
                id,
                name,
                arguments,
                result: None,
                state: ToolState::Running,
            }));
        let location = TranscriptLocation { turn, item };
        self.refresh_transcript_cache();
        location
    }

    fn current_turn_index(&mut self) -> usize {
        if self.turns.is_empty() {
            self.turns.push(TranscriptTurn { items: Vec::new() });
        }
        self.turns.len() - 1
    }

    fn turn_item_mut(&mut self, location: TranscriptLocation) -> Option<&mut TurnTranscriptItem> {
        self.turns
            .get_mut(location.turn)?
            .items
            .get_mut(location.item)
    }

    fn refresh_transcript_cache(&mut self) {
        self.transcript = flatten_transcript_turns(&self.turns);
        self.pending_work_row = self
            .pending_work_location
            .and_then(|location| self.flat_index_for_location(location));
        self.active_assistant = self
            .active_assistant_location
            .and_then(|location| self.flat_index_for_location(location));
    }

    fn flat_index_for_location(&self, target: TranscriptLocation) -> Option<usize> {
        let mut row = 0;
        for (turn_idx, turn) in self.turns.iter().enumerate() {
            let mut item_idx = 0;
            while item_idx < turn.items.len() {
                match &turn.items[item_idx] {
                    TurnTranscriptItem::Row(_) => {
                        if target
                            == (TranscriptLocation {
                                turn: turn_idx,
                                item: item_idx,
                            })
                        {
                            return Some(row);
                        }
                        row += 1;
                        item_idx += 1;
                    }
                    TurnTranscriptItem::Tool(tool) if is_exploration_tool(&tool.name) => {
                        let group_start = item_idx;
                        while item_idx < turn.items.len() {
                            match &turn.items[item_idx] {
                                TurnTranscriptItem::Tool(tool)
                                    if is_exploration_tool(&tool.name) =>
                                {
                                    if target
                                        == (TranscriptLocation {
                                            turn: turn_idx,
                                            item: item_idx,
                                        })
                                    {
                                        return Some(row);
                                    }
                                    item_idx += 1;
                                }
                                _ => break,
                            }
                        }
                        if item_idx == group_start {
                            item_idx += 1;
                        }
                        row += 1;
                    }
                    TurnTranscriptItem::Tool(_) => {
                        if target
                            == (TranscriptLocation {
                                turn: turn_idx,
                                item: item_idx,
                            })
                        {
                            return Some(row);
                        }
                        row += 1;
                        item_idx += 1;
                    }
                }
            }
        }
        None
    }

    fn set_permission_prompt(&mut self, prompt: PermissionPrompt) {
        self.clear_pending_work();
        self.permission_prompt_view = Some(PermissionPromptView::from_request(&prompt.request));
        self.permission_prompt = Some(prompt);
        self.viewport.jump_to_bottom();
    }

    fn answer_permission_prompt(&mut self, response: RuntimePermissionResponse) {
        if let Some(prompt) = self.permission_prompt.take() {
            let _ = prompt.respond_to.send(response);
        }
        self.permission_prompt_view = None;
    }

    fn clear_pending_work(&mut self) {
        if let Some(location) = self.pending_work_location.take() {
            if let Some(turn) = self.turns.get_mut(location.turn) {
                if location.item < turn.items.len() {
                    turn.items.remove(location.item);
                    self.tool_rows.retain(|_, tool_location| {
                        if tool_location.turn == location.turn && tool_location.item > location.item
                        {
                            tool_location.item -= 1;
                        }
                        true
                    });
                    if let Some(active_location) = self.active_assistant_location.as_mut() {
                        if active_location.turn == location.turn
                            && active_location.item > location.item
                        {
                            active_location.item -= 1;
                        }
                    }
                }
            }
            self.refresh_transcript_cache();
            self.pending_work_row = None;
            return;
        }

        let Some(row) = self.pending_work_row.take() else {
            return;
        };
        if row < self.transcript.len() && self.transcript[row].kind == TranscriptKind::System {
            self.transcript.remove(row);
            if let Some(active_row) = self.active_assistant.as_mut() {
                if *active_row > row {
                    *active_row -= 1;
                }
            }
        }
    }

    fn refresh_pending_work(&mut self) {
        if let Some(location) = self.pending_work_location {
            let Some(started) = self.turn_started_at else {
                return;
            };
            if let Some(TurnTranscriptItem::Row(item)) = self.turn_item_mut(location) {
                item.text = format!(
                    "working for {} - Esc to interrupt",
                    format_duration(started.elapsed())
                );
                self.refresh_transcript_cache();
            }
            return;
        }

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
        self.push_turn_row(
            TranscriptKind::Divider,
            format!("Worked for {worked_for}{suffix}"),
        );
    }

    async fn apply_slash_command(
        &mut self,
        command: SlashCommand,
        command_tx: &mpsc::Sender<RuntimeCommand>,
    ) -> Result<()> {
        match command {
            SlashCommand::Clear => {
                self.clear_transcript_view();
                self.push_system_row("Context cleared.".to_string());
                command_tx.send(RuntimeCommand::ClearContext).await?;
            }
            SlashCommand::Status => self.push_system_row(tui_status_text(self.status.as_ref())),
            SlashCommand::Help => self.push_system_row(tui_help_text()),
            SlashCommand::Quit => self.should_quit = true,
            SlashCommand::Unknown(command) => self.push_system_row(format!(
                "unknown command: {command}. Type /help for available TUI commands."
            )),
        }
        self.viewport.jump_to_bottom();
        Ok(())
    }

    fn clear_transcript_view(&mut self) {
        self.turns.clear();
        self.transcript.clear();
        self.tool_rows.clear();
        self.active_assistant = None;
        self.active_assistant_location = None;
        self.pending_work_row = None;
        self.pending_work_location = None;
        self.turn_started_at = None;
        self.viewport = ViewportState::default();
    }

    fn push_system_row(&mut self, text: String) {
        self.turns.push(TranscriptTurn {
            items: vec![TurnTranscriptItem::Row(TranscriptItem {
                kind: TranscriptKind::System,
                text,
            })],
        });
        self.refresh_transcript_cache();
    }
}

fn parse_tui_slash_command(input: &str) -> SlashCommand {
    let token = input.split_whitespace().next().unwrap_or(input.trim());
    match token {
        "/clear" => SlashCommand::Clear,
        "/status" => SlashCommand::Status,
        "/help" => SlashCommand::Help,
        "/quit" | "/exit" => SlashCommand::Quit,
        other => SlashCommand::Unknown(other.to_string()),
    }
}

fn tui_help_text() -> String {
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

fn tui_status_text(status: Option<&RuntimeStatus>) -> String {
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
        status.model,
        status.messages_count,
        status.total_input_tokens,
        status.total_output_tokens,
        cost
    )
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

    fn visual_cursor(&self, width: u16) -> (usize, usize) {
        let inner_width = width.max(1) as usize;
        let row = self
            .lines
            .iter()
            .take(self.row)
            .map(|line| visual_line_count(line, inner_width) as usize)
            .sum::<usize>()
            + (self.col / inner_width);
        let col = self.col % inner_width;
        (row, col)
    }

    fn input_scroll(&self, width: u16, height: u16) -> usize {
        let visible_height = height.max(1) as usize;
        let (row, _) = self.visual_cursor(width);
        row.saturating_sub(visible_height.saturating_sub(1))
    }

    fn cursor_position(
        &self,
        origin: Position,
        width: u16,
        height: u16,
        scroll: usize,
    ) -> Position {
        let visible_height = height.max(1);
        let (row, col) = self.visual_cursor(width);
        let visible_row = row.saturating_sub(scroll);
        Position::new(
            origin.x.saturating_add(col as u16),
            origin
                .y
                .saturating_add((visible_row as u16).min(visible_height.saturating_sub(1))),
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
    app.viewport
        .set_content_height(transcript_lines.lines.len());
    app.viewport.set_viewport_height(chunks[0].height as usize);
    let scroll = app.viewport.scroll_top.min(u16::MAX as usize) as u16;
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

    let status = Paragraph::new(status_line(app, chunks[2].width));
    frame.render_widget(status, chunks[2]);
}

fn input_text(input: &InputBuffer, width: u16, scroll: usize) -> Text<'static> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusState {
    Idle,
    Active,
    Permission,
    Cancelling,
    Failed,
}

fn status_line(app: &TuiApp, width: u16) -> String {
    abbreviate(&full_status_line(app), width as usize)
}

fn full_status_line(app: &TuiApp) -> String {
    let Some(status) = &app.status else {
        return "initializing runtime".to_string();
    };

    let state = status_state(app);
    let state = match state {
        StatusState::Idle => "idle".to_string(),
        StatusState::Active => elapsed_state("active", app.turn_started_at),
        StatusState::Permission => elapsed_state("permission", app.turn_started_at),
        StatusState::Cancelling => elapsed_state("cancelling", app.turn_started_at),
        StatusState::Failed => "failed".to_string(),
    };
    let cost = status.cost_usd.map(format_cost).unwrap_or_default();
    let tool_count = visible_tool_count(app);
    format!(
        "{} | model: {} | msgs: {} | tools: {} | tokens: {}/{}{}",
        state,
        status.model,
        status.messages_count,
        tool_count,
        status.total_input_tokens,
        status.total_output_tokens,
        cost,
    )
}

fn status_state(app: &TuiApp) -> StatusState {
    if app
        .active_cancel
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return StatusState::Cancelling;
    }
    if app.permission_prompt_view.is_some() {
        return StatusState::Permission;
    }
    if app.active {
        return StatusState::Active;
    }
    if matches!(app.last_turn_status, Some(TurnStatus::Error)) {
        return StatusState::Failed;
    }
    StatusState::Idle
}

fn elapsed_state(label: &str, started_at: Option<Instant>) -> String {
    let elapsed = started_at
        .map(|started| format_duration(started.elapsed()))
        .unwrap_or_else(|| "0s".to_string());
    format!("{label} {elapsed}")
}

fn visible_tool_count(app: &TuiApp) -> usize {
    app.transcript
        .iter()
        .filter(|item| matches!(item.kind, TranscriptKind::Tool))
        .count()
}

fn flatten_transcript_turns(turns: &[TranscriptTurn]) -> Vec<TranscriptItem> {
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

fn is_exploration_tool(name: &str) -> bool {
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

fn format_cost(cost: f64) -> String {
    if cost == 0.0 {
        return " | $0.0000".to_string();
    }
    if cost.abs() < 0.0001 {
        return format!(" | ${cost:.6}");
    }
    format!(" | ${cost:.4}")
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
    use ratatui::layout::Rect;
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

    fn draw_screen(terminal: &mut Terminal<TestBackend>, app: &mut TuiApp) -> String {
        terminal.draw(|frame| draw(frame, app)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
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
        assert!(status.starts_with("idle |"));
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

        assert!(status.starts_with("idle |"));
        assert!(status.contains("msgs: 2"));
        assert!(status.contains("tools: 1"));
        assert!(!status.contains("msgs: 4"));
    }

    #[test]
    fn status_line_active_shows_turn_elapsed_time() {
        let mut app = TuiApp::new();
        app.status = Some(runtime_status(true, 1, 0, 0, None));
        app.active = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(65));

        let status = status_line(&app, 120);

        assert!(status.starts_with("active 1m 5s |"));
    }

    #[test]
    fn status_line_permission_state_takes_precedence_over_active() {
        let mut app = TuiApp::new();
        app.status = Some(runtime_status(true, 1, 0, 0, None));
        app.active = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(2));
        app.permission_prompt_view = Some(PermissionPromptView {
            name: "bash".to_string(),
            call_id: "call_1".to_string(),
            arguments: "{}".to_string(),
            reason: None,
        });

        let status = status_line(&app, 120);

        assert!(status.starts_with("permission 2s |"));
    }

    #[test]
    fn status_line_cancelling_state_takes_precedence_over_permission() {
        let mut app = TuiApp::new();
        app.status = Some(runtime_status(true, 1, 0, 0, None));
        app.active = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(3));
        let cancel = CancellationToken::new();
        cancel.cancel();
        app.active_cancel = Some(cancel);
        app.permission_prompt_view = Some(PermissionPromptView {
            name: "bash".to_string(),
            call_id: "call_1".to_string(),
            arguments: "{}".to_string(),
            reason: None,
        });

        let status = status_line(&app, 120);

        assert!(status.starts_with("cancelling 3s |"));
    }

    #[test]
    fn status_line_post_usage_shows_tokens_cost_and_failed_state() {
        let mut app = TuiApp::new();
        app.status = Some(runtime_status(false, 4, 1234, 56, Some(0.00125)));
        app.last_turn_status = Some(TurnStatus::Error);

        let status = status_line(&app, 120);

        assert!(status.starts_with("failed |"));
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
    fn slash_command_parser_recognizes_tui_local_commands() {
        assert_eq!(parse_tui_slash_command("/clear"), SlashCommand::Clear);
        assert_eq!(parse_tui_slash_command(" /status "), SlashCommand::Status);
        assert_eq!(parse_tui_slash_command("/help"), SlashCommand::Help);
        assert_eq!(parse_tui_slash_command("/quit"), SlashCommand::Quit);
        assert_eq!(parse_tui_slash_command("/exit"), SlashCommand::Quit);
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
