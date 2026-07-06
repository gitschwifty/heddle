use heddle::cli::shell::{format_shell_for_context, run_shell, ShellResult};
use heddle::types::Message;

#[tokio::test]
async fn captures_stdout_and_returns_exit_code_zero() {
    let r = run_shell("echo hello").await;
    assert_eq!(r.stdout.trim(), "hello");
    assert_eq!(r.stderr, "");
    assert_eq!(r.exit_code, 0);
}

#[tokio::test]
async fn returns_nonzero_exit_code_on_failure() {
    let r = run_shell("exit 1").await;
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn captures_stderr() {
    let r = run_shell("echo err >&2").await;
    assert_eq!(r.stderr.trim(), "err");
    assert_eq!(r.stdout, "");
    assert_eq!(r.exit_code, 0);
}

#[tokio::test]
async fn captures_both_stdout_and_stderr() {
    let r = run_shell("echo out && echo err >&2").await;
    assert_eq!(r.stdout.trim(), "out");
    assert_eq!(r.stderr.trim(), "err");
    assert_eq!(r.exit_code, 0);
}

#[test]
fn format_returns_user_message_with_command_and_stdout() {
    let result = ShellResult {
        stdout: "hello world\n".into(),
        stderr: "".into(),
        exit_code: 0,
    };
    let msg = format_shell_for_context("echo hello world", &result);
    match msg {
        Message::User(u) => {
            assert!(u.content.contains("echo hello world"));
            assert!(u.content.contains("hello world"));
            assert!(u.content.contains("```"));
        }
        _ => panic!("expected user message"),
    }
}

#[test]
fn format_includes_stderr_section_when_stderr_non_empty() {
    let result = ShellResult {
        stdout: "out\n".into(),
        stderr: "warning\n".into(),
        exit_code: 0,
    };
    let msg = format_shell_for_context("cmd", &result);
    match msg {
        Message::User(u) => {
            assert!(u.content.contains("stderr"));
            assert!(u.content.contains("warning"));
        }
        _ => panic!("expected user message"),
    }
}

#[test]
fn format_does_not_include_stderr_section_when_empty() {
    let result = ShellResult {
        stdout: "out\n".into(),
        stderr: "".into(),
        exit_code: 0,
    };
    let msg = format_shell_for_context("cmd", &result);
    match msg {
        Message::User(u) => {
            assert!(!u.content.contains("stderr"));
        }
        _ => panic!("expected user message"),
    }
}
