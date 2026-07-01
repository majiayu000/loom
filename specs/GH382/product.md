# GH382 Product Spec: Remote And Devcontainer Provisioning

Issue: https://github.com/majiayu000/loom/issues/382
Parent: https://github.com/majiayu000/loom/issues/376
Status: Blocked design packet
Locale: zh-CN

## Goal

Add explicit, reproducible, dry-run-first provisioning plans that reproduce
registry skills, active views, dependencies, and agent config in remote,
devcontainer, Codespaces, CI, or cloud-hosted coding environments.

Local active views are not enough when the actual agent runs elsewhere. Loom
must produce plans and artifacts that can be reviewed, applied, and recovered
without hidden background sync or secret copying.

## Blocking Dependencies

Production implementation is blocked by:

- #366 single-skill inspect/status.
- #367 activation semantics.
- #370 safety/trust/quarantine.
- #371 dependency and MCP readiness.
- #373 adapter discovery roots and reload metadata.
- #377 skillsets and bundles.
- #381 org policy/RBAC.

## User-Facing Commands

Target command surface:

```bash
loom provision plan --target devcontainer [--workspace <path>] [--agent codex] [--output-plan <path>] [--json]
loom provision apply <plan-id|plan-artifact> --idempotency-key <key> [--approve <approval-token>...]
loom provision doctor --target devcontainer|codespaces|remote --workspace <path> [--agent <agent>] [--plan <plan-id|plan-artifact>]
loom provision export <plan-id|plan-artifact> --format devcontainer|shell|tar --output <path>
loom provision import <artifact> --dry-run
```

## Target Kinds

Initial target kinds:

- `devcontainer`: generate or update `.devcontainer/devcontainer.json` and
  setup scripts.
- `codespaces`: GitHub Codespaces-specific devcontainer-compatible target with
  Codespaces environment notes.
- `shell`: generate a reproducible install/setup script.
- `tar`: export a portable registry and active-view artifact.
- `remote`: abstract target for future SSH/cloud integrations.

## Non-Goals

1. No hidden background sync daemon.
2. No direct cloud deployment without explicit provider config.
3. No secret copying by default.
4. No bypass of org policy/RBAC.
5. No target-environment mutation during `provision plan` or `provision
   doctor`; `provision plan` may write an explicit reviewed plan artifact or
   durable command event when requested.
6. No destructive merge of user-authored devcontainer files.

## Plan Behavior

`provision plan` must:

1. Inspect active skills and skillsets for the selected agent/workspace.
2. Resolve target environment paths using adapter metadata.
3. Include dependency readiness requirements from #371.
4. Include safety/trust and org policy checks.
5. Generate reviewed file diffs or artifacts, including content digests.
6. Report secrets as requirements but never copy or print values.
7. Never write target files unless `apply` is called.

## Apply Behavior

`provision apply` must:

- Revalidate the reviewed plan or plan artifact; apply must not regenerate
  unreviewed file content from current state.
- Revalidate target-file preimage digests before writing so user edits after plan
  creation fail as drift instead of being overwritten.
- Require idempotency key.
- Accept and validate approval tokens when org policy marks the reviewed plan as
  approval-required.
- Write files atomically.
- Preserve user-authored config where possible.
- Stop on merge conflicts.
- Return recovery commands.

## Acceptance Criteria

1. `provision plan --target devcontainer` outputs a reproducible plan without
   target-environment writes.
2. Generated devcontainer snippets use project `.agents/skills` for Codex
   project scope.
3. Secrets are reported as required but never copied or printed.
4. Plans include active skills, skillsets, dependencies, and policy gates.
5. `provision apply` writes atomically and is idempotent.
6. Tests cover dry-run, devcontainer file generation, existing file merge
   conflict, secret redaction, policy approval required, idempotent apply, and
   provision doctor.
