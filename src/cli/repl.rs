//! Interactive REPL loop. Handles `/`-commands, `!`/`!!` shell prefixes, and
//! `@`-mentions; pipes streaming agent output to stdout.
//!
//! The TS version (`ts-src/cli/index.ts`) is callback-driven via Node's
//! readline; here we use rustyline synchronously inside the async runtime via
//! `spawn_blocking`.

use std::io::{Read, Write};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use futures::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Config, Editor};
use serde_json::json;

use crate::agent::loop_::{
    run_agent_loop_streaming, AgentLoopOptions, PermissionResolver, PermissionResponse,
};
use crate::agent::types::AgentEvent;
use crate::cli::completer::MentionCompleter;
use crate::cli::mentions::{build_mention_message, resolve_mentions};
use crate::cli::oneshot::{format_oneshot_output, run_oneshot, OneshotOptions};
use crate::cli::shell::{format_shell_for_context, print_shell_result, run_shell};
use crate::commands::builtins::create_builtin_commands;
use crate::commands::loader::load_custom_commands;
use crate::commands::registry::CommandRegistry;
use crate::commands::types::CommandContext;
use crate::config::loader::ApprovalMode;
use crate::config::paths::get_project_dir;
use crate::context::compaction::{compact_context, should_compact, CompactionConfig};
use crate::context::pruning::{prune_tool_results, PruningOptions};
use crate::history::writer::{append_history_entry, ContentType, HistoryEntry};
use crate::hooks::types::{HookContext, HookEvent};
use crate::permissions::checker::read_only_tool_filter;
use crate::session::jsonl::{append_context_marker, append_message};
use crate::session::setup::{create_session, SessionOptions};
use crate::tools::ask_user::create_ask_user_tool;
use crate::types::{Message, ToolCall, ToolMessage, UserMessage};
use crate::usage::writer::{write_usage_record, UsageRecord};

fn build_permission_resolver() -> PermissionResolver {
    Arc::new(move |name: String, _call: ToolCall| {
        Box::pin(async move {
            // Read a single answer line from stdin.
            print!("  Allow {name}? [y/n/always] ");
            let _ = std::io::stdout().flush();
            let mut buf = String::new();
            if std::io::stdin().read_line(&mut buf).is_err() {
                return PermissionResponse::Deny;
            }
            let trimmed = buf.trim().to_lowercase();
            match trimmed.as_str() {
                "y" | "yes" => PermissionResponse::Allow,
                "always" | "a" => PermissionResponse::Always,
                _ => PermissionResponse::Deny,
            }
        })
    })
}

fn print_help() {
    println!(
        "heddle — interactive LLM CLI

Usage:
  heddle [FLAGS]
  heddle -p <PROMPT> [FLAGS]      run a single prompt and exit
  echo <PROMPT> | heddle [FLAGS]  read prompt from stdin

Session flags:
  --resume <ID|NAME>     resume an existing session by id or name
  --fork <ID|NAME>       fork from an existing session into a new one

One-shot flags:
  -p, --prompt <TEXT>    run a single prompt non-interactively
  --json                 emit JSON output (oneshot only)
  --quiet                suppress diagnostics (oneshot only)
  --agent <NAME>         run under the named agent persona

Other:
  --interactive          force interactive mode even with non-TTY stdin
  -h, --help             print this help

Use /help inside the REPL to list slash commands."
    );
}

