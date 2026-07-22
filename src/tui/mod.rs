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
#[cfg(test)]
use ratatui::layout::Position;
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

mod input;
mod render;
mod slash;
mod transcript;
mod viewport;

use input::InputBuffer;
use render::draw;
#[cfg(test)]
use render::{blank_fill, input_text, status_line, transcript_text};
use slash::{parse_tui_slash_command, tui_help_text, tui_status_text, SlashCommand};
use transcript::{
    abbreviate, display_model, divider_line, flatten_transcript_turns, format_cost,
    is_exploration_tool, summarize_arguments, wrap_message_lines, ToolState, ToolTranscript,
    TranscriptItem, TranscriptKind, TranscriptLocation, TranscriptTurn, TurnTranscriptItem,
};
use viewport::ViewportState;

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
                    text: "Working... 0s - Esc to interrupt".to_string(),
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
                self.restore_pending_work_if_active();
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
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::UsageUpdated { .. } => {}
            RuntimeEvent::RoutedModel { model } => {
                if let Some(status) = &mut self.status {
                    status.last_routed_model = Some(model);
                }
            }
            RuntimeEvent::Error { error } => {
                self.clear_pending_work();
                self.push_transcript_row(TranscriptKind::Error, error.message);
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::PermissionRequested { name, reason, .. } => {
                self.clear_pending_work();
                self.push_transcript_row(
                    TranscriptKind::System,
                    format!(
                        "permission requested: {name} {}",
                        reason.unwrap_or_default()
                    )
                    .trim()
                    .to_string(),
                );
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::PermissionDenied { name, reason, .. } => {
                self.clear_pending_work();
                self.push_transcript_row(
                    TranscriptKind::Error,
                    format!("permission denied: {name}: {reason}"),
                );
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::PlanCompleted { plan } => {
                self.clear_pending_work();
                self.push_transcript_row(TranscriptKind::System, format!("plan completed\n{plan}"));
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::ContextPruned {
                messages_pruned, ..
            } => {
                self.clear_pending_work();
                self.push_transcript_row(
                    TranscriptKind::System,
                    format!("context pruned: {messages_pruned} messages"),
                );
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::ContextCompacted => {
                self.clear_pending_work();
                self.push_transcript_row(TranscriptKind::System, "context compacted".to_string());
                self.restore_pending_work_if_active();
                self.viewport.on_new_output();
            }
            RuntimeEvent::ContextHandoff => {
                self.clear_pending_work();
                self.push_transcript_row(TranscriptKind::System, "context handoff".to_string());
                self.restore_pending_work_if_active();
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
            TurnStatus::Cancelled => {
                self.push_transcript_row(TranscriptKind::System, "turn cancelled".to_string())
            }
            TurnStatus::Error => {
                if let Some(error) = outcome.error {
                    if !self.current_turn_has_error(&error.message) {
                        self.push_transcript_row(TranscriptKind::Error, error.message);
                    }
                }
            }
        }
        self.push_turn_footer(&status, &worked_for);
        self.viewport.on_new_output();
    }

    fn append_assistant_delta(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if text.trim().is_empty()
            && self.active_assistant.is_none()
            && self.active_assistant_location.is_none()
        {
            return;
        }

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
        if text.trim().is_empty() {
            return;
        }

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

    fn push_transcript_row(&mut self, kind: TranscriptKind, text: String) {
        if self.turns.is_empty() && !self.transcript.is_empty() {
            self.transcript.push(TranscriptItem { kind, text });
            return;
        }

        self.push_turn_row(kind, text);
    }

    fn current_turn_has_error(&self, message: &str) -> bool {
        self.turns.last().is_some_and(|turn| {
            turn.items.iter().any(|item| {
                matches!(
                    item,
                    TurnTranscriptItem::Row(TranscriptItem {
                        kind: TranscriptKind::Error,
                        text
                    }) if text == message
                )
            })
        })
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
                    "Working... {} - Esc to interrupt",
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
                "Working... {} - Esc to interrupt",
                format_duration(started.elapsed())
            );
        }
    }

    fn restore_pending_work_if_active(&mut self) {
        if !self.active || self.turn_started_at.is_none() || self.pending_work_location.is_some() {
            return;
        }

        let location = self.push_turn_row(TranscriptKind::System, self.pending_work_text());
        self.pending_work_location = Some(location);
        self.pending_work_row = self.flat_index_for_location(location);
    }

    fn pending_work_text(&self) -> String {
        let elapsed = self
            .turn_started_at
            .map(|started| format_duration(started.elapsed()))
            .unwrap_or_else(|| "0s".to_string());
        format!("Working... {elapsed} - Esc to interrupt")
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
mod tests;
