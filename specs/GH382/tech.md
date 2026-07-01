# GH382 Tech Spec: Remote And Devcontainer Provisioning

Issue: https://github.com/majiayu000/loom/issues/382
Product spec: `specs/GH382/product.md`
Status: Blocked design packet

## Current State

Loom has registry sync, remote status, target/binding/projection state, and an
agent adapter protocol. Adapter v1 currently exposes default skill dirs; GH373
extends this toward discovery roots, visibility, and reload semantics. GH382
must consume adapter metadata rather than hard-coding Codex/Claude remote paths.

Relevant files:

- `src/agent_adapters.rs`
- `docs/AGENT_ADAPTERS.md`
- `src/commands/sync_cmds.rs`
- `src/commands/workspace_cmds/remote.rs`
- `docs/plan/codex-active-view-projection-spec.md`

## Plan Model

Recommended plan shape:

```json
{
  "plan_id": "prov_...",
  "schema_version": "provision-plan-v1",
  "target_kind": "devcontainer",
  "workspace": "/repo",
  "agents": ["codex"],
  "registry_source": "git+https://github.com/org/skills-registry.git",
  "active_views": [
    {
      "agent": "codex",
      "scope": "project",
      "path": "/workspaces/repo/.agents/skills",
      "skills": ["fixflow", "plan-flow"],
      "skillsets": ["coding-flow"]
    }
  ],
  "dependency_readiness": [
    {"skill": "fixflow", "status": "ready", "digest": "sha256:..."}
  ],
  "files_to_write": [
    {
      "path": ".devcontainer/loom-setup.sh",
      "content_digest": "sha256:...",
      "preview": "#!/usr/bin/env bash\\n..."
    },
    {
      "path": ".devcontainer/devcontainer.json",
      "content_digest": "sha256:...",
      "patch": []
    }
  ],
  "secrets_required": [],
  "policy": {"requires_approval": false},
  "guards": {
    "root": "/registry",
    "registry_head": "abc123",
    "active_view_digest": "sha256:...",
    "skillset_digest": "sha256:...",
    "dependency_readiness_digest": "sha256:..."
  }
}
```

Plans should be stored through the same durable plan/event mechanisms used by
other apply flows where practical, or written as an explicit reviewed plan
artifact when `--output-plan` is supplied. `apply` must consume the durable plan
or artifact that contains the reviewed file changes, not regenerate unreviewed
content from current state.

## Devcontainer Output

Generated setup script should be explicit:

```bash
#!/usr/bin/env bash
set -euo pipefail
git clone "$LOOM_REGISTRY_SOURCE" /workspaces/.loom-registry
git -C /workspaces/.loom-registry fetch origin "$LOOM_REGISTRY_HEAD"
git -C /workspaces/.loom-registry checkout --detach "$LOOM_REGISTRY_HEAD"
loom --root /workspaces/.loom-registry skill diagnose fixflow --json
```

Implementation must avoid assuming `~/.codex/skills` for Codex project scope.
Use project `.agents/skills` from adapter metadata.
If activation commands are not yet implemented, generated scripts must emit
reviewed materialization instructions or fail clearly instead of calling
nonexistent `skillset activate` or `skill doctor` commands.

## File Merge Rules

For `.devcontainer/devcontainer.json`:

- parse JSON with a structured parser
- preserve unknown fields
- add Loom setup only under a deterministic key/comment-free field strategy
- fail on incompatible existing commands unless a safe merge is implemented

For setup scripts:

- write with `set -euo pipefail`
- avoid embedding secrets
- include idempotent commands
- include verification commands

## Secrets

Provisioning may report:

- secret names
- required environment variable names
- where the operator should configure them

Provisioning must not print, store, copy, or export secret values.

## Doctor

`provision doctor` should check:

- target kind support
- workspace path
- generated file presence
- registry remote availability
- adapter path compatibility
- active-view expected paths
- missing dependencies
- required secrets names
- policy approval status

It must be read-only.

## Tests

Focused tests:

1. plan creates no files.
2. devcontainer snippets use `.agents/skills` for Codex project scope.
3. shell output is deterministic and idempotent.
4. tar export omits secrets.
5. existing devcontainer conflict fails without overwrite.
6. required secrets are redacted.
7. policy approval required appears in plan.
8. apply is idempotent with the same key.
9. doctor is read-only and reports missing generated files.

## Verification

```bash
git diff --check
cargo test --test provision_cli
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #382` for design-only or partial provisioning slices. Use
`Fixes #382` only after plan, apply, export/import, doctor, idempotency,
redaction, and policy gates satisfy the acceptance criteria.