fn flag_value(args: &[String], names: &[&str]) -> Option<String> {
    for (i, a) in args.iter().enumerate() {
        if names.contains(&a.as_str()) {
            return args.get(i + 1).cloned();
        }
        for name in names {
            let prefix = format!("{name}=");
            if let Some(rest) = a.strip_prefix(&prefix) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

pub async fn start_cli() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let resume = flag_value(&args, &["--resume"]);
    let fork = flag_value(&args, &["--fork"]);
    if resume.is_some() && fork.is_some() {
        eprintln!("Error: --resume and --fork are mutually exclusive");
        std::process::exit(1);
    }
    let session_opts_base = SessionOptions {
        resume: resume.clone(),
        fork: fork.clone(),
        ..SessionOptions::default()
    };

    // -p / --prompt non-interactive mode
    let oneshot_idx = args
        .iter()
        .position(|a| a == "-p")
        .or_else(|| args.iter().position(|a| a == "--prompt"));
    if let Some(idx) = oneshot_idx {
        let prompt = match args.get(idx + 1) {
            Some(p) => p.clone(),
            None => {
                eprintln!("Error: -p requires a prompt argument");
                std::process::exit(1);
            }
        };
        let json_flag = args.iter().any(|a| a == "--json");
        let quiet = args.iter().any(|a| a == "--quiet");
        let agent = flag_value(&args, &["--agent"]);
        let opts = OneshotOptions {
            prompt: prompt.clone(),
            json: json_flag,
            quiet,
            agent,
            session_options: Some(session_opts_base.clone()),
        };
        let result = run_oneshot(opts.clone()).await;
        println!("{}", format_oneshot_output(&result, &opts));
        std::process::exit(result.exit_code);
    }

    // Pipe mode: stdin not a TTY → read all and run as oneshot.
    if !atty::is(atty::Stream::Stdin) && !args.iter().any(|a| a == "--interactive") {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).ok();
        let prompt = buf.trim().to_string();
        if !prompt.is_empty() {
            let opts = OneshotOptions {
                prompt: prompt.clone(),
                session_options: Some(session_opts_base.clone()),
                ..Default::default()
            };
            let result = run_oneshot(opts.clone()).await;
            println!("{}", format_oneshot_output(&result, &opts));
            std::process::exit(result.exit_code);
        }
    }

    let mut ctx = match create_session(session_opts_base).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let mut active_provider = ctx.provider.clone();

    let cwd = std::env::current_dir().unwrap_or_default();
    let mut editor: Editor<MentionCompleter, DefaultHistory> =
        Editor::with_config(Config::builder().build())?;
    editor.set_helper(Some(MentionCompleter::new(cwd.clone())));

    let mut command_registry = CommandRegistry::new();
    for cmd in create_builtin_commands() {
        command_registry.register(cmd);
    }
    for cmd in load_custom_commands(ctx.discovery.as_ref()) {
        command_registry.register(cmd);
    }

    // ask_user tool
    let _ = ctx.registry.register(create_ask_user_tool(Arc::new(
        |question: String, options: Option<Vec<String>>| {
            Box::pin(async move {
                let mut display = format!("\n  [ask_user] {question}");
                if let Some(opts) = &options {
                    if !opts.is_empty() {
                        display.push_str(&format!("\n  Options: {}", opts.join(", ")));
                    }
                }
                println!("{display}");
                print!("  your answer> ");
                let _ = std::io::stdout().flush();
                let mut buf = String::new();
                let _ = std::io::stdin().read_line(&mut buf);
                let answer = buf.trim();
                if answer.is_empty() {
                    "(no response)".to_string()
                } else {
                    answer.to_string()
                }
            })
        },
    )));

    let mut loop_options = AgentLoopOptions::default();
    if let Some(t) = ctx.config.doom_loop_threshold {
        loop_options.doom_loop_threshold = Some(t);
    }
    if let Some(checker) = &ctx.permission_checker {
        loop_options.permission_checker = Some(checker.clone());
        loop_options.permission_resolver = Some(build_permission_resolver());
    }
    if matches!(ctx.config.approval_mode, Some(ApprovalMode::Plan)) {
        loop_options.tool_filter = Some(Arc::new(read_only_tool_filter));
        loop_options.plan_mode = true;
    }
    if let Some(runner) = &ctx.hooks_runner {
        loop_options.hooks_runner = Some(runner.clone());
    }

    if let Some(runner) = &ctx.hooks_runner {
        let _ = runner
            .run(HookEvent::SessionStart, HookContext::default())
            .await;
    }

    println!("Heddle CLI — model: {}", ctx.config.model);
    println!("Session: {}", ctx.session_file.display());
    println!("Type \"exit\" or \"quit\" to stop.\n");

    loop {
        let line = editor.readline("you> ");
        let input = match line {
            Ok(s) => s,
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break,
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        };
        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(trimmed);

        if trimmed == "exit" || trimmed == "quit" {
            if let Some(runner) = &ctx.hooks_runner {
                let _ = runner
                    .run(HookEvent::SessionEnd, HookContext::default())
                    .await;
            }
            if let Some(mc) = &ctx.metrics_collector {
                let metrics = mc.lock().metrics();
                let cost = ctx.cost_tracker.lock().total_cost();
                let record = UsageRecord {
                    session_id: ctx.session_id.clone(),
                    project: std::env::current_dir()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    created: ctx.session_start_time.to_rfc3339(),
                    ended: Utc::now().to_rfc3339(),
                    duration_ms: (Utc::now() - ctx.session_start_time).num_milliseconds() as u64,
                    metrics,
                    cost_usd: cost,
                };
                let _ = write_usage_record(&record, &get_project_dir(None));
            }
            println!("Goodbye!");
            break;
        }

        // !! shell + inject
        if let Some(rest) = trimmed.strip_prefix("!!") {
            let cmd = rest.trim();
            if cmd.is_empty() {
                continue;
            }
            let result = run_shell(cmd).await;
            print_shell_result(&result);
            let msg = format_shell_for_context(cmd, &result);
            ctx.messages.push(msg.clone());
            let _ = append_message(&ctx.session_file, &msg);
            continue;
        }
        // ! shell only
        if let Some(rest) = trimmed.strip_prefix('!') {
            let cmd = rest.trim();
            if cmd.is_empty() {
                continue;
            }
            let result = run_shell(cmd).await;
            print_shell_result(&result);
            continue;
        }
        // / commands
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args_str) = match rest.find(' ') {
                Some(i) => (&rest[..i], &rest[i + 1..]),
                None => (rest, ""),
            };
            if let Some(cmd) = command_registry.get(name) {
                let mut cmd_ctx = CommandContext {
                    config: &mut ctx.config,
                    messages: &mut ctx.messages,
                    registry: &ctx.registry,
                    cost_tracker: ctx.cost_tracker.clone(),
                    session_file: ctx.session_file.clone(),
                    session_id: ctx.session_id.clone(),
                    provider: active_provider.clone(),
                    weak_provider: ctx.weak_provider.clone(),
                    editor_provider: ctx.editor_provider.clone(),
                    discovery: ctx.discovery.as_ref(),
                    agent_definitions: &ctx.agent_definitions,
                    paste_cache: ctx.paste_cache.clone(),
                };
                let new_model = (cmd.execute)(args_str, &mut cmd_ctx).await;
                if let Some(model) = new_model {
                    ctx.config.model = model.clone();
                    active_provider = ctx.provider.with(json!({ "model": model }));
                }
            } else {
                let suggestion = command_registry.suggest(name);
                match suggestion {
                    Some(s) => println!("Unknown command: /{name}. Did you mean /{s}?"),
                    None => {
                        println!("Unknown command: /{name}. Type /help for available commands.")
                    }
                }
            }
            continue;
        }

        // @ mentions
        let mentions = resolve_mentions(trimmed, &cwd).await;
        for f in &mentions.injected_files {
            println!("  [injected] {} ({} lines)", f.path.display(), f.lines);
        }
        for err in &mentions.errors {
            println!("  [mention] {err}");
        }
        let content = if !mentions.injected_files.is_empty() {
            build_mention_message(trimmed, &mentions.injected_files)
        } else {
            trimmed.to_string()
        };
        let user_msg = Message::User(UserMessage { content });
        ctx.messages.push(user_msg.clone());
        let _ = append_message(&ctx.session_file, &user_msg);

        if let Some(mc) = &ctx.metrics_collector {
            mc.lock().on_user_message();
        }

        if ctx.features.history {
            let _ = append_history_entry(&HistoryEntry {
                timestamp: Utc::now().to_rfc3339(),
                session_id: ctx.session_id.clone(),
                project: std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                message_preview: trimmed.chars().take(200).collect(),
                content_type: if !mentions.injected_files.is_empty() {
                    ContentType::Mention
                } else {
                    ContentType::Text
                },
            });
        }

        // Pre-prompt hooks
        if let Some(runner) = &ctx.hooks_runner {
            let results = runner
                .run(
                    HookEvent::PrePrompt,
                    HookContext {
                        user_input: Some(trimmed.to_string()),
                        ..Default::default()
                    },
                )
                .await;
            if let Some(blocked) = results.iter().find(|r| r.blocked) {
                println!(
                    "\n  [hook blocked] {}",
                    blocked.reason.as_deref().unwrap_or("hook rejected")
                );
                continue;
            }
        }

        let mut needs_newline = false;
        let mut stream = run_agent_loop_streaming(
            active_provider.clone(),
            ctx.registry.clone(),
            &mut ctx.messages,
            loop_options.clone(),
        );
        while let Some(event) = stream.next().await {
            match event {
                AgentEvent::ContentDelta { text } => {
                    if !needs_newline {
                        print!("\nassistant> ");
                        needs_newline = true;
                    }
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                AgentEvent::AssistantMessage { message } => {
                    if needs_newline {
                        println!("\n");
                        needs_newline = false;
                    }
                    let _ = append_message(&ctx.session_file, &Message::Assistant(message.clone()));
                    if let Some(mc) = &ctx.metrics_collector {
                        mc.lock().on_assistant_message();
                    }
                }
                AgentEvent::ToolStart { name, call } => {
                    println!("  [tool] {name}({})", call.function.arguments);
                    if let Some(mc) = &ctx.metrics_collector {
                        mc.lock().on_tool_call(&name);
                    }
                }
                AgentEvent::ToolEnd { result, call, .. } => {
                    let preview = if result.len() > 200 {
                        format!("{}...", &result[..200])
                    } else {
                        result.clone()
                    };
                    println!("  [result] {preview}");
                    let _ = append_message(
                        &ctx.session_file,
                        &Message::Tool(ToolMessage {
                            tool_call_id: call.id.clone(),
                            content: result,
                        }),
                    );
                }
                AgentEvent::PermissionRequest { name, reason, .. } => {
                    println!(
                        "  [permission] {name} requires approval: {}",
                        reason.unwrap_or_default()
                    );
                }
                AgentEvent::PermissionDenied { name, reason, .. } => {
                    println!("  [denied] {name}: {reason}");
                }
                AgentEvent::PlanComplete { plan } => {
                    println!("\n  [plan complete]\n{plan}");
                }
                AgentEvent::Usage { usage } => {
                    ctx.cost_tracker.lock().add_usage(&usage);
                    if let Some(mc) = &ctx.metrics_collector {
                        mc.lock()
                            .on_usage(usage.prompt_tokens, usage.completion_tokens);
                    }
                    let tracker = ctx.cost_tracker.lock();
                    let cost_str = tracker
                        .total_cost()
                        .map(|c| format!(" | cost: ${c:.4}"))
                        .unwrap_or_default();
                    println!(
                        "  [tokens: {} in / {} out{cost_str}]",
                        tracker.total_input_tokens(),
                        tracker.total_output_tokens()
                    );
                }
                AgentEvent::LoopDetected { count } => {
                    eprintln!(
                        "\n  [warning] Doom loop detected: {count} identical tool call iterations. Stopping."
                    );
                }
                AgentEvent::Error { message } => {
                    eprintln!("  [error] {message}");
                    if let Some(mc) = &ctx.metrics_collector {
                        mc.lock().on_provider_error();
                    }
                }
                _ => {}
            }
        }
        drop(stream);

        let prune_result = prune_tool_results(&mut ctx.messages, &PruningOptions::default());
        if prune_result.messages_pruned > 0 {
            let _ = append_context_marker(
                &ctx.session_file,
                &json!({
                    "type": "context_prune",
                    "messages_pruned": prune_result.messages_pruned,
                    "tokens_before": prune_result.tokens_before,
                    "tokens_after": prune_result.tokens_after,
                    "timestamp": Utc::now().to_rfc3339(),
                }),
            );
        }

        if let Some(weak) = &ctx.weak_provider {
            let model_limit = ctx.config.max_tokens.unwrap_or(128_000);
            if should_compact(&ctx.messages, model_limit, CompactionConfig::default()) {
                if let Ok(stats) = compact_context(
                    &mut ctx.messages,
                    weak.as_ref(),
                    model_limit,
                    CompactionConfig::default(),
                )
                .await
                {
                    if stats.messages_removed > 0 {
                        let _ = append_context_marker(
                            &ctx.session_file,
                            &json!({
                                "type": "context_compaction",
                                "messages_removed": stats.messages_removed,
                                "tokens_before": stats.tokens_before,
                                "tokens_after": stats.tokens_after,
                                "timestamp": Utc::now().to_rfc3339(),
                            }),
                        );
                    }
                }
            }
        }
        if let Some(runner) = &ctx.hooks_runner {
            let _ = runner
                .run(HookEvent::PostTurn, HookContext::default())
                .await;
        }
    }
    Ok(())
}
