//! Session setup: load config, build providers, register tools, prepare context.
//!
//! This is the central wiring point — it ties config, providers, tools, hooks,
//! permissions, and metrics together. The TS module is 283 LOC; this Rust port
//! keeps the same flow but uses owned types and `Arc<Mutex<…>>` to share state
//! across the agent loop and the CLI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::Utc;
use parking_lot::Mutex;
use uuid::Uuid;

use crate::agents::loader::load_agent_definitions;
use crate::agents::types::AgentDefinition;
use crate::config::agents_md::load_agents_context;
use crate::config::discovery::{resolve_discovery, DiscoveryResult};
use crate::config::features::{get_features, FeatureFlags, Mode};
use crate::config::loader::{load_config, HeddleConfig, PermissionsLayer};
use crate::config::paths::{ensure_heddle_dirs, get_project_memory_dir, get_project_sessions_dir};
use crate::context::paste_cache::PasteCache;
use crate::cost::pricing::ModelPricing;
use crate::cost::tracker::CostTracker;
use crate::file_history::cleanup::{run_file_history_cleanup, CleanupConfig};
use crate::hooks::runner::HooksRunner;
use crate::hooks::types::HookMode;
use crate::memory::loader::load_memory_context;
use crate::permissions::checker::PermissionChecker;
use crate::provider::factory::create_providers;
use crate::provider::types::Provider;
use crate::session::fork::{fork_session, ForkOptions};
use crate::session::jsonl::{append_message, load_session, write_session_meta, SessionMeta};
use crate::session::list::find_session;
use crate::tasks::storage::{format_tasks_summary, load_tasks};
use crate::tools::registry::ToolRegistry;
use crate::tools::types::HeddleTool;
use crate::tools::{
    create_bash_tool, create_edit_tool, create_glob_tool, create_grep_tool, create_read_tool,
    create_save_memory_tool, create_save_plan_tool, create_subagent_tool,
    create_web_fetch_tool_with_options, create_write_tool, SubagentOptions, WebFetchOptions,
};
use crate::tools::{create_create_task_tool, create_list_tasks_tool, create_update_task_tool};
use crate::types::{Message, SystemMessage};
use crate::usage::collector::MetricsCollector;

const DEFAULT_PROMPT: &str = r#"You are an interactive software engineering assistant operating in a real file system. The user is collaborating with you on the codebase rooted at the current working directory.

## How to work

- Prefer reading actual file contents over guessing. When asked about code, locate the file with `glob` or `grep` and read it.
- Use tools to take action, not to narrate. If the task is to edit a file, call `edit_file` rather than describing the change.
- Make small, targeted edits. Match existing conventions in nearby code.
- When a request is ambiguous and the wrong interpretation would be hard to undo, ask one short clarifying question before acting. Otherwise, prefer doing.
- Don't add error handling, validation, or abstractions that the task doesn't require.

## Tools

You have file system tools (`read_file`, `write_file`, `edit_file`, `glob`, `grep`), a shell tool (`bash`), and a web fetch tool. Tool calls execute on the user's machine; results come back as tool messages.

## Output style

- When you've completed the task, give a one- to two-sentence summary of what changed.
- Don't repeat the request back. Don't enumerate steps you already took. The user can see the diff.
- If you couldn't complete the task, say so plainly with the reason."#;

fn runtime_context(cwd: &std::path::Path) -> String {
    format!(
        "## Runtime Context\n\nCurrent working directory: {}\n\nWhen the user asks about files in \"this repo\", \"the README\", or similar relative paths, resolve them from the current working directory. Do not invent absolute paths.",
        cwd.display()
    )
}

pub struct SessionContext {
    pub config: HeddleConfig,
    pub provider: Arc<dyn Provider>,
    pub weak_provider: Option<Arc<dyn Provider>>,
    pub editor_provider: Option<Arc<dyn Provider>>,
    pub registry: ToolRegistry,
    pub messages: Vec<Message>,
    pub session_file: PathBuf,
    pub session_id: String,
    pub cost_tracker: Arc<Mutex<CostTracker>>,
    pub model_pricing: ModelPricing,
    pub permission_checker: Option<Arc<Mutex<PermissionChecker>>>,
    pub hooks_runner: Option<Arc<HooksRunner>>,
    pub features: FeatureFlags,
    pub discovery: Option<DiscoveryResult>,
    pub agent_definitions: HashMap<String, AgentDefinition>,
    pub metrics_collector: Option<Arc<Mutex<MetricsCollector>>>,
    pub paste_cache: Option<Arc<Mutex<PasteCache>>>,
    pub session_start_time: chrono::DateTime<Utc>,
    pub base_system_prompt: String,
}

