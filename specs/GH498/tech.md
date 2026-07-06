# GH498 Technical Spec

## Design

- Keep `CodexVisibilityReport` and `CodexReconcilePlan` wire shapes stable, but populate `agent` from the requested adapter.
- Add a generic visibility entrypoint that loads `AgentAdapterRegistry`, reads `AdapterVisibility`, and delegates Codex config parsing only for `codex`.
- Treat projection identity through `identity_by_projection_method`:
  - `canonical-skill-md-path`: symlink must resolve to the canonical source skill directory.
  - `runtime-skill-md-path`: projected directory must contain the adapter entrypoint.
  - Unknown identity: return a structured check and do not silently claim visibility.
- Add `agent reconcile --agent <agent> --dry-run` as the generic plan command.
- Refactor the reconcile planner to accept `agent` in the request and to use Codex config repair only when `agent == codex`.

## Compatibility

- `loom codex reconcile` remains the Codex apply command.
- `loom codex reconcile --apply --fix-config` continues to be the only path that edits Codex config.
- Existing Codex check IDs remain unchanged.

## Risks

- Adapter-defined disable rules are not parsed for Claude settings. Reports must say that explicitly in metadata rather than implying they were repaired.
- Generic apply is intentionally excluded because projection write and config semantics differ by adapter.
