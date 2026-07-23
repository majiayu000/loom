---
name: loom-registry
description: Manage the local Loom Skill registry and CLI safely. Use for registry status, Skill lifecycle, targets, bindings, projections, activation, sync, operation history, rollback, or diagnostics. Do not use for Loom.com video recording, sharing, editing, or transcription.
---

# Loom Registry

Use the `loom` CLI as the control plane for a local, Git-backed Agent Skill registry. Keep the registry root explicit, consume its JSON envelope, and preserve Loom's planning and approval boundaries.

## Route The Request

Use this Skill when the user is working with any of these local Loom concepts:

- a Skill registry or `loom` CLI command;
- Skill sources, lifecycle, lint, eval, release anchors, diff, or rollback;
- agent targets, workspace bindings, projections, activation, or visibility;
- registry sync, operation backlog, history, replay, health, or diagnostics.

Do not use this Skill for Loom.com videos, screen recording, video links, sharing, editing, captions, transcripts, or viewer analytics. If the request says only “Loom” and the context is video, route it to the Loom.com capability instead. Do not create or advertise a `loom` Skill alias.

## Establish The Registry Boundary

1. Run a read-only JSON command and require a valid `cli_contract_version` in the range declared by `loom.skill.toml` (`>=1.9.0,<2.0.0`). If the field is missing, invalid, or outside that range, stop all mutations; use only `loom --version`/`--help` for diagnosis and install the matching Loom release and shipped `loom-registry` Skill together. Check `loom <command> --help` for the command surface needed by the request. Never guess flags.
2. Obtain the intended registry root from the user or existing project context. Use a path such as `$HOME/.loom-registry` only when it is the user's established registry.
3. Never use the Loom source-code checkout as the writable registry root.
4. Run machine-facing commands in this form:

```bash
loom --json --root "$REGISTRY_ROOT" workspace status
```

5. Treat only `ok=true` as success. On failure, branch on `error.code` and retain `error.message` plus `request_id` for diagnosis.
6. Record every `meta.warnings` entry. A successful envelope with warnings is a risky success, not a clean success.
7. Read current state before proposing a mutation. Prefer `workspace status`, `workspace doctor`, `skill inspect`, `skill diagnose`, `skill visibility`, `target list`, `workspace binding list`, `sync status`, and `ops list` as appropriate.

## Plan Before Mutation

For a normal canonical-source Skill edit, use the durable convergence workflow by default:

```bash
PLAN_JSON="$(loom --json --root "$REGISTRY_ROOT" plan converge "$SKILL" --from-source --require-runtime)"
PLAN_ID="$(printf '%s\n' "$PLAN_JSON" | jq -er 'select(.ok == true and .data.execution_enabled == true and .data.safe_to_apply == true and .data.requires_digest_confirmation == true) | .data.plan_id | select(type == "string" and length > 0)')"
PLAN_DIGEST="$(printf '%s\n' "$PLAN_JSON" | jq -er '.data.plan_digest | select(type == "string" and length > 0)')"
loom --json --root "$REGISTRY_ROOT" apply "$PLAN_ID" --plan-digest "$PLAN_DIGEST" --idempotency-key "$IDEMPOTENCY_KEY"
```

The first command is plan-only: it persists the immutable plan and command audit but must not commit source, update projections, change runtime visibility, or push a remote. Require `ok=true`, `data.execution_enabled=true`, `data.safe_to_apply=true`, non-empty `data.plan_id` and `data.plan_digest`, and `data.requires_digest_confirmation=true`. Review the exact selectors, effects, input conflicts, risks, required approvals, required axes, restart policy, and remote policy before extracting `PLAN_ID` and `PLAN_DIGEST` from the JSON. Never parse the example shell assignment as authorization and never invent missing fields.

Apply only the reviewed `plan_id` with its exact `plan_digest` and a caller-held idempotency key. Reuse all three values for a retry; changing the key creates a different authority boundary. Do not expand the agent, workspace, profile, projection instance, runtime requirement, restart acceptance, or remote policy at apply time. If the plan reports conflicts, blocking risks, required approvals, `execution_enabled=false`, or `safe_to_apply=false`, do not apply.

Remote transport is always last. `local_complete_remote_pending` means local source/projections were retained and the same immutable plan/key may retry transport; it is not full completion. `local_complete_restart_required` requires a restart or new session and recheck unless the reviewed plan explicitly accepted restart-required. Acceptance never changes `visibility.state` to `visible`. When both blockers exist, preserve both next actions and do not let remote `SYNCED` stand in for runtime visibility.

