//! Discover custom commands as `.md` files in skills/ and commands/ dirs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;

use super::types::{CommandContext, SlashCommand};
use crate::config::discovery::{DiscoveryResult, DiscoverySource};
use crate::config::paths::{get_heddle_home, get_local_heddle_dir};
use crate::session::jsonl::append_message;
use crate::types::{Message, UserMessage};

fn scan_directory(dir: &Path, base: &Path) -> Vec<SlashCommand> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            out.extend(scan_directory(&path, base));
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(&path);
        let name = rel
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/")
            .replace('/', ":");
        let file_path: PathBuf = path.clone();
        let name_for_msg = name.clone();
        out.push(SlashCommand {
            name,
            description: format!("Custom command: {}", name_for_msg),
            execute: Arc::new(move |args: &str, ctx: &mut CommandContext<'_>| {
                let file_path = file_path.clone();
                let name_for_msg = name_for_msg.clone();
                let args = args.to_string();
                Box::pin(async move {
                    let content = match tokio::fs::read_to_string(&file_path).await {
                        Ok(c) => c,
                        Err(_) => return None,
                    };
                    let user_content = if args.is_empty() {
                        content
                    } else {
                        format!("{content}\n\n{args}")
                    };
                    let msg = Message::User(UserMessage {
                        content: user_content,
                    });
                    ctx.messages.push(msg.clone());
                    let _ = append_message(&ctx.session_file, &msg);
                    println!("  [skill] {name_for_msg} injected");
                    None
                })
            }),
        });
    }
    out
}

pub fn load_custom_commands(discovery: Option<&DiscoveryResult>) -> Vec<SlashCommand> {
    let mut command_map = std::collections::HashMap::new();
    if let Some(disc) = discovery {
        let mut levels = disc.levels.clone();
        levels.reverse();
        for level in &levels {
            let subdirs: Vec<PathBuf> = if matches!(level.source, DiscoverySource::Agents) {
                vec![level.path.clone()]
            } else {
                vec![level.path.join("skills"), level.path.join("commands")]
            };
            for dir in subdirs {
                for cmd in scan_directory(&dir, &dir) {
                    command_map.insert(cmd.name.clone(), cmd);
                }
            }
        }
    } else {
        let heddle_home = get_heddle_home();
        let local_dir = get_local_heddle_dir();
        let dirs = [
            heddle_home.join("skills"),
            heddle_home.join("commands"),
            local_dir.join("skills"),
            local_dir.join("commands"),
        ];
        for dir in dirs {
            for cmd in scan_directory(&dir, &dir) {
                command_map.insert(cmd.name.clone(), cmd);
            }
        }
    }
    command_map.into_values().collect()
}

#[allow(dead_code)]
fn _unused_chrono() -> String {
    Utc::now().to_rfc3339()
}
