//! macOS Seatbelt command construction for the workspace Bash tool.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use super::{
    curated_runtimes, runtime_path, rust_toolchain_runtime, sandbox_profile, sandbox_string,
    scrub_sensitive_environment,
};

pub(super) fn confined_bash_command(
    roots: &[PathBuf],
    runtime_root: &Path,
    additional_deny_paths: &[PathBuf],
    command: &str,
) -> Result<Command, String> {
    let root = roots
        .first()
        .ok_or_else(|| "Error: workspace boundary denied empty workspace".to_string())?;
    let root = sandbox_string(root)?;
    let additional = roots
        .iter()
        .skip(1)
        .map(|root| sandbox_string(root))
        .collect::<Result<Vec<_>, _>>()?;
    let runtime_root = sandbox_string(runtime_root)?;
    let additional_deny_paths = additional_deny_paths
        .iter()
        .map(|path| sandbox_string(path))
        .collect::<Result<Vec<_>, _>>()?;
    let runtime_tmp = Path::new(&runtime_root).join("tmp");
    let cargo_target_dir = Path::new(&runtime_root).join("cargo-target");
    std::fs::create_dir_all(&runtime_tmp)
        .map_err(|error| format!("Error: could not create runtime temp directory: {error}"))?;
    std::fs::create_dir_all(&cargo_target_dir)
        .map_err(|error| format!("Error: could not create Cargo target directory: {error}"))?;
    let toolchain = rust_toolchain_runtime()?;
    let runtimes = curated_runtimes()?;
    let profile = sandbox_profile(
        &root,
        &additional,
        &runtime_root,
        &additional_deny_paths,
        toolchain.as_ref(),
        &runtimes,
    );
    let mut cmd = Command::new("/usr/bin/sandbox-exec");
    cmd.args(["-p", &profile, "/bin/bash", "-c", command])
        .current_dir(&root)
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env(
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| root.clone()),
        )
        .env("TMPDIR", runtime_tmp)
        .env("CARGO_TARGET_DIR", cargo_target_dir);
    scrub_sensitive_environment(&mut cmd);
    if let Some(toolchain) = toolchain {
        cmd.env("PATH", runtime_path(&runtimes, Some(&toolchain.cargo_bin)));
        cmd.env("CARGO_HOME", Path::new(&runtime_root).join("cargo-home"));
        cmd.env("RUSTUP_HOME", toolchain.rustup_home)
            .env("RUSTUP_TOOLCHAIN", toolchain.name);
    } else {
        cmd.env("PATH", runtime_path(&runtimes, None));
    }
    cmd.env(
        "GOTELEMETRY",
        std::env::var_os("GOTELEMETRY").unwrap_or_else(|| "off".into()),
    )
    .env("GOCACHE", Path::new(&runtime_root).join("go-cache"))
    .env("GOMODCACHE", Path::new(&runtime_root).join("go-mod-cache"));
    Ok(cmd)
}
