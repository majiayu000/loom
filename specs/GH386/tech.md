# GH386 Tech Spec: MCP Provisioning

Issue: https://github.com/majiayu000/loom/issues/386
Product spec: `specs/GH386/product.md`
Status: Draft for implementation

## Design Summary

Add MCP provisioning as a plan-first workflow. Read-only commands resolve skill
requirements, inspect current agent configuration, check package/tool
availability, and return a structured plan. Mutating apply is permitted only
after revalidation, idempotency, approval, policy checks, and atomic config
writes are in place.

## Dependencies And Blocks

| Issue | Required capability |
|---|---|
| #371 | dependency and MCP readiness checks |
| #373 | agent adapter metadata and config path discovery |
| #370 | safety, trust, quarantine, and security gates |
| #381 | org policy, RBAC, and approvals |
| #382 | provisioning target and plan/apply conventions |

The first implementation may produce manual or blocked plans when an upstream
primitive is unavailable. It must not silently guess config paths or trust
unknown MCP packages.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs` or split MCP args module |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| MCP implementation | new `src/commands/mcp.rs` or module directory |
| requirement parsing | skill metadata/read model modules |
| adapter config | agent adapter metadata after #373 |
| policy | policy/approval module after #381 |
| tests | new `tests/mcp_provisioning.rs`, CLI tests |
| docs/specs | `specs/GH386/*`, CLI contract docs |

## Requirement Resolution

Resolution order:

1. parse `loom.skill.toml` `requires_mcp`;
2. parse `[mcp.<server>]` detail sections;
3. parse `SKILL.md` `metadata.loom.requires_mcp`;
4. inspect compatibility text for conservative hints;
5. consume agent-specific metadata when present;
6. consume provider/catalog metadata only when provenance is trusted.

Rules:

1. structured metadata wins over heuristics;
2. heuristics can add warnings or suggested requirements, not silent hard
   requirements;
3. duplicate server requirements are merged deterministically;
4. malformed requirement metadata returns typed findings;
5. secret references are represented as `env:NAME` or secret-provider handles,
   never values.

## Catalog And Source Policy

Supported source locators:

```text
npm:<package>@<version>
git:<url>#<tag-or-commit>
local:<path>@sha256:<digest>
catalog:<server>@<version>
```

Policy rules:

1. unpinned package locators are blocked under strict policy;
2. unknown package sources require approval or fail;
3. local paths require digest and user acknowledgement;
4. team catalog entries include trust metadata and allowed permission scopes;
5. install plans never execute package code during planning.

## Plan Model

Suggested Rust model:

```rust
struct McpPlan {
    plan_id: String,
    skill: String,
    agent: String,
    workspace: Option<PathBuf>,
    source_digest: String,
    adapter_digest: Option<String>,
    actions: Vec<McpPlanAction>,
    risk_summary: McpRiskSummary,
    approvals_required: Vec<String>,
}
```

Action kinds:

```text
install_server
write_agent_config
require_env
require_secret_provider
manual_configuration_required
restart_agent
```

Plan output should include:

1. current config summary;
2. proposed config diff;
3. package/source provenance;
4. secrets required by name only;
5. network/file-system/external-system risk summary;
6. policy decisions and approval tokens;
7. restart or new-session guidance.

## Apply Semantics

`mcp apply` should remain deferred until plan semantics are tested. When
implemented:

1. load the saved or reproducible plan;
2. revalidate skill source digest, adapter metadata, policy, and target config;
3. require idempotency key;
4. require approval tokens for risky actions;
5. reject missing secret values without printing them;
6. write config atomically through parse/merge/format APIs;
7. preserve unrelated user config;
8. record rollback metadata for config writes;
9. return restart/new-session guidance.

Plan drift must fail with a typed result and require a new plan.

## Agent Config Support

Agent config paths and merge semantics must come from adapter metadata after
#373. If an adapter lacks MCP config support:

1. return `manual_configuration_required`;
2. include server name, source, transport, required env var names, and
   documentation links when known;
3. do not guess file paths.

## Doctor Integration

`mcp doctor` and `skill doctor` should reuse the same requirement and readiness
read model:

1. present missing servers;
2. present missing env vars by name only;
3. present policy blocks;
4. present approval-required next actions;
5. link to `mcp plan` command.

## Test Plan

Focused tests:

1. parse `requires_mcp` from `loom.skill.toml`;
2. parse `[mcp.<server>]` sections;
3. parse supported `SKILL.md` metadata;
4. malformed metadata returns typed findings;
5. plan detects missing server;
6. plan renders config diff without writing;
7. env secrets are redacted by value and named only;
8. unpinned source is blocked or approval-required;
9. unsupported agent returns manual mode;
10. apply revalidates and rejects drift once apply is implemented;
11. apply is idempotent and atomic once apply is implemented.

Suggested commands:

```bash
git diff --check
cargo test --test mcp_provisioning
cargo check --workspace --all-targets --all-features
```

Run SpecRail workflow validation for this packet when available.

## Rollback

The first slice should be isolated to requirement parsing, plan models, CLI
commands, tests, docs, and optional read-only catalog data. Rollback removes
the MCP command group and plan model without changing skill source, existing
agent config, registry state, or secrets.

## Risks

1. Secret leakage. Mitigation: never accept or print secret values in plan
   output; test redaction paths.
2. Unsafe package install. Mitigation: pinned provenance and approval gates.
3. Config corruption. Mitigation: parse/merge/format APIs and atomic writes.
4. Agent mismatch. Mitigation: adapter metadata owns config paths; unsupported
   agents use manual mode.
