# GH381 Tech Spec: Org Policy, Approval, And RBAC

Issue: https://github.com/majiayu000/loom/issues/381
Product spec: `specs/GH381/product.md`
Status: Blocked design packet

## Current State

Loom has local skill policy evaluation in `src/commands/skill_policy.rs`.
Projection uses `enforce_skill_policy`, and durable `plan/apply` already emits
approval tokens from capability and risk findings.

GH381 should extend those primitives into Git-backed org policy and approval
records. It should not replace local safety gates.

Relevant files:

- `src/commands/skill_policy.rs`
- `src/commands/plan_cmds.rs`
- `src/commands/skill_cmds.rs`
- `docs/LOOM_CLI_CONTRACT.md`
- `tests/skill_policy.rs`
- `tests/agent_plan_apply.rs`

## State Model

Recommended files:

```text
state/registry/org_policy.toml
state/registry/approvals.jsonl
state/registry/roles.json
```

`org_policy.toml` is human-reviewable. `approvals.jsonl` is append-only.
`roles.json` stores deterministic resolved role grants when roles are not kept
inside the TOML file.

Policy example:

```toml
schema = "loom.policy.v1"

[roles]
reviewer = ["alice", "team:platform"]
maintainer = ["team:devtools"]
admin = ["alice"]

[requirements.skill_activate]
third_party_unreviewed = "approval_required"
high_risk = "approval_required"
blocked = "deny"
quarantined = "deny"

[requirements.skill_release]
requires_clean_source = true
requires_eval_pass = true
requires_security_scan = true
requires_reviewer_approval = true
```

## Policy Evaluator

Add a shared org policy evaluator:

```rust
fn evaluate_org_policy(ctx, action, subject) -> OrgPolicyDecision
```

Decision output:

```json
{
  "action": "skill.activate",
  "decision": "approval_required",
  "required_roles": ["reviewer"],
  "required_approvals": ["approval:reviewer"],
  "reasons": [],
  "evidence": {}
}
```

The evaluator should join:

- single-skill inspect status from #366
- trust/quarantine state from #370
- provider trust from #380
- existing local policy findings
- existing plan approval token semantics

## Enforcement

Mutating commands should call org policy before writing:

- install
- activate/deactivate
- release/rollback
- trust update
- quarantine
- provider add/remove
- sync push

If approval is required, return `POLICY_BLOCKED` with:

- `decision`
- `required_roles`
- `required_approvals`
- `approval_request_command`
- evidence references

Approval tokens supplied to apply commands must be validated against approved
requests, not just string equality, once org policy is enabled.

## Approval Store

Approval request lifecycle:

1. request
2. approve
3. reject
4. supersede or expire in a later slice

Append-only records:

```json
{"event":"requested","request_id":"approval_...","...": "..."}
{"event":"approved","request_id":"approval_...","actor":"alice","comment":"..."}
```

Derived request state is computed from events. Malformed approval logs fail
closed.

## Roles

Role grants must be deterministic and auditable. Role resolution should support:

- local username
- Git author identity where available
- `team:<name>` labels as policy subjects

Team membership resolution can remain manual in v1. Do not require a hosted
service.

## Tests

Focused tests:

1. `policy org init` creates deterministic policy.
2. `policy org show` returns JSON roles and requirements.
3. `policy org check` returns allow, deny, and approval_required.
4. approval request/approve/reject appends auditable records.
5. approved request unblocks the matching action.
6. rejected request remains blocked.
7. missing role blocks approval.
8. blocked/quarantined skill denies action.
9. release preflight enforces clean source/eval/scan requirements.
10. local safety gates still run even when org policy allows the action.

## Verification

```bash
git diff --check
cargo test --test org_policy
cargo test --test skill_policy
cargo test --test agent_plan_apply
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #381` for design-only or partial governance slices. Use `Fixes #381`
only after org policy, roles, approval requests, command enforcement, audit, and
tests satisfy the issue acceptance criteria.
