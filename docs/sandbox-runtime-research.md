# Sandboxed Developer Runtime Research

This note informs the next iteration of Heddle's macOS workspace Bash policy.
The first implementation deliberately proves a minimal Cargo contract. It is
not a general developer-runtime policy: real developer tools commonly live
outside `/usr/bin`, have read-only package/toolchain state, and create cache or
build output in distinct locations.

## Common approaches

### Copilot CLI: discover a runtime for each command

GitHub Copilot CLI resolves its policy before each sandboxed process. It treats
each absolute, existing, canonicalized directory from `PATH` as a candidate
read-only tool directory, and also examines named toolchain variables. It
rejects relative paths, filesystem roots, system-critical paths, and duplicate
resolved paths. It inspects `CARGO_HOME` and `RUSTUP_HOME` alongside Python,
Go, Node, Java, .NET, and other toolchain variables.

It separates immutable developer state from mutable state: toolchains and
registries are read-only, while build caches are read/write. Its policy report
shows the resolved per-command grants. This is the closest published model for
handling Cargo/Rustup and future runtimes without hardcoding every install
location.

Sources:

- [Copilot filesystem-policy construction](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/understanding-local-sandboxing)
- [Copilot tool-directory discovery and variable list](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference)

### Codex: compose workspace policy with macOS runtime defaults

Codex keeps a reusable deny-by-default Seatbelt base policy separate from a
curated macOS platform policy. The latter grants the dynamic loader, framework
and library mapping, selected device files, runtime metadata, and other normal
OS facilities. A workspace-specific policy is then composed with that base.
This separation avoids incorrectly treating system-loader requirements as
project permissions.

Sources:

- [Codex Seatbelt base policy](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt_base_policy.sbpl)
- [Codex macOS restricted platform defaults](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/restricted_read_only_platform_defaults.sbpl)
- [Codex Seatbelt policy assembly](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs)

### Anthropic Sandbox Runtime / Claude Code: configurable roots and violations

Anthropic's open-source runtime generates Seatbelt profiles per invocation,
records attributed violations, and uses explicit writable roots plus network
proxying. Its default read approach is more compatibility-oriented than
Heddle's: users deny sensitive read paths and re-allow project paths, whereas
Heddle intends to keep host reads denied by default. The implementation is
still a valuable reference for ordered Seatbelt deny/allow rules, path glob
handling, move protection, and surfacing the actual denied path to an agent.

Sources:

- [Sandbox Runtime repository and configuration](https://github.com/anthropic-experimental/sandbox-runtime)
- [macOS profile generator](https://github.com/anthropic-experimental/sandbox-runtime/blob/main/src/sandbox/macos-sandbox-utils.ts)
- [Claude Code sandboxing architecture](https://www.anthropic.com/engineering/claude-code-sandboxing)

### Nix-oriented and write-only designs

`sandboxed-agents` avoids most host-toolchain discovery by running tools from
the Nix store, then explicitly grants the project, agent caches, and an
invocation temp directory. Fletch intentionally uses Seatbelt only to restrict
writes outside an isolated clone; it leaves host reads and network available.
Those designs are useful contrast: reproducible runtimes reduce discovery
work, while write-only isolation maximizes compatibility but does not meet
Heddle's default confidential-read boundary.

Sources:

- [sandboxed-agents Darwin backend](https://github.com/eordano/sandboxed-agents)
- [Fletch isolation model](https://github.com/fwdai/fletch)

## Implications for Heddle

Heddle should construct a sanitized child environment and independently derive
the matching filesystem policy. It should not blindly expose the caller's
entire environment, and it should not hardcode a single Cargo installation.

Each discovered runtime needs four explicit categories:

1. executable and dynamic-library roots (read/execute);
2. immutable toolchain and registry state (read-only);
3. mutable build/cache/temp locations (write only where necessary, preferably
   isolated per workspace or eval); and
4. required platform facilities (kept in a reusable macOS base policy, not
   mixed into a tool-specific rule).

Cargo illustrates why this matters. The Rustup shim in `~/.cargo/bin` may
perform host-side update bookkeeping, while the resolved Cargo binary lives in
the selected Rustup toolchain. Cargo also needs a registry/cache and an output
directory; macOS compilation invokes Xcode command-line tools, the selected
SDK, and `xcode_select_link`. A robust policy resolves those paths, then grants
the smallest category-appropriate permission. It must not solve a compiler
failure by opening all of the user's home directory.

The associated implementation plan is [Task 134](../private/planning/task-details/134-runtime-aware-workspace-bash-policy.md).
