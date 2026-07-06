# Directory Discovery

Heddle discovers configuration, skills, and agents by walking the filesystem from your project directory up to your home directory. This means nested projects, monorepos, and shared team configs all work without explicit configuration.

## How Discovery Works

Starting from `cwd`, heddle walks up the directory tree checking for `.heddle/` directories at each level, stopping at `$HOME`. It also checks the repo root for `.agents/skills/` and optionally `/etc/heddle/` for system-wide config.

```
~/repos/myorg/myproject/          ← cwd
  .heddle/                        ← project-level (deepest, highest priority for content)
    config.toml
    skills/
    agents/
~/repos/myorg/
  .heddle/                        ← org-level (shared across repos)
    skills/
~/.heddle/                        ← global (user defaults)
    config.toml
    skills/
    agents/
~/repos/myorg/myproject/
  .agents/skills/                 ← repo-root convention (lowest priority for content)
/etc/heddle/                      ← system-level (admin override for config)
```

## Priority Order

**Content** (skills, agents) — deepest wins:
1. Deepest `.heddle/` (closest to cwd)
2. Intermediate `.heddle/` directories
3. `~/.heddle/` (global)
4. `.agents/skills/` at repo root
5. `/etc/heddle/`

When skill names collide, the first (deepest) occurrence wins.

**Config** — loaded separately by the config loader with its own merge rules (see [config.md](config.md)).

## Discovery Sources

| Source | Path pattern | Contains |
|---|---|---|
| `heddle` | `.heddle/` at any ancestor dir | skills, agents, config |
| `agents` | `.agents/skills/` at repo root | skills only |
| `system` | `/etc/heddle/` | skills, agents, config |

## Skills

Skill files are Markdown (`.md`) in `skills/` subdirectories. They're loaded as slash commands accessible via `/skillname`.

### Name Derivation

Names are derived from file paths relative to the `skills/` directory, with `/` replaced by `:`:

```
skills/review.md          → /review
skills/git/commit.md      → /git:commit
skills/testing/e2e/run.md → /testing:e2e:run
```

### Frontmatter

Skills support optional YAML frontmatter:

```markdown
---
description: Run the full test suite with coverage
author: team
---

Run all tests with coverage reporting...
```

The `description` field is used in help text. Other fields are preserved in the skill's metadata.

### Collision Resolution

When multiple discovery levels define a skill with the same name:
- Deeper `.heddle/` beats shallower
- `.heddle/` beats `.agents/`
- `.agents/` beats `/etc/heddle/`

This lets projects override org-level or global skills.

## Repo Root Detection

Heddle finds the repo root by walking up from cwd looking for a `.git` directory or file (the latter supports git worktrees). This is used to locate `.agents/skills/`.

## Backward Compatibility

The `commands/` subdirectory is still scanned alongside `skills/` in all discovery levels. Both produce slash commands with the same name derivation rules.

## Test Isolation

Set `HEDDLE_HOME` to point at an isolated directory for testing:

```bash
HEDDLE_HOME=/tmp/test-heddle cargo test
```

The `Sandbox::new()` helper in `tests/common/sandbox.rs` automates this for unit tests.
