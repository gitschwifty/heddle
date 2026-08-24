# Workspace Bash Sandbox

Heddle confines the agent's `bash` tool separately from the Heddle process.
On macOS it uses Apple Seatbelt through `sandbox-exec`; the session and eval
runner both use this same backend. File tools enforce the same workspace
boundary in Rust before accessing their targets.

## Boundary

- The canonical workspace root and explicitly approved additional roots are
  readable and writable.
- The shell receives a minimal, constructed environment: system commands plus
  the conventional Rustup/Cargo runtime when it exists. It does not inherit the
  parent shell's full `PATH`, `HOME`, tokens, or proxy configuration.
- System runtime files and the configured developer toolchain are readable and
  executable only as required to run common build and test commands.
- Network access and files outside the workspace/toolchain runtime are denied.
- On platforms without an equivalent backend, workspace `bash` fails closed;
  it does not fall back to unrestricted shell execution.

The policy must allow macOS's dynamic-loader and runtime operations in addition
to the workspace itself. Merely allowing `/bin/bash` to be read is insufficient:
the OS separately controls executable-library mapping and standard runtime
metadata access.

## Runtime contract

The macOS backend must support this baseline inside a fresh Rust workspace:

```sh
echo sandbox-echo
printf ' sandbox-printf'
cargo metadata --no-deps --format-version 1 >/dev/null
cargo generate-lockfile
```

That contract proves basic shell execution, builtin output, Rust-toolchain
discovery, dynamic loading, and `Cargo.lock` generation. It is deliberately a
small starting point; compilation and additional runtimes are added as named
contracts when Heddle supports them. It is covered by the macOS test suite:

```sh
cargo test --test workspace_bash_runtime_live -- --nocapture
```

The test creates its fixture under this checkout so it can also run when the
test process already has a workspace-scoped sandbox. If the host explicitly
rejects applying a nested Seatbelt profile, it reports that condition and skips
the contract rather than mistaking it for a Heddle policy denial.

## Design notes and references

Codex is the closest design reference: it constructs a sandbox command with an
explicit environment and separates the macOS base/runtime policy from the
workspace policy, rather than treating command parsing as the security
boundary. Claude Code's sandboxed Bash preserves a usable subprocess
environment while separately scrubbing credentials and redirecting its temp
directory. Hermes-style agents choose an explicit execution backend (local,
container, remote, or cloud sandbox), which owns its runtime environment.

Heddle deliberately takes the stricter of those approaches for local sessions:
it constructs a small environment rather than inheriting the terminal's entire
`PATH`. `HOME` and `TMPDIR` point into the workspace. Cargo is made available
via `~/.cargo/bin`, with read-only access to its registry cache and Rustup
toolchains; build artifacts and temporary files remain in the workspace. This
keeps a host's unrelated PATH entries and
credential-bearing home directories out of the bash tool.

As more runtimes are added, model them as runtime descriptors: a constructed
environment plus the executable, library, read-only state, and workspace-local
write paths it needs. Do not turn approvals for individual commands into
permanent filesystem allowances, and do not inherit the entire host `PATH`.

- [Codex macOS base policy](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt_base_policy.sbpl)
- [Codex platform runtime policy](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/restricted_read_only_platform_defaults.sbpl)
- [Claude Code sandboxing](https://code.claude.com/docs/en/sandboxing)
- [Hermes terminal backends](https://github.com/hermes-agent-org/hermes/blob/main/website/docs/user-guide/features/tools.md)

When a command needs additional runtime access, add the smallest allowance
that makes a named contract test pass, and retain a negative escape test. Do
not add a permissive fallback for normal sessions or evals.

For comparative Seatbelt/runtime research and the next policy design, see
[Sandboxed Developer Runtime Research](sandbox-runtime-research.md).
