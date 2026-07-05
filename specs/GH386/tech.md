# GH386 Tech Spec: MCP Provisioning

Issue: https://github.com/majiayu000/loom/issues/386
Product spec: `specs/GH386/product.md`
Status: Implemented; closeout evidence recorded

## Design Summary

Add MCP provisioning as a plan-first workflow. Read-only commands resolve skill
requirements, inspect current agent configuration, and check package/tool
availability. Planning persists an audited durable reviewed plan artifact, and
mutating apply is limited to reviewed Codex config writes after revalidation,
idempotency, approval, policy checks, and atomic config writes. Direct package
installation and authentication flows remain deferred.

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
3. parse `SKILL.md` nested `metadata.loom.requires_mcp` and dotted
   `metadata["loom.requires_mcp"]`;
4. parse scalar string-map values for dotted MCP metadata, accepting single
   values and comma-separated values after trimming empty entries;
5. parse existing dependency-readiness env requirement metadata such as
   `requires_env` so plans can report token blockers even when `[mcp.<server>]`
   omits `auth`;
6. inspect compatibility text for conservative hints;
7. consume agent-specific metadata when present;
8. consume provider/catalog metadata only when provenance is trusted.

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
git:<url>#<commit>
local:<path>@sha256:<digest>
catalog:<server>@<version>
```

NPM locator parsing must handle scoped packages such as
`npm:@modelcontextprotocol/server-github@1.2.3` by splitting the package/version
at the last `@` after the `npm:` prefix. Unscoped packages use the same
rightmost-version separator rule.

Policy rules:

1. unpinned package locators are blocked under strict policy before approval, or
   converted during planning into a pinned version/digest-backed source before
   any approval can apply;
2. unknown package sources require approval or fail;
3. Git locators must resolve to immutable commits; tag input is allowed only
   when the plan stores and revalidates the resolved commit and source digest;
4. local paths require digest and user acknowledgement;
5. team catalog entries include trust metadata and allowed permission scopes;
6. install plans never execute package code during planning.

## Plan Model

Suggested Rust model:

```rust
struct McpPlan {
    plan_id: String,
    skill: String,
    agent: String,
    workspace: Option<PathBuf>,
    skill_source_digest: String,
    resolved_sources: Vec<McpResolvedSource>,
    adapter_digest: Option<String>,
    actions: Vec<McpPlanAction>,
    risk_summary: McpRiskSummary,
    approvals_required: Vec<String>,
    tool_availability: Vec<McpToolAvailability>,
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

1. current config summary, including existing matching servers and their source,
   command, transport, env-name, and scope fingerprints when available;
2. proposed config diff;
3. package/source provenance, including resolved commit/integrity digest per MCP
   server source;
4. secrets required by name only;
5. package/tool availability for required runtimes such as `node`, `npx`, `uvx`,
   and `docker`;
6. network/file-system/external-system risk summary;
7. policy decisions and RBAC-issued approval requirements;
8. restart or new-session guidance.

Existing servers are reusable only when command/source, transport, env-name, and
scope requirements are compatible with the skill requirement. Mismatches are
findings that keep dependent config writes unsafe until resolved.

## Apply Semantics

`mcp apply` is implemented for reviewed Codex MCP config writes only. It must:

1. load the durable plan event or explicit plan artifact;
2. revalidate the current skill source digest and recomputed required MCP
   requirements/resolved sources against the reviewed plan;
3. revalidate every resolved MCP server source digest, adapter metadata,
   package/tool availability, policy, and target config preimage;
4. require an idempotency key and replay a successful record before volatile
   approval/env/tool gates;
5. recover a missing apply record without volatile gates when target config
   already exactly matches the reviewed rendered config;
6. require approval ids issued and validated by the policy/approval backend
   for risky actions, or explicitly mark local-only consent when RBAC is not
   enabled;
7. reject missing secret values without printing them;
8. write config atomically through parse/merge/format APIs;
9. preserve unrelated user config, including unknown per-server settings;
10. forward auth variable names through Codex `env_vars` rather than writing
    literal secret placeholders;
11. use config-scoped locking and recover stale idempotency-key locks;
12. scope filesystem servers to the reviewed workspace path;
13. render only immutable pinned sources and absolute local command paths;
14. reject `--output-plan` paths inside skill source directories;
15. return restart/new-session guidance.

Plan drift must fail with a typed result and require a new plan.
Config writes depend on satisfied install, package/tool, env, and policy
prerequisites. Apply must not write agent config first and hope a later install
or env step succeeds. Apply must not execute direct package installs in this
slice.

## Agent Config Support

Agent config paths and merge semantics must come from adapter metadata after
#373. Generate config diffs only for adapters that explicitly expose MCP config
path and merge metadata. If an adapter lacks MCP config support:

1. return `manual_configuration_required`;
2. include server name, source, transport, required env var names, and
   documentation links when known;
3. do not guess file paths.

## Doctor Integration

`mcp doctor` and `skill diagnose` should reuse the same requirement and readiness
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
5. plan detects missing and existing servers;
6. existing server command/source/transport/env/scope mismatch is reported;
7. package/tool availability is included in plan criteria;
8. plan renders config diff only for adapters with explicit MCP config support
   and writes only audited reviewed plan artifacts;
9. env secrets are redacted by value and named only;
10. unpinned source is blocked before approval unless the plan resolves an
   immutable source first;
11. unsupported agent returns manual mode;
12. apply revalidates and rejects drift;
13. apply is idempotent, atomic, and preserves unrelated config;
14. apply recovers missing records, uses config-scoped locks, rejects mutable
    package specs, recomputes current sources, and forwards auth names via
    `env_vars`.

Suggested commands:

```bash
git diff --check
cargo test --test mcp_provisioning
cargo test --test mcp_apply_review
cargo check --workspace --all-targets --all-features
```

Run SpecRail workflow validation for this packet when available.

## Rollback

The first slice is isolated to requirement parsing, plan models, CLI commands,
guarded Codex config apply, tests, docs, and optional catalog data. Rollback
removes the MCP command group and plan/apply model; successful apply changes can
be reverted by restoring the previous Codex config file from normal user
backups or VCS-managed config state. No skill source, package install state, or
secrets are written by apply.

## Risks

1. Secret leakage. Mitigation: never accept or print secret values in plan
   output; test redaction paths.
2. Unsafe package install. Mitigation: pinned provenance and approval gates.
3. Config corruption. Mitigation: parse/merge/format APIs and atomic writes.
4. Agent mismatch. Mitigation: adapter metadata owns config paths; unsupported
   agents use manual mode.
