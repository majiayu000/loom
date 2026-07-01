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
state/registry/team_members.toml
```

`org_policy.toml` is human-reviewable. `approvals.jsonl` is append-only.
`roles.json` stores deterministic resolved role grants when roles are not kept
inside the TOML file. `team_members.toml` is optional manual state for resolving
offline team grants; absent team mappings mean `team:<name>` grants do not match
the current actor.

Policy example:

```toml
schema = "loom.policy.v1"

[roles]
reviewer = ["alice", "team:platform"]
maintainer = ["team:devtools"]
admin = ["alice"]

[requirements."skill.activate"]
third_party_unreviewed = "approval_required"
high_risk = "approval_required"
blocked = "deny"
quarantined = "deny"

[requirements."skill.release"]
requires_clean_source = true
requires_eval_pass = true
requires_security_scan = true
requires_reviewer_approval = true
```

Requirement keys use the same canonical dotted action ids as policy decisions.
TOML must encode these as quoted keys, such as
`[requirements."skill.activate"]`. Implementations may accept older underscore
keys such as `[requirements.skill_activate]` only through an explicit
normalization layer that is covered by tests and that fails on ambiguous
duplicates.

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

Before evaluation, command ids and CLI subjects are normalized into canonical
policy action ids and action-specific subjects. The normalization table must
cover existing dispatcher ids that are less specific than the governed action,
including `workspace.remote` to `workspace.remote.set` for
`workspace remote set`. Commands that produce multiple writes, such as
autosaving `skill watch`, expand into a deterministic batch of per-write policy
checks before any mutation lands.

## Enforcement

Mutating commands should call org policy before writing:

- install
- remote skill add/import
- observed skill import/monitor updates
- skill new/save/capture draft writes
- skill watch autosave writes
- skill snapshot and provenance refresh
- trash add/restore/purge
- orphan projection cleanup
- project
- activate/deactivate
- release/rollback
- trust update
- quarantine
- provider add/remove
- target add/remove
- workspace binding add/remove
- workspace remote set
- sync pull/push/replay, including autosync writes inherited from an enclosing
  mutation decision
- ops retry/purge/history repair

If approval is required, return `POLICY_BLOCKED` with:

- `decision`
- `required_roles`
- `required_approvals`
- `approval_request_command`
- evidence references

Composite apply flows such as `use --apply` and `apply` must evaluate every
planned write before mutating target, binding, projection, registry, or sync
state. A partial allow must not let earlier writes land before a later denied
write is discovered.

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
{"event":"requested","request_id":"approval_...","required_roles":["reviewer"],"required_approvals":["approval:reviewer"],"policy_decision_digest":"sha256:...","...": "..."}
{"event":"approved","request_id":"approval_...","actor":"alice","satisfied_approval":"approval:reviewer","comment_redacted":"..."}
```

Derived request state is computed from events. Malformed approval logs fail
closed. Decision commands reject additional approve/reject events for terminal
approved or rejected requests unless a later explicit supersede flow creates a
new request id.

## Roles

Role grants must be deterministic and auditable. Role resolution should support:

- local username
- verified local actor mappings, such as reviewed username/email bindings
- `team:<name>` labels as policy subjects

Git author identity is audit evidence only unless the repository has a trusted
signature or reviewed actor-mapping mechanism. Implementations must not grant
roles from mutable Git author strings alone.

Team membership resolution is manual in v1. Use a deterministic, reviewed
mapping such as:

```toml
[teams.platform]
members = ["alice", "bob@example.com"]
```

Implementations must not accept self-asserted team membership from a command
argument. If a team grant has no local mapping, role resolution reports an
unresolved team and fails closed for approvals or admin checks. Do not require a
hosted service.

Grant/revoke operations require an admin policy decision before writing. The
first admin must come from an explicit init-time bootstrap grant or a manually
reviewed roles file; normal grant commands must not bypass admin checks.
Revoking or rewriting roles must preserve at least one resolved admin; a change
that would leave no admin fails closed with a typed policy error.

## Tests

Focused tests:

1. `policy org init` creates deterministic policy.
2. `policy org show` returns JSON roles and requirements.
3. `policy org check` returns allow, deny, and approval_required.
4. approval request/approve/reject appends auditable records.
5. approved request unblocks the matching action.
6. rejected request remains blocked.
7. missing or unauthorized approver role blocks approval.
8. blocked/quarantined skill denies action.
9. release preflight enforces clean source/eval/scan requirements.
10. local safety gates still run even when org policy allows the action.
11. initial admin bootstrap is explicit and fresh empty policy does not deadlock.
12. approvals match the full action-specific subject, not only skill/evidence.
13. team grants resolve only through deterministic local membership mappings.
14. remote import, draft-write, sync pull/replay, and autosync paths are governed.
15. composite apply flows preflight every planned write before any mutation.
16. role changes cannot remove the last resolved admin.

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
