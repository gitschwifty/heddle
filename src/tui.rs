//! Ratatui frontend over the runtime facade.
//!
//! This first pass keeps terminal concerns local to the TUI and treats
//! `HeddleRuntime` as the turn execution boundary.

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::features::Mode;
use crate::runtime::{
    HeddleRuntime, RuntimeConfig, RuntimeEvent, RuntimeStatus, TurnOptions, TurnOutcome, TurnState,
    TurnStatus,
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

        terminal.draw(|frame| draw(frame, &app))?;

        if app.should_quit && !app.active {
            break;
        }

        if event::poll(Duration::from_millis(30))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };

            if app.handle_key(key, &command_tx, &mut turn_counter).await? {
                break;
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
                let _ = event_tx.send(RuntimeUpdate::Status(runtime.status(true)));
                let outcome = runtime
                    .send(message, TurnOptions { id, cancel }, |event| {
                        let _ = event_tx.send(RuntimeUpdate::Event(event));
                    })
                    .await;
                let _ = event_tx.send(RuntimeUpdate::Outcome(outcome));
                let _ = event_tx.send(RuntimeUpdate::Status(runtime.status(false)));
            }
        }
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
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
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptKind {
    User,
    Assistant,
    Tool,
    Error,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptItem {
    kind: TranscriptKind,
    text: String,
}

#[derive(Debug, Default)]
struct TuiApp {
    input: String,
    transcript: Vec<TranscriptItem>,
    tool_rows: HashMap<String, usize>,
    status: Option<RuntimeStatus>,
    active: bool,
    should_quit: bool,
    active_cancel: Option<CancellationToken>,
}

impl TuiApp {
    fn new() -> Self {
        Self::default()
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        command_tx: &mpsc::Sender<RuntimeCommand>,
        turn_counter: &mut u64,
    ) -> Result<bool> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if let Some(cancel) = &self.active_cancel {
                    cancel.cancel();
                } else {
                    return Ok(true);
                }
            }
            (KeyCode::Esc, _) => return Ok(true),
            (KeyCode::Backspace, _) => {
                self.input.pop();
            }
            (KeyCode::Enter, _) => {
                self.submit(command_tx, turn_counter).await?;
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                if !self.active {
                    self.input.push(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn submit(
        &mut self,
        command_tx: &mpsc::Sender<RuntimeCommand>,
        turn_counter: &mut u64,
    ) -> Result<()> {
        let message = self.input.trim().to_string();
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
        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::Assistant,
            text: String::new(),
        });

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
            RuntimeEvent::ContentDelta { text } => self.append_assistant_delta(&text),
            RuntimeEvent::ToolStarted { name, call } => {
                let row = self.transcript.len();
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Tool,
                    text: format!("{name} running"),
                });
                self.tool_rows.insert(call.id, row);
            }
            RuntimeEvent::ToolFinished { name, call, .. } => {
                if let Some(row) = self.tool_rows.remove(&call.id) {
                    self.transcript[row].text = format!("{name} finished");
                } else {
                    self.transcript.push(TranscriptItem {
                        kind: TranscriptKind::Tool,
                        text: format!("{name} finished"),
                    });
                }
            }
            RuntimeEvent::UsageUpdated { usage } => {
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!(
                        "tokens: {} in / {} out",
                        usage.prompt_tokens, usage.completion_tokens
                    ),
                });
            }
            RuntimeEvent::Error { error } => {
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Error,
                    text: error.message,
                });
            }
            RuntimeEvent::PermissionRequested { name, reason, .. } => {
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
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::Error,
                    text: format!("permission denied: {name}: {reason}"),
                });
            }
            RuntimeEvent::PlanCompleted { plan } => {
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!("plan completed\n{plan}"),
                });
            }
            RuntimeEvent::ContextPruned {
                messages_pruned, ..
            } => {
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: format!("context pruned: {messages_pruned} messages"),
                });
            }
            RuntimeEvent::ContextCompacted => {
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "context compacted".to_string(),
                });
            }
            RuntimeEvent::ContextHandoff => {
                self.transcript.push(TranscriptItem {
                    kind: TranscriptKind::System,
                    text: "context handoff".to_string(),
                });
            }
            RuntimeEvent::AssistantMessage { .. } => {}
        }
    }

    fn apply_turn_outcome(&mut self, outcome: TurnOutcome) {
        self.active = false;
        self.active_cancel = None;
        match outcome.status {
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
    }

    fn append_assistant_delta(&mut self, text: &str) {
        if let Some(item) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|item| item.kind == TranscriptKind::Assistant)
        {
            item.text.push_str(text);
            return;
        }
        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::Assistant,
            text: text.to_string(),
        });
    }
}

fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    let transcript_lines = transcript_text(app);
    let scroll = transcript_lines
        .lines
        .len()
        .saturating_sub(chunks[0].height.saturating_sub(2) as usize) as u16;
    let transcript = Paragraph::new(transcript_lines)
        .block(Block::default().title("Heddle").borders(Borders::ALL))
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(transcript, chunks[0]);

    let input_title = if app.active {
        "Prompt (turn active)"
    } else {
        "Prompt"
    };
    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().title(input_title).borders(Borders::ALL));
    frame.render_widget(input, chunks[1]);

    let status = Paragraph::new(status_line(app));
    frame.render_widget(status, chunks[2]);
}

fn transcript_text(app: &TuiApp) -> Text<'_> {
    if app.transcript.is_empty() {
        return Text::from(vec![Line::from(Span::styled(
            "Type a message and press Enter. Ctrl-C cancels an active turn; Esc exits.",
            Style::default().fg(Color::DarkGray),
        ))]);
    }

    let mut lines = Vec::new();
    for item in &app.transcript {
        let (label, style) = match item.kind {
            TranscriptKind::User => (
                "you",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            TranscriptKind::Assistant => (
                "assistant",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            TranscriptKind::Tool => ("tool", Style::default().fg(Color::Yellow)),
            TranscriptKind::Error => ("error", Style::default().fg(Color::Red)),
            TranscriptKind::System => ("status", Style::default().fg(Color::DarkGray)),
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{label}> "), style),
            Span::raw(item.text.as_str()),
        ]));
        lines.push(Line::raw(""));
    }
    Text::from(lines)
}

fn status_line(app: &TuiApp) -> String {
    let Some(status) = &app.status else {
        return "initializing runtime".to_string();
    };
    let activity = if app.active { "active" } else { "idle" };
    let cost = status
        .cost_usd
        .map(|cost| format!(" | ${cost:.4}"))
        .unwrap_or_default();
    format!(
        "{activity} | model: {} | messages: {} | tokens: {} in / {} out{cost}",
        status.model, status.messages_count, status.total_input_tokens, status.total_output_tokens,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeError, RuntimeUsage};
    use crate::types::{FunctionCall, ToolCall, ToolCallKind};

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

    #[test]
    fn content_delta_appends_to_latest_assistant_row() {
        let mut app = TuiApp::new();
        app.transcript.push(TranscriptItem {
            kind: TranscriptKind::Assistant,
            text: String::new(),
        });

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
        assert_eq!(app.transcript[0].text, "read_file finished");
    }

    #[test]
    fn usage_and_errors_become_transcript_rows() {
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

        assert_eq!(app.transcript.len(), 2);
        assert_eq!(app.transcript[0].text, "tokens: 7 in / 11 out");
        assert_eq!(app.transcript[1].kind, TranscriptKind::Error);
        assert_eq!(app.transcript[1].text, "bad response");
    }
}
