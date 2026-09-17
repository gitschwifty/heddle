//! macOS Seatbelt command construction for the workspace Bash tool.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use super::{
    curated_runtimes, runtime_path, rust_toolchain_runtime, sandbox_profile, sandbox_string,
    scrub_sensitive_environment, SandboxProfile,
};

pub(super) fn confined_bash_command(
    roots: &[PathBuf],
    runtime_root: &Path,
    isolated_runtime: bool,
    additional_deny_paths: &[PathBuf],
    profile: SandboxProfile,
    command: &str,
) -> Result<Command, String> {
    // Seatbelt checks physical paths. Also deny existing targets of protected
    // symlink names, even when the target itself has an innocuous filename.
    let mut deny_paths = additional_deny_paths.to_vec();
    for root in roots {
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry.map_err(|_| {
                "Error: could not inspect workspace protected paths safely".to_string()
            })?;
            if entry.file_type().is_symlink() && crate::secret_io::is_protected_path(entry.path()) {
                if let Ok(target) = entry.path().canonicalize() {
                    deny_paths.push(target);
                }
            }
        }
    }
    let additional_deny_paths = deny_paths.as_slice();
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
    let toolchain = rust_toolchain_runtime()?;
    let runtimes = curated_runtimes()?;
    let profile_text = sandbox_profile(
        &root,
        &additional,
        &runtime_root,
        &additional_deny_paths,
        profile,
        toolchain.as_ref(),
        &runtimes,
    );
    let mut cmd = Command::new("/usr/bin/sandbox-exec");
    cmd.args(["-p", &profile_text, "/bin/bash", "-c", command])
        .current_dir(&root);
    match profile {
        SandboxProfile::Strict => {
            cmd.env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", &root);
        }
        SandboxProfile::Developer => {
            cmd.env(
                "PATH",
                std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            )
            .env(
                "HOME",
                std::env::var("HOME").unwrap_or_else(|_| root.clone()),
            );
            scrub_sensitive_environment(&mut cmd);
        }
    }
    // Apply runtime overrides after env_clear: strict children need these too.
    cmd.env("TMPDIR", &runtime_tmp);
    if isolated_runtime {
        cmd.env("CARGO_TARGET_DIR", cargo_target_dir);
    } else {
        // Let Cargo use the repository's normal target directory.
        cmd.env_remove("CARGO_TARGET_DIR");
    }
    if let Some(toolchain) = toolchain {
        cmd.env("PATH", runtime_path(&runtimes, Some(&toolchain.cargo_bin)));
        // Cargo's package-cache locks are mutable, but installed registry/git
        // inputs remain read-only. Never point CARGO_HOME at the host directory.
        let cargo_home = Path::new(&runtime_root).join("cargo-home");
        std::fs::create_dir_all(&cargo_home)
            .map_err(|error| format!("Error: could not create Cargo home: {error}"))?;
        for entry in ["registry", "git", "config.toml"] {
            let source = Path::new(&toolchain.cargo_home).join(entry);
            let destination = cargo_home.join(entry);
            if source.exists() && std::fs::symlink_metadata(&destination).is_err() {
                match std::os::unix::fs::symlink(&source, &destination) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(format!("Error: could not link Cargo inputs: {error}"))
                    }
                }
            }
        }
        cmd.env("CARGO_HOME", cargo_home);
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
    .env("GOMODCACHE", Path::new(&runtime_root).join("go-mod-cache"))
    .env("GOTMPDIR", runtime_tmp);
    Ok(cmd)
}
