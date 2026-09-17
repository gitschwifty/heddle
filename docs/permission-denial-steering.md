# Permission denial steering

When the TUI requests permission, press Y to allow once, A to always allow,
or N/Escape to deny and continue. Press S or G to enter a correction while
denying the tool. The tool will not run; the model receives the note and
continues the turn.

In the guidance editor, Enter submits. Shift-Enter or backslash followed by
Enter inserts a newline. Arrow keys, Home, End, and Backspace edit the note.
Escape discards the note and returns to the choices. Ctrl-C denies and cancels
the active turn. Blank notes behave like an ordinary denial.

Runtime callers can return `RuntimePermissionResponse::DenyWithGuidance(String)`.
The agent resolver has the matching `PermissionResponse::DenyWithGuidance(String)`.
Existing `Allow`, `Deny`, and `Always` variants retain their behavior; these enums
are now `Clone` rather than `Copy`. Exhaustive matches must handle the new variant.
REPL choices and headless requests without a resolver retain their existing behavior.

The denied tool result contains the original permission reason followed by
`User guidance:` on a separate, blank-line-delimited section. The correction is
part of the conversation's tool result, so the model can use it on its next
iteration. It does not alter permission rules or grant permission to another tool.

Task 57 includes runtime and TUI regression coverage. Tests, formatting, compiler,
and lint checks are deferred to the parent agent for this implementation.
