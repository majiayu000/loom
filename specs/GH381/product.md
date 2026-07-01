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
- `author`: create/edit local draft skills with commands such as `skill.new`,
  `skill.save`, and `skill.capture`.
- `reviewer`: approve skills for activation/release.
- `maintainer`: release, rollback, quarantine, manage providers, and update trust
  state.
- `admin`: manage org policy and roles.

Local Git and repository permissions remain authoritative for repository write
access. Loom roles only gate Loom actions.

A fresh org must define the first admin explicitly. `policy org init` either
records a reviewed `--bootstrap-admin <user>` grant or fails with instructions
for a manual reviewed roles-file bootstrap; it must not create an empty policy
that permanently denies all future `roles grant` commands.
Once policy or role state exists, `policy org init` is create-only/idempotent:
it must not reset admins or bootstrap a new admin unless an existing admin policy
decision authorizes an explicit reset/migration flow.

## Actions

Governed actions:

- `skill.install`
- `skill.add`
- `skill.import_observed`
- `skill.monitor_observed`
- `skill.new`
- `skill.save`
- `skill.capture`
- `skill.watch`
- `skill.snapshot`
- `skill.provenance.refresh`
- `skill.trash.add`
- `skill.trash.restore`
- `skill.trash.purge`
- `skill.orphan.clean`
- `skill.project`
- `skill.activate`
- `skill.deactivate`
- `skill.release`
- `skill.rollback`
- `skill.trust.update`
- `skill.quarantine`
- `provider.add`
- `provider.remove`
- `workspace.remote.set`
- `workspace.binding.add`
- `workspace.binding.remove`
- `target.add`
- `target.remove`
- `sync.pull`
- `sync.push`
- `sync.replay`
- `ops.retry`
- `ops.purge`
- `ops.history.repair`

Subject arguments are action-specific. Skill lifecycle actions require
`--skill` when the command targets one skill; provider actions require
`--provider`; sync actions require the configured remote or sync target; target
actions require `target_id`; workspace binding actions require the binding
identity and target; and `skill.trash.purge` requires `trash_id`. Commands whose
current CLI does not carry a single skill, such as `skill watch` autosave mode,
must derive a deterministic per-skill batch plan and evaluate each planned
skill mutation before writing. `skill.trash.purge` must either resolve
`trash_id` to the original skill subject from immutable trash metadata or govern
the action with a `trash_id` subject; it must not invent a dummy skill value.
The evaluator must reject irrelevant or missing subject arguments instead of
guessing replacements.

Policy action ids are canonical dotted ids. Existing dispatcher ids that are
coarser than a policy action must be mapped before evaluation; for example the
current `workspace.remote` command id aliases to `workspace.remote.set` for
`workspace remote set` until the dispatcher exposes the split id directly.

Autosync and queued sync writes inherit the skill mutation evidence but must also
preflight the planned remote-specific `sync.push` policy before scheduling or
executing. If sync policy requires a different role or approval, autosync queues
or reports the blocked sync action until a matching sync approval exists. Pull,
replay, `ops retry`, remote target changes, and remote `skill add` imports must
not bypass policy by entering through sync, ops, or legacy import paths.

## Policy Behavior

`policy org check` returns one of:

- `allow`
- `deny`
- `approval_required`

Mutating commands must call the same policy evaluator internally. If approval
is required, commands return `POLICY_BLOCKED` with an approval request command
and any required approval tokens.

Default gates are:

- author actions require `author` or stronger. The default role lattice is
  `viewer < author < reviewer < maintainer < admin`; stronger roles inherit
  weaker permissions unless policy explicitly disables inheritance.
- activation, deactivation, and projection require reviewer approval when policy
  marks risk, live-state impact, or third-party trust as review-required.
- release, rollback, provider, trust, and quarantine actions require
  `maintainer` or `admin`.
- remote configuration, workspace binding, target, sync pull/push/replay,
  `ops.retry`, `ops.purge`, `ops.history.repair`, snapshot/tag, provenance
  refresh, trash purge, and orphan cleanup actions require `maintainer` or
  `admin` unless policy explicitly delegates them.
- role and policy administration require `admin`.

Maintainer and admin actions must verify the current actor's maintainer/admin
role before writing even when local Git permissions would allow the file change.

## Approval Request Model

Approval requests are Git-tracked append-only events. Current request status is
derived from the event stream; implementations must not rewrite a mutable
request record to append decisions.

```json
{"event": "requested", "request_id": "approval_...", "action": "skill.activate", "subject": {"skill": "fixflow"}, "requester": "alice", "reason_redacted": "...", "risk_summary": {"high": 1}, "evidence": {"skill_source_digest": "sha256:...", "registry_head": "abc123", "command_inputs_digest": "sha256:..."}, "required_roles": ["reviewer"], "required_approvals": ["approval:reviewer"], "policy_decision_digest": "sha256:...", "created_at": "..."}
{"event": "approved", "request_id": "approval_...", "approver": "bob", "satisfied_approval": "approval:reviewer", "comment_redacted": "...", "created_at": "..."}
```

Approve/reject commands append decision events; they must not rewrite prior
records silently. Free-form reason and comment text must be scanned or redacted
before commit so pasted tokens or keys are not stored in Git-tracked approval
files.

Decision commands must authorize the current actor against the request's
required roles before appending approved or rejected events. An approval event
unblocks only the exact action plus action-specific subject, immutable command
inputs, source digest, registry head, and evidence digest that were requested.
Every `required_approvals[]` token must be satisfied by a matching decision
event; one approval does not satisfy unrelated required tokens. Approved and
rejected requests are terminal unless a later explicit superseding-request flow
creates a new request id.

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
   blocked skill, policy aliases for existing command ids, and release preflight
   policy gates.
