# GH381 Product Spec: Org Policy, Approval, And RBAC

Issue: https://github.com/majiayu000/loom/issues/381
Parent: https://github.com/majiayu000/loom/issues/376
Status: Blocked design packet
Locale: zh-CN

## Goal

Add Git-backed team and organization governance primitives for skill lifecycle
actions without requiring a hosted RBAC service in v1.

Teams need policy checks, approval requests, role assignments, and audit trails
for installing, activating, releasing, rolling back, trusting, quarantining, and
syncing skills. These gates must extend existing local safety checks, not bypass
them.

## Blocking Dependencies

Production implementation is blocked by:

- #366 single-skill inspect/status.
- #367 activation semantics.
- #370 safety/trust/quarantine.
- #380 provider/catalog install.

## User-Facing Commands

Target command surface:

```bash
loom policy org init [--bootstrap-admin <user>]
loom policy org show
loom policy org check <action> [--skill <skill>] [--provider <provider-id>] [--sync-remote <remote>] [--agent <agent>] [--json]
loom approval request <action> [--skill <skill>] [--provider <provider-id>] [--sync-remote <remote>] [--reason <text>]
loom approval list [--pending|--approved|--rejected]
loom approval approve <request-id> [--comment <text>]
loom approval reject <request-id> [--comment <text>]
loom roles list
loom roles grant <user-or-team> <role>
loom roles revoke <user-or-team> <role>
```

## Roles

Initial roles:

- `viewer`: read status and inspect.
- `author`: create/edit local draft skills.
- `reviewer`: approve skills for activation/release.
- `maintainer`: release, rollback, quarantine, manage providers.
- `admin`: manage org policy and roles.

Local Git and repository permissions remain authoritative for repository write
access. Loom roles only gate Loom actions.

A fresh org must define the first admin explicitly. `policy org init` either
records a reviewed `--bootstrap-admin <user>` grant or fails with instructions
for a manual reviewed roles-file bootstrap; it must not create an empty policy
that permanently denies all future `roles grant` commands.

## Actions

Governed actions:

- `skill.install`
- `skill.project`
- `skill.activate`
- `skill.deactivate`
- `skill.release`
- `skill.rollback`
- `skill.trust.update`
- `skill.quarantine`
- `provider.add`
- `provider.remove`
- `sync.push`

Subject arguments are action-specific. Skill lifecycle actions require
`--skill`; provider actions require `--provider`; sync actions require the
configured remote or sync target. The evaluator must reject irrelevant or
missing subject arguments instead of inventing dummy skill values.

## Policy Behavior

`policy org check` returns one of:

- `allow`
- `deny`
- `approval_required`

Mutating commands must call the same policy evaluator internally. If approval
is required, commands return `POLICY_BLOCKED` with an approval request command
and any required approval tokens.

## Approval Request Model

Approval requests are Git-tracked append-only events. Current request status is
derived from the event stream; implementations must not rewrite a mutable
request record to append decisions.

```json
{"event": "requested", "request_id": "approval_...", "action": "skill.activate", "subject": {"skill": "fixflow"}, "requester": "alice", "reason_redacted": "...", "risk_summary": {"high": 1}, "evidence": {}, "created_at": "..."}
{"event": "approved", "request_id": "approval_...", "approver": "bob", "comment_redacted": "...", "created_at": "..."}
```

Approve/reject commands append decision events; they must not rewrite prior
records silently. Free-form reason and comment text must be scanned or redacted
before commit so pasted tokens or keys are not stored in Git-tracked approval
files.

Decision commands must authorize the current actor against the request's
required roles before appending approved or rejected events. An approval event
unblocks only the exact action plus action-specific subject and evidence digest
that were requested.

## Non-Goals

1. No hosted RBAC service in v1.
2. No replacement for GitHub/GitLab repository permissions.
3. No bypass of local user safety gates.
4. No secrets in policy, role, or approval files.
5. No implicit approval from green CI alone.
6. No hidden mutation when checking policy.

## Acceptance Criteria

1. Org policy can be initialized and displayed.
2. Policy check returns allow/deny/approval_required for lifecycle actions.
3. Activation/release/trust/quarantine commands enforce policy.
4. Approval requests can be created, approved, and rejected.
5. Approvals are auditable and Git-tracked.
6. RBAC roles are resolved from local policy and exposed in JSON.
7. Tests cover allow, deny, approval-required, approved action, rejected action,
   missing role, initial admin bootstrap, action-specific subject matching,
   blocked skill, and release preflight policy gates.
