# GH381 Tasks: Org Policy, Approval, And RBAC

Issue: https://github.com/majiayu000/loom/issues/381
Product spec: `specs/GH381/product.md`
Tech spec: `specs/GH381/tech.md`
Status: Blocked design packet

## Scope For First PR

Implement Git-backed governance primitives:

```text
org policy init/show/check + approval request/list/approve/reject + role list/grant/revoke
```

Do not implement:

```text
hosted RBAC service, replacement for Git hosting permissions, local safety gate bypass
```

## Tasks

- [ ] `SP381-T001` Owner: policy-state | Done when: org policy and role state files are deterministic, human-reviewable, first-admin bootstrap is explicit, and malformed state fails closed | Verify: `cargo test --test org_policy`
- [ ] `SP381-T002` Owner: policy-cli | Done when: `policy org init/show/check` returns allow/deny/approval_required with roles, reasons, evidence, and approval commands | Verify: `cargo test --test org_policy`
- [ ] `SP381-T003` Owner: approval-store | Done when: approval request/list/approve/reject uses append-only audited events with required roles, approval requirements, and policy decision digest, checks approver roles before decision events, and computes current request state deterministically | Verify: `cargo test --test org_policy`
- [ ] `SP381-T004` Owner: roles | Done when: roles list/grant/revoke validates role names, requires admin policy for grant/revoke, preserves at least one resolved admin, and exposes resolved role grants in JSON | Verify: `cargo test --test org_policy`
- [ ] `SP381-T005` Owner: enforcement | Done when: skill install/add/new/save/capture, project, activate/deactivate, release/rollback, trust/quarantine, provider add/remove, sync pull/push/replay, autosync, and composite apply mutations call org policy before writing | Verify: `cargo test --test skill_policy && cargo test --test agent_plan_apply`
- [ ] `SP381-T006` Owner: safety | Done when: org policy approval cannot bypass local safety gates and blocked/quarantined skills remain denied | Verify: `cargo test --test skill_policy`
- [ ] `SP381-T007` Owner: regression | Done when: focused and full repository checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP381-T1: Add Org Policy State

Owner: backend

Files:

- new org policy command module
- `state/registry/org_policy.toml`
- `state/registry/roles.json` or embedded role section
- tests

Done when:

- Policy init creates deterministic defaults.
- Policy init records an explicit first admin or fails with manual bootstrap
  instructions instead of creating an unusable empty-admin policy.
- Policy show returns structured JSON.
- Malformed policy fails closed.
- Policy files do not contain secrets.

Verify:

```bash
cargo test --test org_policy
```

### SP381-T2: Add Policy Check

Owner: backend
Depends on: SP381-T1

Done when:

- `policy org check` supports lifecycle actions.
- Decisions are allow, deny, or approval_required.
- Output includes required roles, approval tokens, evidence, and next actions.
- Check is read-only.

Verify:

```bash
cargo test --test org_policy
```

### SP381-T3: Add Approval Requests

Owner: backend
Depends on: SP381-T2

Done when:

- Requests capture action, action-specific subject, requester, redacted reason,
  risk summary, evidence, required roles, required approvals, and policy decision
  digest as append-only events.
- Approve/reject appends decision events with redacted comments.
- Approve/reject verifies the current actor has one of the request's required
  roles before appending a decision event.
- Current state is derived from append-only events.
- Rejected requests do not unblock actions.

Verify:

```bash
cargo test --test org_policy
```

### SP381-T4: Add Role Management

Owner: backend
Depends on: SP381-T1

Done when:

- Roles list/grant/revoke work.
- Valid roles are viewer, author, reviewer, maintainer, admin.
- Unknown roles fail.
- Grant/revoke require an admin policy decision before writing.
- Grant/revoke cannot remove or obscure the last resolved admin.
- Missing role blocks approval.

Verify:

```bash
cargo test --test org_policy
```

### SP381-T5: Enforce Org Policy In Mutations

Owner: backend
Depends on: SP381-T2, SP381-T3

Done when:

- Mutating commands call org policy before writing.
- Skill draft writes, remote `skill add` imports, sync pull/replay, and autosync
  writes are governed, not only install and activation.
- Composite apply paths such as `use --apply` preflight all target, binding,
  projection, registry, and sync writes before any mutation lands.
- Approval-required actions return `POLICY_BLOCKED` with approval request
  command.
- Approved requests unblock only the matching action, full action-specific
  subject, and evidence digest.
- Existing local policy gates still run.

Verify:

```bash
cargo test --test skill_policy
cargo test --test agent_plan_apply
```

### SP381-T6: Release And Trust Gates

Owner: backend
Depends on: SP381-T5
Blocked by: #370

Done when:

- Release policy checks clean source, eval evidence, security scan, and reviewer
  approval.
- Trust update and quarantine enforce maintainer/admin roles.
- Blocked/quarantined skills remain denied even with unrelated approvals.

Verify:

```bash
cargo test --test org_policy
cargo test --test skill_policy
```

### SP381-T7: Full Verification

Owner: testing
Depends on: SP381-T1, SP381-T2, SP381-T3, SP381-T4, SP381-T5, SP381-T6

Done when:

- Focused tests cover every acceptance criterion.
- Full check and test suites pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #381` for design-only or partial governance slices. Use `Fixes #381`
only after org policy, roles, approvals, command enforcement, audit, and
release/trust gates are complete.
