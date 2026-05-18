//! Built-in slash commands shown by `/help`.

use std::sync::Arc;

use chrono::Utc;
use serde_json::json;

use super::types::{CommandContext, SlashCommand};
use crate::config::paths::get_project_dir;
use crate::context::compaction::{compact_context, CompactionConfig};
use crate::file_history::restore::{list_backups, restore_backup};
use crate::history::reader::{load_history, LoadHistoryOptions};
use crate::plans::storage::{list_plans, load_plan};
use crate::session::fork::{fork_session, ForkOptions};
use crate::session::jsonl::append_context_marker;
use crate::session::list::list_sessions;
use crate::tasks::storage::{format_tasks_summary, load_tasks, save_tasks};
use crate::usage::reader::aggregate_usage;

fn cmd<F>(name: &str, description: &str, exec: F) -> SlashCommand
where
    F: for<'a> Fn(
            &'a str,
            &'a mut CommandContext<'_>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>,
        > + Send
        + Sync
        + 'static,
{
    SlashCommand {
        name: name.to_string(),
        description: description.to_string(),
        execute: Arc::new(exec),
    }
}

pub fn create_builtin_commands() -> Vec<SlashCommand> {
    let mut out = Vec::new();

    out.push(cmd("help", "List available commands", |_args, _ctx| {
        Box::pin(async move {
            println!("  /help — see registered commands via /tools or /agents");
            None
        })
    }));

    out.push(cmd("clear", "Clear conversation context", |_args, ctx| {
        Box::pin(async move {
            ctx.messages.truncate(1);
            println!("Context cleared.");
            None
        })
    }));

    out.push(cmd("exit", "Exit Heddle", |_args, _ctx| {
        Box::pin(async move {
            println!("Goodbye!");
            std::process::exit(0);
        })
    }));
    out.push(cmd("quit", "Exit Heddle", |_args, _ctx| {
        Box::pin(async move {
            println!("Goodbye!");
            std::process::exit(0);
        })
    }));

    out.push(cmd("cost", "Show token usage and cost", |_args, ctx| {
        Box::pin(async move {
            let tracker = ctx.cost_tracker.lock();
            println!("  Input tokens:  {}", tracker.total_input_tokens());
            println!("  Output tokens: {}", tracker.total_output_tokens());
            let cost_str = tracker
                .total_cost()
                .map(|c| format!("${c:.4}"))
                .unwrap_or_else(|| "N/A".to_string());
            println!("  Total cost:    {cost_str}");
            None
        })
    }));

    out.push(cmd("status", "Show session status", |_args, ctx| {
        Box::pin(async move {
            println!("  Model:         {}", ctx.config.model);
            println!("  Session:       {}", ctx.session_file.display());
            println!("  Messages:      {}", ctx.messages.len());
            println!(
                "  Approval mode: {}",
                ctx.config
                    .approval_mode
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            None
        })
    }));

    out.push(cmd(
        "context",
        "Show context size estimate",
        |_args, ctx| {
            Box::pin(async move {
                let total: usize = ctx
                    .messages
                    .iter()
                    .map(|m| m.content_str().map(|s| s.len()).unwrap_or(0))
                    .sum();
                let est = total.div_ceil(4);
                println!("  Messages:         {}", ctx.messages.len());
                println!("  Estimated tokens: ~{est}");
                None
            })
        },
    ));

    out.push(cmd(
        "model",
        "Switch model (e.g., /model openrouter/free)",
        |args, ctx| {
            Box::pin(async move {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    println!("  Current model: {}", ctx.config.model);
                    return None;
                }
                println!("  Model switched to: {trimmed}");
                Some(trimmed.to_string())
            })
        },
    ));

    out.push(cmd("tools", "List available tools", |_args, ctx| {
        Box::pin(async move {
            for tool in ctx.registry.all() {
                println!("  {} — {}", tool.name(), tool.description());
            }
            None
        })
    }));

    out.push(cmd(
        "history",
        "Show recent message history",
        |args, _ctx| {
            Box::pin(async move {
                let parts: Vec<&str> = args.split_whitespace().collect();
                let mut limit: usize = 20;
                let mut search: Option<String> = None;
                let mut i = 0;
                while i < parts.len() {
                    if parts[i] == "--limit" && i + 1 < parts.len() {
                        limit = parts[i + 1].parse().unwrap_or(20);
                        i += 2;
                    } else if parts[i] == "--search" && i + 1 < parts.len() {
                        search = Some(parts[i + 1..].join(" "));
                        break;
                    } else {
                        search = Some(parts[i..].join(" "));
                        break;
                    }
                }
                let entries = load_history(&LoadHistoryOptions {
                    limit: Some(limit),
                    search,
                });
                if entries.is_empty() {
                    println!("  No history entries found.");
                    return None;
                }
                for e in entries {
                    let time = e.timestamp.replace('T', " ");
                    println!("  [{time}] {:?} {}", e.content_type, e.message_preview);
                }
                None
            })
        },
    ));

    out.push(cmd(
        "restore",
        "Restore a file from backup (usage: /restore <file> [version])",
        |args, _ctx| {
            Box::pin(async move {
                let parts: Vec<&str> = args.split_whitespace().collect();
                let file_path = match parts.first() {
                    Some(p) => *p,
                    None => {
                        println!("  Usage: /restore <file-path> [version]");
                        return None;
                    }
                };
                if let Some(v) = parts.get(1) {
                    let version: u32 = v.parse().unwrap_or(0);
                    println!(
                        "  {}",
                        restore_backup(std::path::Path::new(file_path), version, None)
                    );
                } else {
                    let backups = list_backups(std::path::Path::new(file_path), None);
                    if backups.is_empty() {
                        println!("  No backups found for {file_path}");
                        return None;
                    }
                    println!("  Backups for {file_path}:");
                    for b in backups.iter().take(10) {
                        println!("    v{} — {} bytes", b.version, b.size);
                    }
                    println!("  Use /restore <file> <version> to restore");
                }
                None
            })
        },
    ));

    out.push(cmd(
        "compact",
        "Compact conversation context",
        |_args, ctx| {
            Box::pin(async move {
                let weak = match &ctx.weak_provider {
                    Some(w) => w.clone(),
                    None => {
                        println!("  No weak model configured — cannot compact.");
                        return None;
                    }
                };
                let model_limit = ctx.config.max_tokens.unwrap_or(128_000);
                match compact_context(
                    ctx.messages,
                    weak.as_ref(),
                    model_limit,
                    CompactionConfig::default(),
                )
                .await
                {
                    Ok(stats) => {
                        println!("  Compacted: removed {} messages", stats.messages_removed);
                        println!("  Tokens: {} → {}", stats.tokens_before, stats.tokens_after);
                    }
                    Err(e) => println!("  Error: {e}"),
                }
                None
            })
        },
    ));

    out.push(cmd("sessions", "List recent sessions", |_args, _ctx| {
        Box::pin(async move {
            let sessions = list_sessions(None);
            if sessions.is_empty() {
                println!("  No sessions found.");
                return None;
            }
            for s in sessions.iter().take(20) {
                let name = s
                    .name
                    .as_deref()
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default();
                let preview = s
                    .first_user_message
                    .as_deref()
                    .map(|m| format!(" — {m}"))
                    .unwrap_or_default();
                println!(
                    "  {}{name} | {} | {} msgs{preview}",
                    &s.id[..8.min(s.id.len())],
                    s.created,
                    s.message_count
                );
            }
            None
        })
    }));

    out.push(cmd("name", "Name the current session", |args, ctx| {
        Box::pin(async move {
            let name = args.trim();
            if name.is_empty() {
                println!("  Usage: /name <session-name>");
                return None;
            }
            let _ = append_context_marker(
                &ctx.session_file,
                &json!({
                    "type": "session_name",
                    "name": name,
                    "timestamp": Utc::now().to_rfc3339(),
                }),
            );
            println!("  Session named: {name}");
            None
        })
    }));

    out.push(cmd("fork", "Fork the current session", |_args, ctx| {
        Box::pin(async move {
            match fork_session(&ctx.session_file, ForkOptions::default()) {
                Ok(result) => {
                    println!("  Forked to: {}", result.session_file.display());
                    println!("  New session ID: {}", result.session_id);
                }
                Err(e) => println!("  Error: {e}"),
            }
            None
        })
    }));

    out.push(cmd(
        "tasks",
        "List or clear tracked tasks (usage: /tasks [clear])",
        |args, ctx| {
            Box::pin(async move {
                if args.trim() == "clear" {
                    let tasks = load_tasks(None);
                    let total = tasks.len();
                    let remaining: Vec<_> = tasks
                        .into_iter()
                        .filter(|t| t.status != crate::tasks::types::TaskStatus::Done)
                        .collect();
                    let cleared = total - remaining.len();
                    let _ = save_tasks(&remaining, None);
                    println!("  Cleared {cleared} completed tasks.");
                    return None;
                }
                let tasks = load_tasks(None);
                if tasks.is_empty() {
                    println!("  No tasks tracked.");
                    return None;
                }
                println!("{}", format_tasks_summary(&tasks, &ctx.session_id));
                None
            })
        },
    ));

    out.push(cmd(
        "agents",
        "List available agent definitions",
        |_args, ctx| {
            Box::pin(async move {
                if ctx.agent_definitions.is_empty() {
                    println!("  No agent definitions found.");
                    return None;
                }
                for (name, def) in ctx.agent_definitions {
                    let model = def
                        .model
                        .as_deref()
                        .map(|m| format!(" ({m})"))
                        .unwrap_or_default();
                    println!("  {name}{model} — {}", def.description);
                }
                None
            })
        },
    ));

    out.push(cmd(
        "plan",
        "Load or list saved plans (usage: /plan list | /plan load <name>)",
        |args, _ctx| {
            Box::pin(async move {
                let parts: Vec<&str> = args.split_whitespace().collect();
                let sub = parts.first().copied().unwrap_or("");
                if sub == "list" || sub.is_empty() {
                    let plans = list_plans(None);
                    if plans.is_empty() {
                        println!("  No saved plans.");
                        return None;
                    }
                    for p in plans {
                        let date = if p.created.is_empty() {
                            "unknown".to_string()
                        } else {
                            p.created.replace('T', " ").chars().take(19).collect()
                        };
                        println!("  {} ({date}) — {}", p.name, p.preview);
                    }
                    return None;
                }
                if sub == "load" {
                    let name = parts[1..].join(" ");
                    if name.is_empty() {
                        println!("  Usage: /plan load <name>");
                        return None;
                    }
                    match load_plan(&name, None) {
                        Some(p) => {
                            println!("  Plan: {}", p.name);
                            if let Some(c) = p.meta.get("created") {
                                println!("  Created: {c}");
                            }
                            if let Some(m) = p.meta.get("model") {
                                println!("  Model: {m}");
                            }
                            println!("\n{}", p.content);
                        }
                        None => println!("  Plan not found: {name:?}"),
                    }
                    return None;
                }
                println!("  Usage: /plan list | /plan load <name>");
                None
            })
        },
    ));

    out.push(cmd(
        "stats",
        "Show usage stats (usage: /stats [project])",
        |args, ctx| {
            Box::pin(async move {
                if args.trim() == "project" {
                    let stats = aggregate_usage(&get_project_dir(None));
                    println!("  Sessions:      {}", stats.total_sessions);
                    println!("  Input tokens:  {}", stats.total_input_tokens);
                    println!("  Output tokens: {}", stats.total_output_tokens);
                    println!("  Total cost:    ${:.4}", stats.total_cost);
                    if !stats.tool_calls.is_empty() {
                        println!("  Tool calls:");
                        let mut entries: Vec<_> = stats.tool_calls.iter().collect();
                        entries.sort_by(|a, b| b.1.cmp(a.1));
                        for (tool, count) in entries {
                            println!("    {tool}: {count}");
                        }
                    }
                    return None;
                }
                let tracker = ctx.cost_tracker.lock();
                println!("  Input tokens:  {}", tracker.total_input_tokens());
                println!("  Output tokens: {}", tracker.total_output_tokens());
                let cost_str = tracker
                    .total_cost()
                    .map(|c| format!("${c:.4}"))
                    .unwrap_or_else(|| "N/A".to_string());
                println!("  Total cost:    {cost_str}");
                None
            })
        },
    ));

    out.push(cmd(
        "paste",
        "Manage paste cache (usage: /paste [list|clear])",
        |args, ctx| {
            Box::pin(async move {
                let cache = match &ctx.paste_cache {
                    Some(c) => c.clone(),
                    None => {
                        println!("  Paste cache is disabled.");
                        return None;
                    }
                };
                let sub = args.trim();
                if sub == "clear" {
                    cache.lock().clear();
                    println!("  Paste cache cleared.");
                    return None;
                }
                let entries = cache.lock().list();
                if entries.is_empty() {
                    println!("  Paste cache is empty.");
                    return None;
                }
                for e in entries {
                    let id = e
                        .paste_id
                        .as_deref()
                        .map(|i| format!(" [paste:{i}]"))
                        .unwrap_or_default();
                    let size = e.content.len();
                    println!(
                        "  {} ({} lines, {size} bytes){id}",
                        e.path.display(),
                        e.lines
                    );
                }
                None
            })
        },
    ));

    out.push(cmd(
        "agent",
        "Show agent definition details (usage: /agent <name>)",
        |args, ctx| {
            Box::pin(async move {
                let name = args.trim();
                if name.is_empty() {
                    println!("  Usage: /agent <name>");
                    return None;
                }
                match ctx.agent_definitions.get(name) {
                    Some(def) => {
                        println!("  Name:        {}", def.name);
                        println!("  Description: {}", def.description);
                        if let Some(m) = &def.model {
                            println!("  Model:       {m}");
                        }
                        if let Some(t) = &def.tools {
                            println!("  Tools:       {}", t.join(", "));
                        }
                        println!("  Source:      {}", def.source.display());
                        if !def.system_prompt.is_empty() {
                            let preview: String = def.system_prompt.chars().take(200).collect();
                            let suffix = if def.system_prompt.len() > 200 {
                                "..."
                            } else {
                                ""
                            };
                            println!("  Prompt:      {preview}{suffix}");
                        }
                    }
                    None => {
                        println!("  Agent not found: {name:?}");
                    }
                }
                None
            })
        },
    ));

    out
}
