//! macOS runtime contract for the confined workspace bash tool.

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use heddle::tools::{create_workspace_bash_tool, ExecOptions, WorkspaceBoundary};
    use serde_json::json;

    fn fixture() -> tempfile::TempDir {
        // Keep the fixture beneath this checkout. This lets the contract run
        // unchanged when the test process itself is already sandboxed.
        let workspace = tempfile::Builder::new()
            .prefix("workspace-bash-runtime-")
            .tempdir_in(std::env::current_dir().unwrap().join("target"))
            .unwrap();
        std::fs::create_dir_all(workspace.path().join("src")).unwrap();
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            "[package]\nname = \"sandbox-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            workspace.path().join("src/lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();

        workspace
    }

    async fn run(workspace: &std::path::Path, command: &str) -> Option<String> {
        let boundary = Arc::new(parking_lot::RwLock::new(
            WorkspaceBoundary::new(workspace).unwrap(),
        ));
        let result = create_workspace_bash_tool(boundary)
            .execute(json!({"command": command}), ExecOptions::default())
            .await;
        if result.contains("sandbox-exec: sandbox_apply: Operation not permitted") {
            eprintln!("skipping: host sandbox does not permit nested Seatbelt");
            None
        } else {
            Some(result)
        }
    }

    #[tokio::test]
    async fn confined_bash_executes_shell_builtins() {
        let workspace = fixture();
        let Some(result) = run(
            workspace.path(),
            "echo sandbox-echo && printf ' sandbox-printf'",
        )
        .await
        else {
            return;
        };

        assert_eq!(result, "sandbox-echo\n sandbox-printf");
    }

    #[tokio::test]
    async fn confined_bash_discovers_cargo_and_generates_a_lockfile() {
        let workspace = fixture();
        let Some(result) = run(
            workspace.path(),
            "command -v cargo && cargo metadata --no-deps --format-version 1 >/dev/null && cargo generate-lockfile",
        )
        .await
        else {
            return;
        };

        assert!(result.contains("cargo"), "{result}");
        assert!(!result.contains("Exit code:"), "{result}");
        assert!(workspace.path().join("Cargo.lock").exists(), "{result}");
    }

    #[tokio::test]
    async fn confined_bash_discovers_curated_developer_runtimes() {
        let workspace = fixture();
        let Some(result) = run(
            workspace.path(),
            "node --version && npx --version && tsc --version && bun --version && go version",
        )
        .await
        else {
            return;
        };

        assert!(!result.contains("Exit code:"), "{result}");
    }

    #[tokio::test]
    async fn confined_bash_resolves_the_macos_sdk_and_compiles_rust() {
        let workspace = fixture();
        let Some(result) = run(
            workspace.path(),
            "xcrun --sdk macosx --show-sdk-path >/dev/null && cargo test --no-run",
        )
        .await
        else {
            return;
        };
        if result.contains("have not agreed to the Xcode license agreements") {
            eprintln!("skipping: host Xcode license has not been accepted");
            return;
        }

        assert!(!result.contains("Exit code:"), "{result}");
        assert!(
            !workspace.path().join("xcrun_db").exists(),
            "xcrun cache escaped runtime output root"
        );
    }
}
