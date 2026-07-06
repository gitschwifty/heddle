//! Interactive REPL CLI.

pub mod completer;
pub mod mentions;
pub mod oneshot;
pub mod repl;
pub mod shell;

pub use mentions::{build_mention_message, resolve_mentions, MentionResult};
pub use oneshot::{format_oneshot_output, run_oneshot, OneshotOptions, OneshotResult};
pub use repl::start_cli;
pub use shell::{format_shell_for_context, print_shell_result, run_shell, ShellResult};
