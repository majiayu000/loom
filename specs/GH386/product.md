# GH386 Product Spec: MCP Provisioning

Issue: https://github.com/majiayu000/loom/issues/386
Parent: https://github.com/majiayu000/loom/issues/376
Status: Draft for implementation
Locale: en-US

## Goal

Add safe MCP server install and configuration automation tied to skill
requirements, agent adapters, dependency readiness, and policy. Loom should
help users plan and apply MCP configuration changes without silently installing
packages, authenticating services, or writing secrets.

## Users

1. Individual users who need clear next actions when a skill requires MCP.
2. Teams that need policy approval before granting agents external access.
3. Maintainers who need deterministic requirement parsing, config diffs, and
   idempotent apply behavior.

## Scope For First PR

The first mergeable slice should implement the non-destructive foundation:

- requirement listing from `loom.skill.toml`, `SKILL.md` metadata, compatibility
  text, and agent metadata;
- `mcp plan` dry-run output with missing servers, config diffs, env
  requirements, risk summary, and RBAC approval requirements;
- catalog source policy model for pinned package, Git, local, and team catalog
  locators;
- `mcp doctor` next actions when dependency readiness fails;
- `mcp apply` design guarded behind revalidation, idempotency keys, approvals,
  atomic config writes, and secret non-storage.

Implementation may defer actual apply writes until plan semantics and adapter
metadata are stable.

## Non-Goals

1. No silent credential creation or secret storage.
2. No automatic OAuth or browser authentication flows in v1.
3. No installing arbitrary untrusted MCP packages without catalog/provenance
   checks.
4. No agent-specific config mutation without plan/apply.
5. No bypass of trust, quarantine, RBAC, or org policy.
6. No execution of newly installed MCP servers during planning.

## Behavior Invariants

1. `mcp requirement list` and `mcp plan` are read-only.
2. `mcp plan` includes config diffs and risk summary but does not write agent
   config, registry state, secrets, or package installs.
3. Secret values are never printed, exported, logged, or stored.
4. Missing secrets are represented by variable names and redacted status only.
5. Unpinned or unknown MCP server packages are blocked or approval-required
   under policy.
6. Apply must consume a durable plan event or explicit plan artifact and
   revalidate it against current skill source, policy, adapter metadata, and
   target config before writing.
7. Apply must be idempotent and require an idempotency key.
8. Config writes must be atomic and preserve user-authored config where
   possible.
9. Unsupported agents return manual configuration plans rather than guessing
   paths.
10. `skill diagnose` and `mcp doctor` report next actions without silently
    installing or authenticating servers.

## User-Facing CLI

Required first-slice commands:

```bash
loom mcp requirement list --skill <skill> [--agent <agent>] [--json]
loom mcp plan --skill <skill> --agent <agent> [--workspace <path>] [--json]
loom mcp doctor --agent <agent> [--skill <skill>] [--workspace <path>] [--json]
loom mcp catalog search <query> [--json]
loom mcp catalog show <server> [--json]
```

Deferred apply command:

```bash
loom mcp apply <plan-id|plan-artifact> --idempotency-key <key> [--approve <approval-id[,approval-id]>]
```

## Requirement Model

Skill MCP requirements may come from:

1. `loom.skill.toml` `requires_mcp`;
2. `[mcp.<server>]` sections;
3. `SKILL.md` `metadata.loom.requires_mcp`;
4. compatibility text heuristics;
5. agent-specific metadata;
6. marketplace or provider metadata after provider support lands.

Example:

```toml
requires_mcp = ["github", "filesystem"]

[mcp.github]
required = true
transport = "stdio"
package = "@modelcontextprotocol/server-github"
auth = "env:GITHUB_TOKEN"
permissions = ["repo:read", "issues:write"]
```

## Plan Model

`mcp plan` should return:

```json
{
  "plan_id": "mcpplan_...",
  "agent": "codex",
  "skill": "fixflow",
  "actions": [
    {
      "kind": "install_server",
      "server": "github",
      "source": "npm:@modelcontextprotocol/server-github@1.2.3",
      "safe_to_apply": false,
      "approval_required": "install-third-party-mcp"
    },
    {
      "kind": "write_agent_config",
      "path": "/Users/me/.codex/config.toml",
      "diff": "@@ ...",
      "safe_to_apply": true
    },
    {
      "kind": "require_env",
      "name": "GITHUB_TOKEN",
      "present": false,
      "safe_to_apply": false
    }
  ],
  "risk_summary": {
    "network_access": true,
    "secrets_required": ["GITHUB_TOKEN"],
    "external_package": true
  }
}
```

## Acceptance Criteria

1. `mcp requirement list --skill <skill> --json` shows requirements from
   `loom.skill.toml` and supported skill metadata.
2. `mcp plan` identifies missing servers, existing servers, config diffs, env
   vars, source provenance, and approval requirements.
3. `mcp plan` is read-only and writes no agent config or package state.
4. Secret values are never printed or stored.
5. Unpinned MCP server sources are blocked before approval unless planning first
   resolves and records an immutable version, commit, or source digest; untrusted
   pinned sources may be approval-required.
6. Unsupported agents return `manual_configuration_required` with required
   server details.
7. `mcp apply` consumes a durable plan event or explicit plan artifact,
   revalidates plans, writes config atomically, and is idempotent once apply is
   implemented.
8. `skill diagnose` includes MCP provisioning next actions when readiness
   fails.
9. Tests cover requirement parsing, missing and existing server plans,
   adapter-supported config diff generation, env secret redaction, unpinned
   rejection, approval-required actions, malformed config, and unsupported agent
   manual mode. Idempotent apply tests are required once apply is implemented.

## Open Questions

1. Whether catalog entries live in repository state, a signed team catalog, or
   provider metadata.
2. Whether apply should install packages directly or only write agent config
   pointing to preinstalled commands in v1.
3. Whether policy approval ids should be reusable across plan revalidation or
   single-use only.
