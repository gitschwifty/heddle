# Orboros/Heddle IPC Compatibility Policy

## Versioning
- `protocol_version` is required in `InitOk`.
- Orboros may include `protocol_version` in `Init` as an expected version.
- Versions use `MAJOR.MINOR.PATCH`.

## Compatibility Rules
- **MAJOR**: breaking changes only (rename/remove fields, change semantics).
- **MINOR**: additive changes only (new fields, new event types).
- **PATCH**: bug fixes, no schema shape changes.

## Field Naming
- IPC fields are **snake_case**.
- In Rust, use `#[serde(rename_all = "snake_case")]` to keep internal names idiomatic.

## Forward/Backward Handling
- Clients must ignore unknown fields.
- Required fields must not be removed within a major version.
- New event types are allowed in MINOR versions; clients should treat unknown events as `Event::Unknown` and continue.

## Contract Tests
- Golden transcripts are the source of truth for expected behavior.
- Tests should be **strict line-by-line** with an allowlist of non-deterministic fields.
- Any IPC change must update:
  - JSON Schema
  - `protocol_version`
  - Golden transcripts (normal + error + cancel flow)

## Rollout
- Bump version in Heddle first.
- Add parsing + handling in Orboros.
- Update transcripts and re-run contract tests.
