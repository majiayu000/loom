# GH370 Tech Spec: Safety Scan, Trust, Quarantine, And Security Diff

Issue: https://github.com/majiayu000/loom/issues/370
Product spec: `specs/GH370/product.md`
Status: Draft for implementation

## Design Summary

Extend the existing policy system into a unified safety model:

1. Reuse `evaluate_skill_policy` as a source of capability/provenance/script findings.
2. Add trust metadata as registry-owned state.
3. Add `skill scan` as the user-facing safety report.
4. Add trust/quarantine mutating commands with registry audit and commit semantics.
5. Enforce blocked/quarantined state before projection/activation.
6. Add security diff by scanning only changed security-relevant files/content between refs.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new `src/cli/safety.rs` or extended `src/cli/policy.rs` |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| safety model | new `src/commands/skill_safety.rs` or extended `src/commands/skill_policy.rs` |
| registry state | `src/state_model/mod.rs`, persistence helpers for trust metadata |
| projection gate | `src/commands/skill_cmds.rs`, future #367 activation module |
| inventory/inspect | `src/commands/skill_inventory.rs`, future #366 inspect module |
| tests | new `tests/skill_safety.rs`, extend `tests/skill_policy.rs`, `tests/skill_inventory_cli.rs` |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH370/*` |

## Registry State

Add an optional registry file:

```text
state/registry/trust.json
```

Shape:

```json
{
  "schema_version": 1,
  "skills": [
    {
      "skill_id": "fixflow",
      "trust": "reviewed",
      "quarantined": false,
      "reason": null,
      "updated_at": "2026-07-01T00:00:00Z",
      "updated_by": "local-user"
    }
  ]
}
```

Rules:

1. Sort by `skill_id`.
2. Treat absent file as all skills `trust=unknown`, `quarantined=false`.
3. Reject malformed file with typed state error; do not overwrite.
4. Do not store trust inside `SKILL.md`.

## Safety Decision

Create a shared decision function:

```rust
fn evaluate_skill_safety(ctx: &AppContext, skill: &str, mode: SafetyMode) -> Result<SkillSafetyReport>
```

It should:

1. load trust metadata;
2. run existing `evaluate_skill_policy`;
3. add instruction/security heuristics not already covered by policy;
4. map findings to severity counts;
5. produce `decision` and `activation_allowed`.

Existing `enforce_skill_policy` should either call this function or be wrapped by projection/activation gates so blocked/quarantined state cannot bypass policy.

## Commands

### scan

Read-only:

```bash
loom skill scan <skill> [--mode install|activate|release] [--strict]
```

Returns safety report. `--strict` upgrades selected warnings to blockers.

### trust

Mutating:

```bash
loom skill trust <skill> --level <level>
```

Validates skill exists, writes `trust.json`, records operation `skill.trust`, commits registry state, and preserves source files.

### quarantine / unquarantine

Mutating:

```bash
loom skill quarantine <skill> [--reason <text>]
loom skill unquarantine <skill>
```

Quarantine sets `trust=quarantined` or `quarantined=true` consistently. It should deactivate/hide active projections only through safe #367/#368 mechanisms when available; if those mechanisms are not implemented yet, it must at least block future projection/activation and report existing active projections as requiring cleanup.

### diff --security

Read-only:

```bash
loom skill diff --security <skill> <from> <to>
```

Use Git diff to list changed paths, then scan changed security-relevant content. Do not show unrelated full file diffs.

## Finding Model

Use stable finding ids:

```text
instruction_prompt_injection
instruction_secret_exfiltration
description_overtrigger
script_network_access
script_secret_read
script_destructive_command
script_shell_injection
script_external_write
provenance_missing
provenance_digest_mismatch
permission_allowed_tools_broad
dependency_undeclared_tool
trust_blocked
trust_quarantined
```

Fields:

```json
{
  "id": "script_network_access",
  "severity": "high",
  "path": "scripts/install.sh",
  "line": 12,
  "message": "script invokes curl",
  "suggested_action": "review network destination and pin checksum"
}
```

## Gating

Projection/activation gate:

1. Evaluate safety for `mode=activate`.
2. If trust is `blocked` or `quarantined`, return `POLICY_BLOCKED` or a more specific safety error with full report.
3. If strict mode or binding policy requires blocking high findings, block before target mutation.
4. Never write target files before safety decision completes.

## Test Plan

Focused tests:

1. harmless skill scan allowed.
2. network script produces high finding.
3. secret-reading script produces high/critical finding.
4. prompt-injection-like instruction produces finding.
5. trust command persists sorted trust state and records operation.
6. blocked/quarantined skill blocks `skill project`.
7. quarantine preserves source and reports active projection cleanup need.
8. unquarantine clears quarantine without elevating to reviewed.
9. inventory exposes trust state.
10. security diff reports new script/network/frontmatter risk only.

Suggested commands:

```bash
git diff --check
cargo test --test skill_safety
cargo test --test skill_policy
cargo test --test skill_inventory_cli
cargo check --workspace --all-targets --all-features
```

For a spec-only PR:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH370-safety-trust-spec/specs/GH370
```

## Rollback

Rollback can remove:

1. trust registry file support;
2. safety command module and CLI args;
3. projection/activation safety gate additions;
4. inventory/inspect trust display;
5. focused tests and docs updates.

Existing `skill policy` behavior must be restored if safety unification is reverted.

## Risks

1. False positives blocking useful local skills. Mitigation: trust levels and strict mode keep review workflow explicit.
2. False negatives from heuristic scan. Mitigation: output states scan is not a sandbox or malware guarantee.
3. Quarantine accidentally deleting source or external entries. Mitigation: quarantine metadata first; projection cleanup only through safe owned-projection mechanisms.
4. Divergence from `skill policy`. Mitigation: one shared decision function feeds both scan and projection/activation gates.
