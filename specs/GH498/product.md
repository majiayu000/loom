# GH498 Product Spec: Adapter-Driven Visibility

## Goal

Loom visibility, diagnose, and reconcile planning must work from agent adapter metadata instead of hard-coded Codex assumptions.

## User Outcomes

- `loom skill visibility <skill> --agent codex` preserves existing Codex behavior.
- `loom skill visibility <skill> --agent claude` reports real source, binding, target, projection, adapter visibility, config metadata, and reload status.
- `loom skill diagnose <skill> --agent claude` includes agent visibility evidence instead of rejecting the agent.
- `loom agent reconcile --agent claude --dry-run` produces safe projection repair plans without mutating registry state.
- Agents that cannot be resolved to visibility metadata return structured `visibility_unsupported` evidence rather than an argument error.

## Non-Goals

- This slice does not apply non-Codex reconcile plans.
- This slice does not edit Claude settings. Adapter-defined disable rules are surfaced as metadata only.

## Done When

- Codex visibility and reconcile tests still pass.
- New black-box tests cover Claude visibility, Claude diagnose, Claude reconcile dry-run, and unsupported visibility evidence.
- `cargo test --workspace --all-features` passes.
