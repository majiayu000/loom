# GH386 Tasks: MCP Provisioning

Issue: https://github.com/majiayu000/loom/issues/386
Product spec: `specs/GH386/product.md`
Tech spec: `specs/GH386/tech.md`
Status: Implemented; closeout evidence recorded

## Scope For First PR

Implement the guarded plan/apply foundation:

```text
MCP requirement listing + audited durable plan + guarded Codex config apply +
catalog/source policy + doctor next actions
```

Still out of scope for this PR:

```text
silent installs, direct package install execution, OAuth flows, secret storage,
unreviewed package execution, or agent config mutation without reviewed apply
```

## Tasks

- [x] `SP386-T1` Owner: implementation | Done when: MCP requirement/list/plan/apply/doctor/catalog CLI parses and command ids classify write behavior correctly | Verify: `cargo test --test cli_surface`
- [x] `SP386-T2` Owner: implementation | Done when: MCP requirement parser reads `loom.skill.toml`, `SKILL.md` metadata, and agent metadata without exposing secret values | Verify: `cargo test --test mcp_provisioning`
- [x] `SP386-T3` Owner: implementation | Done when: catalog/source policy parses scoped npm locators, rejects mutable/unpinned sources before approval unless resolved to immutable source, and approval-gates unknown pinned MCP server sources | Verify: `cargo test --test mcp_provisioning`
- [x] `SP386-T4` Owner: implementation | Done when: `mcp plan` returns missing and existing servers, adapter-supported config diffs or manual mode, env names, risk summary, RBAC approval requirements, and writes only reviewed plan artifacts | Verify: `cargo test --test mcp_provisioning`
- [x] `SP386-T5` Owner: implementation | Done when: `mcp apply` loads a durable plan event or explicit artifact, revalidates plans, requires idempotency/approvals, writes atomically, and preserves user config | Verify: `cargo test --test mcp_provisioning --test mcp_apply_review`
- [x] `SP386-T6` Owner: implementation | Done when: `mcp doctor` and `skill diagnose` include provisioning next actions from the readiness read model | Verify: `cargo test --test mcp_provisioning`

### SP386-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs` or a split MCP args module
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom mcp requirement list --skill <skill> [--agent <agent>] [--json]` parses.
- `loom mcp plan --skill <skill> --agent <agent> [--workspace <path>] [--json]` parses.
- `loom mcp doctor --agent <agent> [--skill <skill>] [--workspace <path>] [--json]` parses.
- `loom mcp catalog search <query> [--json]` parses.
- `loom mcp catalog show <server> [--json]` parses.
- Catalog search/show JSON includes source provenance, transport, required
  package tool, trust state, and policy warnings; missing entries and malformed
  sources return typed errors.
- `mcp apply` parses and is classified as a write command.

Verify:

```bash
cargo test --test cli_surface
```

### SP386-T2: Implement Requirement Parsing

Owner: implementation
Depends on: SP386-T1

Done when:

- `loom.skill.toml` `requires_mcp` parses.
- `[mcp.<server>]` sections parse.
- supported `SKILL.md` metadata parses.
- compatibility text heuristics produce suggestions, not silent hard
  requirements.
- duplicate requirements merge deterministically.
- malformed metadata returns typed findings.
- secret refs are represented by env/secret names only.

Verify:

```bash
cargo test --test mcp_provisioning
```

### SP386-T3: Add Catalog And Source Policy

Owner: implementation
Depends on: SP386-T2

Done when:

- pinned npm locators parse, including scoped packages by splitting package and
  version at the rightmost `@` after `npm:`.
- immutable Git commit locators parse; tag inputs must store and revalidate the
  resolved commit and source digest.
- local path locators require digest.
- team catalog entries include trust metadata.
- unpinned sources are blocked before approval unless planning resolves and
  records an immutable source first; unknown pinned sources are
  approval-required or denied under policy.
- planning never executes package code.

Verify:

```bash
cargo test --test mcp_provisioning
```

### SP386-T4: Implement Audited Plan

Owner: implementation
Depends on: SP386-T2, SP386-T3

Done when:

- plan resolves requirements for a skill and agent.
- plan inspects current config through adapter metadata where available.
- missing servers, existing servers, and env vars are reported.
- existing servers match only when requirement command/source, transport, env
  names, and scope are compatible; mismatches are reported as findings, not
  reused silently.
- package/tool availability for `node`, `npx`, `uvx`, `docker`, or other source
  runtime tools is reported before any config write is considered safe.
- config diffs are generated only for adapters with explicit MCP config path
  and merge support; otherwise return `manual_configuration_required`.
- planning writes only audited reviewed plan artifacts and explicit
  `--output-plan` artifacts.
- config-write actions depend on satisfied install/env/tool prerequisites and
  remain unsafe to apply while any dependency is missing or mismatched.
- risk summary includes network, secret, package, and external-system risk.
- RBAC approval requirements or local-only consent requirements are included for
  risky actions.
- unsupported agents return manual mode instead of guessed config paths.

Verify:

```bash
cargo test --test mcp_provisioning
```

### SP386-T5: Implement Apply

Owner: implementation
Depends on: SP386-T4

Done when:

- apply revalidates source digest, policy, adapter metadata, and target config.
- apply recomputes current skill MCP requirements and resolved sources before
  writing.
- apply requires an idempotency key.
- apply replays successful records before volatile approval/env/tool gates.
- apply recovers a missing apply record when config already matches reviewed
  output.
- risky actions require policy/approval backend-issued approval ids, or explicit
  local-only consent when RBAC is not enabled.
- config writes are atomic and preserve unrelated user config.
- secrets are required by reference only, forwarded through `env_vars`, and
  never written directly.
- config locking is scoped to the target config path and stale key locks are
  reaped.
- filesystem servers are scoped to the reviewed workspace path.
- local command sources resolve to absolute digest-pinned paths.
- plan drift fails and asks for a new plan.

Verify:

```bash
cargo test --test mcp_provisioning
cargo test --test mcp_apply_review
```

### SP386-T6: Add Doctor Integration And Final Checks

Owner: implementation
Depends on: SP386-T4

Done when:

- `mcp doctor` reports missing servers, missing env names, missing launcher
  tools such as `node`, `npx`, `uvx`, or `docker`, policy blocks, and
  approval-required next actions.
- `skill diagnose` points to `mcp plan` when readiness fails.
- CLI contract documents MCP provisioning safety rules.
- tests cover first-slice acceptance criteria.
- repository checks pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

- PR #437 implemented read-only requirement, plan, catalog, and doctor
  foundation.
- PR #491 implemented guarded `mcp apply` for reviewed Codex config writes.
- Direct package installation, OAuth/browser flows, arbitrary untrusted
  package execution, and secret storage remain explicit non-goals for this
  issue rather than incomplete acceptance criteria.
- Use `Fixes #386` from the closeout PR after current verification passes.
- Never print or store secret values.
