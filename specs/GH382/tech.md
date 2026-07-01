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
  "container_workspace": "/workspaces/repo",
  "agents": ["codex"],
  "registry_source_display": "git+https://github.com/org/skills-registry.git",
  "registry_clone_url": "https://github.com/org/skills-registry.git",
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
      "preimage_digest": null,
      "content_digest": "sha256:...",
      "preview": "#!/usr/bin/env bash\\n..."
    },
    {
      "path": ".devcontainer/devcontainer.json",
      "preimage_digest": "sha256:...",
      "content_digest": "sha256:...",
      "patch": []
    }
  ],
  "secrets_required": [],
  "policy": {"requires_approval": false, "required_approvals": []},
  "loom_cli": {"required": true, "version": ">=0.1.5", "install": "preinstalled"},
  "guards": {
    "root": "/registry",
    "registry_head": "abc123",
    "registry_head_reachable": true,
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
LOOM_WORKSPACE="${LOOM_WORKSPACE:-/workspaces/repo}"
LOOM_REGISTRY_DIR="${LOOM_REGISTRY_DIR:-$LOOM_WORKSPACE/.loom-registry}"
LOOM_REGISTRY_SOURCE="https://github.com/org/skills-registry.git"
LOOM_REGISTRY_HEAD="abc123"
command -v loom >/dev/null || {
  echo "loom CLI is required before provisioning can continue" >&2
  exit 127
}
loom --version | grep -Eq 'loom (0\\.1\\.([5-9]|[1-9][0-9]+)|0\\.([2-9]|[1-9][0-9]+)\\.[0-9]+|[1-9][0-9]*\\.[0-9]+\\.[0-9]+)' || {
  echo "loom CLI version does not satisfy reviewed plan requirement >=0.1.5" >&2
  exit 127
}
if [ -d "$LOOM_REGISTRY_DIR/.git" ]; then
  git -C "$LOOM_REGISTRY_DIR" fetch origin "$LOOM_REGISTRY_HEAD"
else
  git clone "$LOOM_REGISTRY_SOURCE" "$LOOM_REGISTRY_DIR"
  git -C "$LOOM_REGISTRY_DIR" fetch origin "$LOOM_REGISTRY_HEAD"
fi
git -C "$LOOM_REGISTRY_DIR" checkout --detach "$LOOM_REGISTRY_HEAD"
mkdir -p "$LOOM_WORKSPACE/.agents/skills"
# Generated from reviewed active_views[].skills; repeat for every planned skill.
loom --root "$LOOM_REGISTRY_DIR" skill diagnose fixflow --json
# Materialize or verify the reviewed active view before success. If the
# activation/materialization command is unavailable, fail with the reviewed
# manual instructions instead of reporting success.
```

Implementation must avoid assuming `~/.codex/skills` for Codex project scope.
Use project `.agents/skills` from adapter metadata.
If activation commands are not yet implemented, generated scripts must emit
reviewed materialization instructions or fail clearly instead of calling
nonexistent `skillset activate` or `skill doctor` commands.
`registry_source` values with a `git+` scheme are normalized into a separate
cloneable `registry_clone_url` before script generation; generated shell must
not pass `git+https://...` directly to `git clone`. Remote URLs with embedded
credentials are split into a redacted display URL, a credential-free clone URL,
and a named secret requirement. Plan JSON and generated scripts must not persist
URL userinfo, token query parameters, or password-like fragments.
Paths such as the workspace, registry clone directory, and active view come from
the reviewed plan and adapter metadata, not hard-coded `/workspaces` defaults.
The script must generate diagnose/materialization checks for every planned
active skill, not for a literal example skill.
The script must verify the installed `loom --version` against the reviewed
`loom_cli.version` requirement with a comparison that accepts multi-digit semver
components; command presence alone is not enough.
Plan and doctor must verify that the exact reviewed `registry_head` is fetchable
from the credential-free clone URL. If the head is local-only, provisioning must
use a tar/export artifact path instead of a remote clone plan.

## File Merge Rules

For `.devcontainer/devcontainer.json`:

- parse JSONC with a structured parser that preserves valid devcontainer files
  containing comments
- preserve unknown fields
- add Loom setup only under a deterministic key/comment-free field strategy
- fail on incompatible existing commands unless a safe merge is implemented

For setup scripts:

- write with `set -euo pipefail`
- avoid embedding secrets
- include idempotent commands
- include verification commands

For every file write, the plan stores the target preimage digest or `null` for a
file that must not already exist. `apply` rechecks those preimages immediately
before writing and fails with plan drift when a target file changed after review.

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
10. apply rejects changed target-file preimages before writing.

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