Use `--dry-run` first for commands that support it, including projection, activation/deactivation, rollback, trash, orphan cleanup, reconcile, and sync changes. Do not assume every command accepts `--dry-run`; check `loom <command> --help` when uncertain.

For agent-directed changes:

1. Run `agent preflight` for an existing low-risk binding. Require `data.safe_to_run=true`; `ok=true` with `safe_to_run=false` is blocked.
2. Use `plan use` when the change needs new targets/bindings or durable idempotency, but only after the user authorizes persisting that plan. Require `data.safe_to_apply=true`, review every effect/risk, and collect every `data.required_approvals` token before `apply`.
3. Treat dry-run, preflight, and plan fields as command-specific. A dry-run that omits `safe_to_run`, `safe_to_apply`, or approval fields never authorizes the write by itself.
4. Obtain the approvals required by the returned plan and the user's authorization for the exact effects before applying.
5. Execute the same scoped change without expanding targets, agents, workspaces, or Skill names.
6. Do not add `--force` unless the user explicitly authorizes the exact overwrite after reviewing the conflict.
7. Re-read state after execution and report the resulting operation or commit identifier.

Example activation sequence:

```bash
loom --json --root "$REGISTRY_ROOT" skill activate "$SKILL" --agent codex --scope user --dry-run
loom --json --root "$REGISTRY_ROOT" agent preflight --agent codex --workspace "$WORKSPACE" --skill "$SKILL"
```

Stop if preflight is blocked or `safe_to_run` is not true. If no matching binding exists, obtain authorization to persist a durable plan with `plan use`; review `safe_to_apply`, effects, risks, and approvals before showing or running its exact `apply` command. Do not place a real activation immediately after a dry-run.

## Manage Skill History

Use the low-level commands below for explicit diagnosis and recovery after a typed convergence plan is blocked or returns a partial outcome. They do not replace the default `plan converge` + digest-confirmed `apply` happy path, and remote sync never proves current-session visibility.

Use the current single-Skill lifecycle verbs:

```bash
loom --json --root "$REGISTRY_ROOT" skill commit "$SKILL" --from-source --message "$MESSAGE"
loom --json --root "$REGISTRY_ROOT" skill release "$SKILL" --anchor
loom --json --root "$REGISTRY_ROOT" skill diff "$SKILL" "$FROM" "$TO"
loom --json --root "$REGISTRY_ROOT" skill rollback "$SKILL" --to "$REF" --dry-run
```

Use `--from-source` or `--from-projection` only when the detected drift requires an explicit side. A release anchor is a local recovery point; do not claim it published a semantic version. Run release preflight before a real version release, and never publish a version without explicit authorization.

## Handle Sync And Operations

- Read all three `data.convergence` axes before claiming runtime completion: `registry_transport`, `projections`, and `visibility`.
- Treat `meta.sync_state` as a compatibility-only registry transport field. `LOCAL_ONLY` and `PENDING_PUSH` do not mean remotely synchronized; `SYNCED` does not mean projections converged or the current agent session loaded the Skill.
- Accept cross-axis states as evidence, not contradictions. For example, `registry_transport=SYNCED` with `projections=drifted` requires projection repair, while `projections=converged` with `visibility=restart_required` requires a new agent session.
- Treat `complete=true` as evidence-collection completeness only, never as a health verdict; inspect every axis state before declaring convergence.
- Fail closed when an axis is absent, `unknown`, `error`, `stale=true`, or named in `incomplete_axes`. Never replace missing visibility evidence with filesystem presence.
- Run `sync push --dry-run` before a real push when supported by the requested flow.
- On `REMOTE_DIVERGED`, pull and resolve explicitly; on `PUSH_REJECTED`, do not force-push.
- Preserve blocked or failed operation records. Use operation history and diagnosis before retry or repair.
- Never edit `state/registry` files directly to bypass a Loom error.

## Verify And Report

After a change:

1. Re-run the narrow read-only inspection that proves the requested outcome.
2. Surface warnings, approval decisions, operation IDs, Git commits, registry transport, projection convergence, and agent visibility separately.
3. State any restart or new-session requirement for agent discovery; file presence alone does not prove the current session loaded a Skill.
4. Do not report success from a dry-run or plan response.

For the full JSON/error contract, read [`docs/AGENT_USAGE.md`](../../docs/AGENT_USAGE.md). For creation, activation, evaluation, history, and rollback details, read [`docs/SINGLE_SKILL_LIFECYCLE.md`](../../docs/SINGLE_SKILL_LIFECYCLE.md).
