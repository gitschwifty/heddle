# Interactive action policy (Task 128, first slice)

REPL and runtime turns with an interactive permission resolver use the shared
agent-loop permission gate before pre-tool hooks and tool execution. With no
configured approval mode, interactive sessions use `auto-edit`. Headless turns
retain the existing fixed permission behavior and never invent approval prompts.

## Decisions and precedence

- Interactive `plan` mode denies writes, execution and unknown tools.
- Explicit `yolo` mode disables the interactive policy floor, as it does existing
  permission rules. It does not disable the workspace sandbox.
- Configured deny rules remain denied; session approvals cannot override them.
- The built-in floor asks for every Bash command, unknown tool, recognized
  credential/config file mutation and `replace_all` edit. Shell classification
  only supplies a descriptive category: compounds, interpreters, wrappers and
  unknown commands all require approval even when not recognized as destructive.
- Existing configured ask rules and the approval-mode matrix apply to other
  actions. A configured allow cannot bypass the built-in floor.
- An explicit session approval satisfies asks only for the same tool and equal
  parsed JSON arguments, within this live checker. It does not survive restart.
  Changed contents, paths, commands or other arguments require a fresh decision.

This slice reuses the existing permission configuration layering: later allow
entries remove identical earlier deny entries; evaluation then uses deny, ask,
allow order. There is no new glob-specificity ordering or category/profile
configuration schema. File rules now recognize `file_path` as well as `path`,
matching the filesystem tools' argument precedence.

## Approval and provenance

REPL/TUI present allow-once, deny, and allow-identical-arguments-for-session.
The historical `Always` response variant now means this narrow session grant.
Shell paths and effects may be unknown; approving the command authorizes its
effects within the workspace boundary. The same arguments can have different
effects after files or environment change. No rollback is promised.

Interactive JSONL sessions receive `interactive_policy_decision` markers with
call ID, category, matching rule reference, policy decision, outcome, scope and
session-grant status. Rule references identify the built-in category, mode, or
merged configured rule list/index. Raw arguments, rule patterns, commands and
file contents are omitted from these markers. Existing conversation/tool
transcripts keep their existing persistence behavior. A marker describes policy
authorization, not execution success; a later hook or workspace check can deny
the action. Failure to append the marker denies execution and revokes the grant.

## Remaining work

This is a conservative interactive first slice, not all Task 128 acceptance
criteria. Category-specific configurable rules/profiles, resolved path scope,
policy snapshot/version provenance, trash/quarantine UX, and comprehensive
tests remain. Unknown tools include delegation: approving a delegation does not
install this interactive gate inside the child; its existing fixed permission
and sandbox configuration still apply. User-entered REPL shell escapes remain
explicit user actions outside the agent tool gate. An exact-argument grant does
not freeze executable or filesystem state and is not an OS security boundary.

Testing, formatting and all verification for this slice are deferred to the
parent agent. In particular, existing permission/TUI tests that assume tool-wide
`Always` grants or the old prompt wording will need adjustment.
