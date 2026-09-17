# Network and secret capabilities: first slice (task 130)

Session setup resolves one existing sandbox profile for Bash and web fetching.
Any placement with a `state_root` (the existing isolation predicate) now forces
`strict`, even when it inherits ambient configuration. Strict Bash already uses
the macOS OS sandbox to deny network access and constructs its environment;
unsupported platforms refuse confined Bash execution. Strict `web_fetch` now
returns a capability denial before creating a client or performing DNS. Tool
allow rules, private-address options, and approval bypasses do not override this
boundary. Subagents inherit the configured tool instances.

Developer sessions retain open Bash/web networking. Existing permission rules
can allow, ask about, or deny web calls. This slice does not add domain-scoped
grants or infer network operations from shell text. The legacy standalone Bash
and web constructors remain unconfined/open APIs; use session construction for
the policy boundary.

Web clients no longer inherit proxy configuration or proxy credentials. Client
construction fails closed, preserving disabled redirects. Provider clients are
separate and retain their existing transport configuration and credential
resolution. Headless setup already defaults to environment provider credentials
and requires an explicit Keychain reference to access Keychain.

Isolated sessions already disable hooks. Enabled hooks now receive a cleared
environment with only `PATH=/usr/bin:/bin`, `HOME=<project>`, `LANG=C`, and the
existing `HEDDLE_HOOK_*` metadata. They execute `/bin/sh` in the project directory.
This removes inherited tokens, proxies, agent sockets, and shell startup
variables. Hooks needing other executables must use explicit paths. No secret
grant mechanism is added. Hooks remain trusted commands with host filesystem
and network access; this environment policy does not sandbox them.

The session JSONL gains a `capability_policy_resolved` context marker recording
the profile, agent network decision, separate provider traffic, and hook policy.
It contains no URLs, commands, environment values, or credential references.
Individual web denials use the existing tool-result path, rather than introducing
a new IPC event. They are not permission-denied runtime events.

## Inventory and remaining work

- Provider factory resolves configured credentials into clients. Provider HTTP
  requests and pricing requests remain trusted runtime network surfaces.
- Workspace Bash has strict/developer policies, curated toolchain inputs, host
  credential-path denies, and developer credential-variable scrubbing. Git
  helpers, toolchain config, local credential files, and the legacy raw Bash API
  need further capability work; environment filtering alone is insufficient.
- Web fetching has no credentials in URL userinfo and disables redirects. DNS
  answers are not yet checked/pinned; hostname-based private-network access is
  still a gap in developer mode.
- Hooks receive tool arguments/results on stdin. Environment isolation does not
  redact that input or hook output. Remote/custom tools registered by library
  callers remain the caller's responsibility; there is no general MCP transport
  policy in this slice.
- Agent results enter model history, hooks, runtime events, headless previews,
  session JSONL, and subagent transcripts. Protected paths and final result
  redaction belong to task 151; no second redaction boundary is added here.
- Per-operation provenance, explicit secret grants, scoped network approvals,
  and broader artifact retention are unfinished task 130 work.

No tests or other verification were run for this slice, as requested. Added web
policy tests are unexecuted. Parent validation should cover isolated session
profile selection, inherited config/allow-rule bypass attempts, subagents, hook
environment and executable compatibility, and existing web proxy expectations.
