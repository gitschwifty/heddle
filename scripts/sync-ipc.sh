#!/usr/bin/env bash
set -euo pipefail

HEDDLE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ORBOROS_ROOT="/Users/pjtaggart/repos/orboros/main"

if [[ ! -d "$ORBOROS_ROOT" ]]; then
  echo "Orboros repo not found at $ORBOROS_ROOT" >&2
  exit 1
fi

mkdir -p "$HEDDLE_ROOT/test/ipc/fixtures"

rsync -a --delete "$ORBOROS_ROOT/fixtures/ipc/" "$HEDDLE_ROOT/test/ipc/fixtures/"
rsync -a "$ORBOROS_ROOT/compatibility.md" "$HEDDLE_ROOT/compatibility.md"
rsync -a "$ORBOROS_ROOT/PROTOCOL_VERSION" "$HEDDLE_ROOT/PROTOCOL_VERSION"

echo "Synced IPC fixtures and compatibility policy from Orboros."

# Fail only if there are unstaged changes after sync.
if [[ -n "$(git -C "$HEDDLE_ROOT" diff --name-only)" ]]; then
  echo "IPC sync produced unstaged changes. Run git add and re-commit." >&2
  exit 1
fi
