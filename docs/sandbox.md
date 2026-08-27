# Workspace Bash Sandbox

Heddle confines the agent's `bash` tool separately from the Heddle process.
On macOS it uses Apple Seatbelt through `sandbox-exec`; the session and eval
runner both use this same backend. File tools enforce the same workspace
boundary in Rust before accessing their targets.

## Policy profiles (design)

The current strict policy is the baseline. The planned policy selector makes
the trade-off explicit rather than silently widening that baseline:

| Profile | Intended use | Reads / environment | Writes / network |
|---|---|---|---|
| `strict` | evals, headless work, untrusted code | Workspace, runtime, and curated toolchain inputs; constructed environment | Workspace/runtime only; network closed by default |
| `developer` | normal interactive local development | Broad host reads and normal developer environment, with hard credential-path denies | Workspace/runtime only; named network mode |
| `trusted` | explicitly trusted local automation | Developer compatibility plus the user-approved structured extensions | Workspace/runtime plus declared additional roots; named network mode |

All profiles keep structured file tools scoped to declared workspace roots.
`trusted` is an explicit compatibility posture, not an implicit fallback when a
sandbox backend is unavailable. On unsupported platforms, Bash still fails
closed.

Profile extensions will be expressed as structured configuration (for example,
additional workspace roots, protected project paths, and a named network mode)
and translated by Heddle into Seatbelt parameters. Heddle will not accept raw
user-supplied Seatbelt fragments: they are difficult to validate, make the
effective policy opaque, and can turn configuration into a sandbox escape.

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
cargo fmt --check
xcrun --sdk macosx --show-sdk-path >/dev/null
cargo test --no-run
```

That contract proves basic shell execution, builtin output, Rust-toolchain
discovery, Rustup-proxy resolution, dynamic loading, `Cargo.lock` generation,
and native Rust compilation. `CARGO_HOME`, `RUSTUP_HOME`, and the resolved
`RUSTUP_TOOLCHAIN` are constructed from Heddle's startup environment and
granted read-only; Heddle never runs `rustup default` or downloads a toolchain
from within the sandbox. The macOS policy admits only the Xcode selector link, the standard
`/Applications/Xcode.app` bundle, and Command Line Tools runtime needed by
`xcrun`, `clang`, and the macOS SDK; it does not grant write access to them. It
is deliberately a small starting point; additional runtimes are added as named
contracts when Heddle supports them. It is covered by the macOS test suite:

```sh
cargo test --test workspace_bash_runtime_live -- --nocapture
```

Native compilation still requires a usable host Xcode or Command Line Tools
installation. In particular, Heddle cannot accept an Xcode license on the
user's behalf; the live compilation contract reports that host prerequisite as
a skip rather than a sandbox-policy failure.

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
`PATH`. `HOME` points into the workspace, while `TMPDIR` and Cargo build output
point into a separate Heddle-owned runtime root. That root is writable only to
the confined Bash child, is unavailable to workspace file tools, and is removed
when its session or eval case ends. Cargo is made available via `~/.cargo/bin`,
with read-only access to its registry cache, Rustup settings, and selected
toolchain. `GOTELEMETRY` is the sole general runtime variable forwarded from
the Heddle process; it defaults to `off` to prevent Go telemetry state from
appearing under the workspace, while an explicit startup value wins. This keeps
transient files such as build artifacts and Xcode's `xcrun_db` out of source
diffs, while also keeping a host's unrelated PATH entries and credential-bearing
home directories out of the bash tool.

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
