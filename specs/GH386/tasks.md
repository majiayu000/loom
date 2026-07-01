# GH386 Tasks: MCP Provisioning

Issue: https://github.com/majiayu000/loom/issues/386
Product spec: `specs/GH386/product.md`
Tech spec: `specs/GH386/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement only the plan-first foundation:

```text
MCP requirement listing + dry-run plan + catalog/source policy + doctor next actions
```

Do not implement in the first PR:

```text
silent installs, OAuth flows, secret storage, unreviewed package execution, or
agent config mutation without explicit apply
```

## Tasks

- [ ] `SP386-T1` Owner: implementation | Done when: MCP requirement/list/plan/doctor/catalog CLI parses and command ids classify read-only behavior correctly | Verify: `cargo test --test cli_surface`
- [ ] `SP386-T2` Owner: implementation | Done when: MCP requirement parser reads `loom.skill.toml`, `SKILL.md` metadata, and agent metadata without exposing secret values | Verify: `cargo test --test mcp_provisioning`
- [ ] `SP386-T3` Owner: implementation | Done when: catalog/source policy parses scoped npm locators, rejects unpinned sources before approval unless resolved to immutable source, and approval-gates unknown pinned MCP server sources | Verify: `cargo test --test mcp_provisioning`
- [ ] `SP386-T4` Owner: implementation | Done when: `mcp plan` returns missing and existing servers, adapter-supported config diffs or manual mode, env names, risk summary, and RBAC approval requirements without writes | Verify: `cargo test --test mcp_provisioning`
- [ ] `SP386-T5` Owner: deferred-apply | Deferred until plan semantics are stable; done when `mcp apply` loads a durable plan event or explicit artifact, revalidates plans, requires idempotency/approvals, writes atomically, and preserves user config | Verify: `cargo test --test mcp_provisioning`
- [ ] `SP386-T6` Owner: implementation | Done when: `mcp doctor` and `skill diagnose` include provisioning next actions from the readiness read model | Verify: `cargo test --test mcp_provisioning`

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
- `mcp apply` is absent or returns typed not-implemented until apply gates are
  ready.

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

### SP386-T4: Implement Dry-Run Plan

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
- config diffs are generated without writes only for adapters with explicit MCP
  config path and merge support; otherwise return `manual_configuration_required`.
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
- apply requires an idempotency key.
- risky actions require policy/approval backend-issued approval ids, or explicit
  local-only consent when RBAC is not enabled.
- config writes are atomic and preserve unrelated user config.
- secrets are required by reference only and never written directly.
- plan drift fails and asks for a new plan.

Verify:

```bash
cargo test --test mcp_provisioning
```

### SP386-T6: Add Doctor Integration And Final Checks

Owner: implementation
Depends on: SP386-T4

Done when:

- `mcp doctor` reports missing servers, missing env names, policy blocks, and
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

- Use `Refs #386` for a first-slice PR unless requirement parsing, plan,
  apply, doctor, catalog, and all acceptance criteria are complete.
- Do not use `Fixes #386` until safe apply, idempotency, approval gates,
  doctor integration, and tests are implemented.
- Never print or store secret values.