#[derive(Debug, Default, Clone)]
pub struct PermissionOverrides {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
    pub ask: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone)]
pub struct SessionOptions {
    pub mode: Option<Mode>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<String>>,
    pub cwd: Option<PathBuf>,
    pub resume: Option<String>,
    pub fork: Option<String>,
    pub session_name: Option<String>,
    pub agent: Option<String>,
    pub permission_overrides: Option<PermissionOverrides>,
}

fn default_tools(config: &HeddleConfig) -> Vec<Arc<dyn HeddleTool>> {
    vec![
        create_read_tool(),
        create_write_tool(),
        create_edit_tool(),
        create_glob_tool(),
        create_grep_tool(),
        create_bash_tool(),
        create_web_fetch_tool_with_options(WebFetchOptions {
            allow_private_addresses: config.web_fetch_allow_private_addresses,
        }),
    ]
}

fn build_system_message(
    features: &FeatureFlags,
    session_id: &str,
    base_prompt: &str,
) -> Result<Message> {
    let agents_ctx = load_agents_context(None);
    let memory_ctx = load_memory_context(None);

    let mut tasks_ctx = String::new();
    if features.tasks {
        let tasks = load_tasks(None);
        if !tasks.is_empty() {
            tasks_ctx = format!(
                "## Current Tasks\n\n{}",
                format_tasks_summary(&tasks, session_id)
            );
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(s) = agents_ctx {
        parts.push(s);
    }
    if let Some(s) = memory_ctx {
        parts.push(s);
    }
    if !tasks_ctx.is_empty() {
        parts.push(tasks_ctx);
    }
    parts.push(runtime_context(&std::env::current_dir()?));
    parts.push(base_prompt.to_string());

    Ok(Message::System(SystemMessage {
        content: parts.join("\n\n"),
    }))
}

pub fn fresh_system_message(session: &SessionContext) -> Result<Message> {
    build_system_message(
        &session.features,
        &session.session_id,
        &session.base_system_prompt,
    )
}

pub async fn create_session(options: SessionOptions) -> Result<SessionContext> {
    ensure_heddle_dirs();
    // Fire-and-forget file-history cleanup.
    tokio::task::spawn_blocking(|| {
        run_file_history_cleanup(CleanupConfig::default());
    });

    if let Some(cwd) = &options.cwd {
        if !cwd.exists() {
            return Err(anyhow!("Directory does not exist: {}", cwd.display()));
        }
        std::env::set_current_dir(cwd)?;
    }

    let mut config = load_config(None);
    let mode = options.mode.unwrap_or(Mode::Interactive);
    let features = get_features(mode, config.features.as_ref());
    let discovery = resolve_discovery(None, None);

    if config.api_key.is_none() {
        return Err(anyhow!(
            "OPENROUTER_API_KEY environment variable or api_key in config.toml is required"
        ));
    }
    if let Some(model) = &options.model {
        config.model = model.clone();
    }

    let agent_definitions = load_agent_definitions(&discovery);
    let mut agent_def: Option<AgentDefinition> = None;
    if let Some(name) = &options.agent {
        agent_def = Some(agent_definitions.get(name).cloned().ok_or_else(|| {
            let avail = agent_definitions
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if avail.is_empty() {
                anyhow!("Agent not found: {name:?}. No agent definitions found.")
            } else {
                anyhow!("Agent not found: {name:?}. Available: {avail}")
            }
        })?);
        if let Some(def) = &agent_def {
            if let Some(m) = &def.model {
                config.model = m.clone();
            }
        }
    }

    let base_system_prompt: String = agent_def
        .as_ref()
        .map(|d| d.system_prompt.clone())
        .or_else(|| options.system_prompt.clone())
        .or_else(|| config.system_prompt.clone())
        .unwrap_or_else(|| DEFAULT_PROMPT.to_string());

    let providers = create_providers(&config)?;
    let provider = providers.main.clone();

    // Tool registry
    let mut registry = ToolRegistry::new();
    let all_tools = default_tools(&config);
    let filter: Option<Vec<String>> = options.tools.clone().or_else(|| config.tools.clone());
    let to_register: Vec<Arc<dyn HeddleTool>> = match &filter {
        Some(names) => all_tools
            .into_iter()
            .filter(|t| names.iter().any(|n| n == t.name()))
            .collect(),
        None => all_tools,
    };
    for tool in to_register {
        registry.register(tool)?;
    }
    registry.register(create_save_memory_tool(get_project_memory_dir(None)))?;

    let (session_id, session_file, messages) = if let Some(target) = &options.resume {
        let found = find_session(Some(target), None)
            .ok_or_else(|| anyhow!("Session not found: {target}"))?;
        let loaded = load_session(&found);
        let raw = std::fs::read_to_string(&found)?;
        let first_line = raw.lines().next().unwrap_or("");
        let meta: SessionMeta = serde_json::from_str(first_line)?;
        (meta.id, found, loaded)
    } else if let Some(target) = &options.fork {
        let source = find_session(Some(target), None)
            .ok_or_else(|| anyhow!("Session not found: {target}"))?;
        let result = fork_session(&source, ForkOptions::default())?;
        let loaded = load_session(&result.session_file);
        (result.session_id, result.session_file, loaded)
    } else {
        let sid = Uuid::new_v4().to_string();
        let session_dir = get_project_sessions_dir(None);
        let session_file = session_dir.join(format!("{sid}.jsonl"));
        let meta = SessionMeta {
            kind: "session_meta".into(),
            id: sid.clone(),
            cwd: std::env::current_dir()?.to_string_lossy().into_owned(),
            model: config.model.clone(),
            created: Utc::now().to_rfc3339(),
            heddle_version: "0.1.0".into(),
            name: options.session_name.clone(),
            forked_from: None,
            extra: Default::default(),
        };
        write_session_meta(&session_file, &meta)?;

        let system_msg = build_system_message(&features, &sid, &base_system_prompt)?;
        append_message(&session_file, &system_msg)?;
        (sid, session_file, vec![system_msg])
    };

    let cost_tracker = Arc::new(Mutex::new(CostTracker::new()));
    let model_pricing = ModelPricing::new(
        config.api_key.clone().unwrap_or_default(),
        config.base_url.as_deref(),
    );

    let permission_checker = if let Some(mode) = config.approval_mode {
        let mut layers: Vec<PermissionsLayer> = Vec::new();
        if let Some(cfg_layers) = &config.permissions_layers {
            layers.extend(cfg_layers.clone());
        }
        if let Some(overrides) = &options.permission_overrides {
            layers.push(PermissionsLayer {
                allow: overrides.allow.clone().unwrap_or_default(),
                deny: overrides.deny.clone().unwrap_or_default(),
                ask: overrides.ask.clone().unwrap_or_default(),
            });
        }
        let cwd = std::env::current_dir().ok();
        let checker = PermissionChecker::new(
            mode,
            if layers.is_empty() {
                None
            } else {
                Some(&layers)
            },
            cwd,
        );
        Some(Arc::new(Mutex::new(checker)))
    } else {
        None
    };

    let hooks_runner = if features.hooks
        && config
            .hooks
            .as_ref()
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    {
        Some(Arc::new(HooksRunner::new(
            config.hooks.clone().unwrap_or_default(),
            hook_mode_for(mode),
            session_id.clone(),
            std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            config.model.clone(),
        )))
    } else {
        None
    };

    registry.register(create_subagent_tool(
        provider.clone(),
        registry.clone(),
        SubagentOptions {
            permission_checker: permission_checker.clone(),
            cost_tracker: Some(cost_tracker.clone()),
            hooks_runner: hooks_runner.clone(),
            max_iterations: None,
        },
    ))?;

    if features.tasks {
        registry.register(create_create_task_tool(session_id.clone(), None))?;
        registry.register(create_update_task_tool(None))?;
        registry.register(create_list_tasks_tool(session_id.clone(), None))?;
    }
    registry.register(create_save_plan_tool(
        session_id.clone(),
        Some(config.model.clone()),
    ))?;

    let metrics_collector = if features.usage_data {
        Some(Arc::new(Mutex::new(MetricsCollector::new())))
    } else {
        None
    };
    let paste_cache = if features.paste_cache {
        Some(Arc::new(Mutex::new(PasteCache::default())))
    } else {
        None
    };

    Ok(SessionContext {
        config,
        provider,
        weak_provider: providers.weak,
        editor_provider: providers.editor,
        registry,
        messages,
        session_file,
        session_id,
        cost_tracker,
        model_pricing,
        permission_checker,
        hooks_runner,
        features,
        discovery: Some(discovery),
        agent_definitions,
        metrics_collector,
        paste_cache,
        session_start_time: Utc::now(),
        base_system_prompt,
    })
}

fn hook_mode_for(mode: Mode) -> HookMode {
    match mode {
        Mode::Interactive | Mode::NonInteractive => HookMode::Interactive,
        Mode::Headless => HookMode::Headless,
    }
}
