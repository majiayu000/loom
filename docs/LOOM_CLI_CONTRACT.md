# Loom registry model CLI Contract

Updated: 2026-07-16
Status: Implemented

Every JSON envelope includes independent SemVer field `cli_contract_version`. The current contract is `1.3.0`; compatibility history and shipped-Skill ranges live in `docs/cli-contract-history.toml` and `skills/loom-registry/loom.skill.toml`.

## 1. Purpose

This document defines the command contract for Loom registry model.

It exists to make three things explicit:

1. what commands exist
2. what selectors must be explicit
3. what JSON shape agents can rely on

This document turns [LOOM_STATE_MODEL.md](LOOM_STATE_MODEL.md) into a concrete machine-facing interface.

## 2. Contract Principles

1. Every state-changing command must support `--json`.
2. Every command must support `--root <abs-path>`.
3. Projection-related writes must never rely on a guessed default Claude directory.
4. Workspace-scoped writes must identify a `binding_id`.
5. Target-scoped writes must identify a `target_id`.
6. Every successful registry write returns an `op_id`; noop writes and
   non-registry lifecycle actions may omit it by explicit command contract.
7. Read commands must have zero control-plane side effects. They do not mutate
   registry state, Git refs, Git index, live targets, or the operation backlog;
   they do append command-audit events.

## 3. Naming Rules

Top-level command groups:

1. `init`
2. `backup`
3. `monitor`
4. `use`
5. `plan`
6. `apply`
7. `workspace`
8. `target`
9. `skill`
10. `skillset`
11. `telemetry`
12. `provider`
13. `catalog`
14. `package`
15. `mcp`
16. `provision`
17. `policy`
18. `approval`
19. `roles`
20. `instruction`
21. `workflow`
22. `sync`
23. `ops`
24. `agent`
25. `codex`
26. `panel`

Removed from runtime surface:

1. `skill import`
2. `migrate legacy-to-registry`

The legacy mental model is rejected:

1. no `Target::Claude|Codex|Both` execution shortcut
2. no hidden path resolution as execution identity
3. no write command keyed only by `agent=claude`

## 4. Global Flags

Required global flags:

```bash
--root <abs-path>
```

Optional global flags:

```bash
--json
--pretty
--request-id <id>
```

Rules:

1. Agents should always use `--json`.
2. `--root` is mandatory for automation and examples in this spec.
3. `--json` defaults to compact single-line output for token efficiency.
4. `--json --pretty` is reserved for human debugging and documentation capture.
5. If argument parsing fails while `--json` is present, Loom returns the same envelope shape with `cmd: "cli.parse"` and `error.code: "ARG_INVALID"`.

## 5. Selector Rules

Supported `agent` values are `claude`, `codex`, `cursor`, `windsurf`, `cline`, `copilot`, `aider`, `opencode`, `gemini-cli`, and `goose`.

### 5.1 `skill_id`

Represents a canonical source skill under `skills/<skill>`.

### 5.2 `binding_id`

Represents a workspace binding.

Required when:

1. projecting a skill into a workspace context
2. capturing live changes from a workspace context
3. reading workspace-scoped projection health

### 5.3 `target_id`

Represents a concrete registered target directory.

Required when:

1. registering or removing a target
2. explicitly overriding target choice during projection

### 5.4 `instance_id`

Represents one materialized projection instance.

Required when:

1. inspecting one projection instance
2. repairing one projection instance
3. capturing from one specific instance when `skill_id + binding_id` is not unique

## 6. JSON Envelope

All `--json` commands return the same top-level shape:

```json
{
  "ok": true,
  "cmd": "skill.project",
  "request_id": "req_01",
  "version": "<loom-version>",
  "data": {},
  "error": null,
  "meta": {
    "op_id": "op_01",
    "warnings": [],
    "sync_state": "LOCAL_ONLY"
  }
}
```

Rules:

1. `ok=true` means the command succeeded.
2. `ok=false` means the command failed and `error` must be populated.
3. `cmd` is the canonical command name, not raw argv text.
4. `request_id` is echoed back if supplied, otherwise generated.
5. `meta.op_id` is required for successful writes and omitted for pure reads.
6. Successful envelopes keep `error: null` so agents can rely on a stable field shape.
7. `meta.sync_state`, when present, describes registry Git transport/backlog only. It is deprecated for new consumers, remains through the next major version, and must equal `data.convergence.registry_transport.state` when a convergence object is present. It does not prove projection convergence or agent visibility.
8. New consumers use `data.convergence.registry_transport`, `data.convergence.projections`, and `data.convergence.visibility`. Missing axes fail closed and must not be interpreted as healthy.

## 7. Error Object

Failure envelope shape:

```json
{
  "ok": false,
  "cmd": "skill.project",
  "request_id": "req_01",
  "version": "<loom-version>",
  "data": {},
  "error": {
    "code": "BINDING_NOT_FOUND",
    "message": "binding 'bind_x' does not exist",
    "details": {},
    "next_actions": [
      {
        "cmd": "loom workspace binding list --json",
        "reason": "list existing bindings to find a valid binding_id"
      }
    ]
  },
  "meta": {
    "warnings": []
  }
}
```

`error.next_actions[]` is optional and omitted when empty. Each entry is a
runnable command plus a short reason. Human output prints the same suggestions
as `hint: try <cmd> - <reason>`.

Default recovery hints:

1. `BINDING_NOT_FOUND` -> `loom workspace binding list --json`
2. `TARGET_NOT_FOUND` -> `loom target list --json`
3. `SKILL_NOT_FOUND` -> `loom skill list --json`
4. `STATE_NOT_INITIALIZED` -> `loom workspace init --json`
5. `TARGET_NOT_MANAGED` -> `loom target list --json`
6. `LOCK_BUSY` -> `loom ops list --json`
7. `REMOTE_UNREACHABLE`, `REMOTE_DIVERGED`, and `PUSH_REJECTED` -> `loom sync status --json`

## 8. Error Codes

`error.code` is the sole stable semantic routing key. Process exit codes are
coarse failure tiers only and must not be used to distinguish individual error
semantics: 2 is argument parsing, 3 is domain/state/policy/internal failure, 4
is lock contention, 5 is Git/I/O failure, and 10 is remote/sync failure. The
table contains 30 codes; 21 currently map to the coarse exit-3 tier.

Action coverage has three forms: `default` is a universal no-argument recovery
command, `contextual` is emitted only when the call site has every required
skill/target/ref value, and `exempt` intentionally has no command because Loom
cannot prescribe a safe recovery action from the error code alone. Every
emitted action is a concrete `loom ... --json` command; placeholders are not
part of the envelope contract.

<!-- error-code-table-start -->
| Error code | Exit | Action coverage | Action or rationale |
| --- | ---: | --- | --- |
| `ARG_INVALID` | 2 | `exempt` | Correct syntax depends on the rejected command and argument. |
| `INIT_ERROR` | 3 | `exempt` | Process initialization failed before a safe recovery command was available. |
| `DEPENDENCY_CONFLICT` | 3 | `contextual` | A dependency-specific inspect or install command requires the affected skill/tool. |
| `SCHEMA_MISMATCH` | 3 | `exempt` | State/schema repair requires diagnosis; automatic mutation is unsafe. |
| `STATE_CORRUPT` | 3 | `exempt` | Corrupt state requires diagnosis or restore rather than a generic command. |
| `STATE_NOT_INITIALIZED` | 3 | `default` | `loom workspace init --json` |
| `PROVIDER_NOT_FOUND` | 3 | `contextual` | Provider recovery requires the missing provider identifier. |
| `SKILL_NOT_FOUND` | 3 | `default` | `loom skill list --json` |
| `BINDING_NOT_FOUND` | 3 | `default` | `loom workspace binding list --json` |
| `TARGET_NOT_FOUND` | 3 | `default` | `loom target list --json` |
| `TRASH_ENTRY_NOT_FOUND` | 3 | `contextual` | Trash inspection requires the affected skill or entry identifier. |
| `TARGET_NOT_MANAGED` | 3 | `default` | `loom target list --json` |
| `TARGET_AGENT_MISMATCH` | 3 | `contextual` | Inspection requires the selected target and agent context. |
| `PROJECTION_CONFLICT` | 3 | `contextual` | The call site may emit a concrete skill inspection command. |
| `PROJECTION_METHOD_UNSUPPORTED` | 3 | `contextual` | Recovery depends on the requested agent, target, and projection method. |
| `POLICY_BLOCKED` | 3 | `contextual` | The call site may emit a concrete skill inspection or policy command. |
| `EVAL_FAILED` | 3 | `contextual` | Rerunning evaluation requires the affected skill and evaluation mode. |
| `CAPTURE_CONFLICT` | 3 | `contextual` | Patch/capture call sites emit a concrete affected-skill inspection command. |
| `COMMIT_DIRECTION_AMBIGUOUS` | 3 | `contextual` | The envelope emits concrete source and projection commit alternatives. |
| `AUDIT_ERROR` | 3 | `exempt` | Audit persistence failure must preserve the original operation context. |
| `LOCK_BUSY` | 4 | `default` | `loom ops list --json` |
| `REMOTE_UNREACHABLE` | 10 | `default` | `loom sync status --json` |
| `REMOTE_DIVERGED` | 10 | `default` | `loom sync status --json` |
| `PUSH_REJECTED` | 10 | `default` | `loom sync status --json` |
| `REPLAY_CONFLICT` | 10 | `contextual` | Replay call sites may emit a concrete operation-inspection command. |
| `QUEUE_BLOCKED` | 10 | `contextual` | Queue recovery depends on the blocked operation and prerequisite. |
| `ADAPTER_INVALID` | 3 | `contextual` | Adapter diagnosis requires the selected adapter and source path. |
| `GIT_ERROR` | 5 | `exempt` | Repository diagnosis is required; a generic mutating Git command is unsafe. |
| `IO_ERROR` | 5 | `exempt` | Filesystem/transport diagnosis depends on the failed path or resource. |
| `INTERNAL_ERROR` | 3 | `exempt` | No safe user action can be inferred from an internal fault. |
<!-- error-code-table-end -->

Semantics:

1. selector-related failures must be explicit
2. ownership and projection conflicts must not collapse into generic IO errors
3. migration ambiguity must return structured details, not only free-form strings
4. policy denials must return `POLICY_BLOCKED` with the full policy report in `error.details.report`

## 9. Workspace Commands

### 9.1 `workspace status`

```bash
loom --json --root <root> workspace status
```

Read-only.

Response shape:

```json
{
  "bindings": [],
  "targets": [],
  "projections": [],
  "git": {
    "branch": "main",
    "head": "abc123"
  },
  "remote": {
    "configured": false,
    "operation_backlog": 0,
    "operation_counts": {
      "actionable_operations": 0,
      "local_journal_events": 3,
      "unpushed_history_events": 0,
      "local_only_history_events": 400
    },
    "sync_state": "LOCAL_ONLY"
  },
  "convergence": {
    "registry_transport": { "state": "LOCAL_ONLY", "evidence": {}, "stale": false, "errors": [] },
    "projections": { "state": "not_applicable", "items": [], "evidence": {}, "stale": false, "errors": [] },
    "visibility": { "state": "unsupported", "agent": null, "evidence": {}, "stale": false, "errors": [] },
    "complete": true,
    "incomplete_axes": []
  },
  "operation_backlog": 0,
  "operation_counts": {
    "actionable_operations": 0,
    "local_journal_events": 3,
    "unpushed_history_events": 0,
    "local_only_history_events": 400
  },
  "agent_dir_defaults": {
    "agent_dirs": [
      { "agent": "claude", "env_var": "CLAUDE_SKILLS_DIR", "path": "/home/me/.claude/skills" },
      { "agent": "codex", "env_var": "CODEX_SKILLS_DIR", "path": "/home/me/.codex/skills" }
    ]
  },
  "agent_adapters": {
    "adapter_api": "2",
    "adapters": [
      { "id": "claude", "source": "built-in", "fidelity": "verified" },
      { "id": "cursor", "source": "built-in", "fidelity": "generic" }
    ]
  }
}
```

Every `agent_adapters.adapters` row always includes `fidelity`, whose closed
values are `verified` and `generic`. External adapter input cannot self-assert
the field and is reported as `generic`. A `verified` row cannot contain a
`discovery_roots[].role` of `legacy-default`. Generic metadata is diagnostic
only and must not be treated as verified visibility evidence by doctor or
skill-diagnose consumers.

Gemini CLI verified visibility reads the documented settings precedence,
including `skills.enabled` and the case-insensitive union semantics of
`skills.disabled`; projected `SKILL.md` files must have loadable `name` and
`description` frontmatter. Loom applies Gemini's `[:\\/<>*?"|]` to `-`
frontmatter-name sanitization before comparing the directory name. Project
roots additionally require affirmative workspace trust unless a valid user
projection independently satisfies discovery. Missing trust, explicit denial,
malformed settings/trust state, or
unobservable remote `admin.skills.enabled` policy cannot yield `visible=true`.
The reload check names `/skills reload` and does not require a new session.

`registry_transport` describes the registry remote and operation backlog only.
`projections` comes from live existence/method/digest or symlink evidence, while
`visibility` requires adapter evidence. Cross-axis combinations such as
`registry_transport.state=SYNCED` with `projections.state=drifted` are valid and
must not be collapsed into one success state. Every axis includes an observation
timestamp or revision/digest evidence; a revision/checkpoint race sets `stale=true`
and lists the affected axis in `incomplete_axes`.

`complete` describes evidence collection only: all requested axes were observed
without an unknown/error/stale result. It is never a convergence-health verdict.
Consumers must inspect every axis state; for example, `complete=true` with
`projections.state=missing` still requires projection repair.

Requirements:

1. must explain resolved bindings
2. must explain projection health, including `observation_status` for each projection
3. must not write state
4. `drifted_projections` counts persisted drift, missing, unreadable, conflict, and orphaned states; legacy copy/materialize records with no digest observation render as `not_observed` but are not counted as drifted

### 9.2 `workspace doctor`

```bash
loom --json --root <root> workspace doctor
```

Read-only unless a future explicit repair subcommand is introduced.

### 9.3 `workspace binding add`

```bash
loom --json --root <root> workspace binding add \
  --agent <agent> \
  --profile <profile-id> \
  --matcher-kind <path-prefix|exact-path|name> \
  --matcher-value <value> \
  --target <target-id>
```

Write command.

Success response:

```json
{
  "binding": {
    "binding_id": "bind_claude_project_a",
    "agent": "claude",
    "profile_id": "default",
    "default_target_id": "target_claude_default"
  }
}
```

Meta requirements:

1. include `op_id`

### 9.4 `workspace binding list`

```bash
loom --json --root <root> workspace binding list
```

Read-only.

### 9.5 `workspace binding remove`

```bash
loom --json --root <root> workspace binding remove <binding-id> [--orphan-projections]
```

Write command.

Rules:

1. without `--orphan-projections`, must fail with `DEPENDENCY_CONFLICT` if non-orphan projections still depend on the binding
2. with `--orphan-projections`, removes the binding and rules, marks dependent projections `orphaned`, and leaves live projection paths in place

## 10. Target Commands

### 10.1 `target add`

```bash
loom --json --root <root> target add \
  --agent <agent> \
  --path <dir> \
  [--ownership <managed|observed|external>]
```

Write command.

Rules:

1. registration does not project anything
2. `ownership` defaults to `observed`; pass `managed` only for directories Loom may write

### 10.2 `target list`

```bash
loom --json --root <root> target list
```

Read-only.

### 10.3 `target show`

```bash
loom --json --root <root> target show <target-id>
```

Read-only.

### 10.4 `target remove`

```bash
loom --json --root <root> target remove <target-id>
```

Write command.

Rules:

1. removing a target does not delete the underlying directory
2. must fail if active projections or bindings still depend on it unless force semantics are explicitly defined

## 11. Skill Commands

### 11.0 `skill list`, `skill inspect`, `skill search`

```bash
loom --json --root <root> skill list
loom --json --root <root> skill inspect <skill-id> [--agent <agent>] [--workspace <path>] [--profile <profile>] [--include-telemetry]
loom --json --root <root> skill inspect <skill-id> --brief
loom --json --root <root> skill used <skill-id> [--agent <agent>] [--workspace <path>] [--session-id <id>] [--tokens-in <n>] [--tokens-out <n>] [--commands <n>] [--duration-ms <n>] [--success | --error] [--failure-category <category>]
loom --json --root <root> skill feedback <skill-id> --feedback <accepted|rejected|ignored> [--agent <agent>] [--workspace <path>] [--session-id <id>] [--task <text>]
loom --json --root <root> skill deps <skill-id> [--agent <agent>] [--workspace <path>]
loom --json --root <root> skill compile <skill-id> --dry-run [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile --skill <skill-id> --dry-run [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile list <skill-id>
loom --json --root <root> skill compile verify <skill-id> [--artifact <artifact-id>]
loom --json --root <root> skill visibility <skill-id> --agent codex [--workspace <path>] [--profile <profile>]
loom --json --root <root> skill search <query> [--agent <agent>] [--profile <profile>] [--status <status>] [--trust <trust>] [--workspace <path>] [--active] [--for-task] [--semantic] [--explain]
```

`skill list`, `skill inspect`, `skill deps`, `skill compile --dry-run`,
`skill compile list`, `skill compile verify`, `skill visibility`, and
`skill search` are read-only commands. `skill used` and `skill feedback`
mutate only local telemetry state when telemetry is enabled.

Rules:

1. `skill list`, `skill inspect --brief`, and `skill search` reuse the same union read model as `GET /api/v1/skills`.
2. `skill inspect` returns the canonical single-skill status model with stable top-level keys: `skill`, `source`, `spec`, `provenance`, `runtime`, `dependencies`, `quality`, `safety`, `telemetry`, `compiled`, `convergence`, and `next_actions`.
3. `skill inspect --brief` returns the compact inventory shape previously used by the dedicated single-skill inventory view.
4. `skill inspect` separates registry source presence, entrypoint presence, Git drift fields, portable lint, agent compatibility lint, binding rules, projection instances, materialized path health, and unknown agent-specific visibility.
5. `skill inspect --agent <agent>` filters runtime sections for that agent while preserving top-level source, spec, provenance, quality, safety, and next action fields.
6. `skill inspect --workspace <path>` and `--profile <profile>` are selectors for binding/runtime classification only; they must not mutate registry state or source files.
7. `visible_to_agent`, `enabled_by_agent_config`, and `restart_required` are `unknown` when Loom only has registry/projection evidence. Projection presence must not be reported as agent visibility.
8. `skill inspect` returns `SKILL_NOT_FOUND` when neither the canonical source nor registry references exist for the skill. Stale registry references with missing source return a status model with explicit error findings.
9. `skill search` is deterministic lexical scoring over skill id, description, tags, warning state, and source status; `--semantic` falls back to lexical scoring with an explicit warning when no local provider exists.
10. `skill search --for-task` returns deterministic task-resolution fields: `strategy`, `selected`, and `candidates`; it must not invoke an LLM.
11. `skill search --explain` returns recommendation details under `recommendations`, including skillset candidates, scoring inputs, safety risks, warnings, recommended actions, and suggested commands.
12. `--workspace` on `skill search --for-task` may boost skills whose binding matcher covers the supplied workspace path.
13. `skill visibility --agent <agent>` is a read-only active-view proof for registered adapters. Its report includes the adapter `fidelity` when an adapter is registered; generic adapters return a structured unsupported check instead of verified visibility claims. The Codex report covers source, active rule, target, symlink projection, Codex `skills.config` disables, runtime entries, external entries, and restart recommendations without claiming current-session hot reload.
14. read commands must not mutate registry state, Git refs, Git index, live targets, or the operation backlog.
15. trust metadata comes from `state/registry/trust.json`; absent metadata is `unknown`.
16. `skill deps` is read-only and reports runtime dependency readiness for tools, MCP servers, environment variables, and network expectations without printing secret values.
15. `skill compile --dry-run`, `skill compile list`, and `skill compile verify` are read-only; they never replace portable `SKILL.md` as the source of truth.
16. `skill inspect --include-telemetry` reads the same local telemetry summary used by `telemetry report`; without the flag, `telemetry` is `null`.
17. `skill used` records `skill.invocation` by default and records `skill.error` only with `--error --failure-category <category>`.
18. `skill used --success` and `skill used --error` are mutually exclusive; `--error` without `--failure-category` fails before any telemetry write.
19. `--failure-category` accepts only the controlled categories `timeout`, `tool_error`, `model_error`, `dependency_error`, `permission_denied`, `rate_limited`, `invalid_input`, `policy_blocked`, `not_found`, `network_error`, `execution_error`, and `unknown`; arbitrary raw error text or token-shaped values must be rejected.
20. `skill feedback` records explicit `recommendation.feedback` values of `accepted`, `rejected`, or `ignored`; `--task` must not be serialized as raw telemetry text and is persisted only as a redacted task hash.
21. when telemetry is absent or disabled, `skill used` and `skill feedback` return `recorded=false` with a structured reason and do not initialize `state/telemetry`.
22. `skill recommend` and `skill search --for-task --explain` may include telemetry-derived `score_inputs` only when matching local telemetry events exist within the telemetry retention window; absent, disabled, or stale telemetry must leave deterministic ranking unchanged.

### 11.0.1 `skill compile`

```bash
loom --json --root <root> skill compile <skill-id> [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile <skill-id> --dry-run [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile --skill <skill-id> --dry-run [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile list <skill-id>
loom --json --root <root> skill compile verify <skill-id> [--artifact <artifact-id>]
```

Planning, artifact write, and read-only verification commands.

Rules:

1. `skill compile --dry-run` returns planned artifact paths, source digest inputs, token estimates, content hashes, and gate status without writing artifact files, state files, target files, or lockfiles.
2. `skill compile <skill-id>` without `--dry-run` writes the deterministic artifact directory under `state/compiled/skills/<skill-id>/<artifact-id>/`, verifies the written artifact from disk, and commits the artifact state.
3. when `--agent` is omitted the deterministic sentinel is `portable`; when `--profile` is omitted the profile is `default`.
4. artifact ids are path segments, not paths; `--artifact` rejects absolute paths, traversal, and unsafe characters before joining under `state/compiled/skills/<skill-id>/`.
5. derived artifacts, when present, use `state/compiled/skills/<skill-id>/<artifact-id>/manifest.json`, `activation.md`, `catalog.json`, `boundaries.json`, `tool-interface.json`, `references.index.json`, and `source-digest.txt`.
6. `source-digest.txt` must match `manifest.source_digest`, and `verify` recomputes the source digest from `SKILL.md`, indexed references/assets/scripts, compiler version, agent, and profile.
7. `verify` detects missing files, malformed manifests or sidecars, stale source digests, content-hash mismatches, manifest identity mismatches, unsafe sidecar paths, and gates that prevent `valid` status.
8. lint, safety, dependency, or eval gates that are missing, blocked, or failed prevent a `valid` artifact; missing eval evidence is blocking until reviewed eval artifacts exist.
9. artifact writes run the local offline eval gate when eval fixtures exist; passing evidence is recorded in `manifest.eval_evidence` with the current generated content hashes and eval suite digest before an artifact may be promoted to `valid`.
10. `verify` rejects `valid` artifacts whose eval evidence is missing, stale, agent-mismatched, or no longer matches generated content hashes.
11. `list` and `verify` without `--artifact` return artifacts sorted by artifact id; no arbitrary filesystem entry is selected as a default.
12. skill names that collide with nested commands such as `list` or `verify` use `--skill <skill-id>` for planning or artifact writes.
13. compiled activation uses only artifacts that `verify` reports as fresh `valid`; artifact writes remain separate from dry-run planning and portable `SKILL.md` remains the source of truth.

### 11.0.2 `skill activate`, `skill deactivate`, `skill active list`

```bash
loom --json --root <root> skill activate <skill-id> --agent <agent> [--scope <user|project>] [--workspace <path>] [--profile <profile>] [--target <target-id>] [--method <symlink|copy|materialize>] [--compiled [--artifact <artifact-id>]] [--dry-run]
loom --json --root <root> skill deactivate <skill-id> --agent <agent> [--scope <user|project>] [--workspace <path>] [--profile <profile>] [--target <target-id>] [--dry-run]
loom --json --root <root> skill active list --agent <agent> [--scope <user|project>] [--workspace <path>] [--profile <profile>]
```

`activate` and `deactivate` are write commands unless `--dry-run` is supplied. `active list` is read-only.

Rules:

1. `skill activate` resolves a managed target and workspace binding from agent, scope, workspace, profile, and optional target id; callers must not need to pass binding ids for the common path.
2. user-scoped Codex activation defaults to `$HOME/.agents/skills`; project-scoped Codex activation defaults to `<workspace>/.agents/skills`; project scope requires `--workspace`.
3. `--dry-run` must return the same plan shape without creating registry files, Git commits, target directories, projections, operation backlog rows, or command audit events.
4. activation enforces the same target ownership, projection capability, filesystem symlink probe, and skill policy gates as projection.
5. repeated activation is idempotent; a missing managed symlink projection is repaired without duplicating targets, bindings, rules, or projections.
6. `skill deactivate` removes the desired rule and projection record, and deletes only a symlink that points back to the registry skill source.
7. deactivation of `copy` or `materialize` projections fails closed with `POLICY_BLOCKED` and must not delete live target files.
8. `skill active list` reports desired rules joined to realized projections, including `target_missing` and `projection_missing`, but must keep agent visibility fields at `not_checked`.
9. `--artifact` is valid only with `--compiled`.
10. `skill activate --compiled` verifies the selected compiled artifact before any projection write, rejects missing, stale, blocked, invalid, or agent/profile-mismatched artifacts with `POLICY_BLOCKED`, and returns `next_actions`.
11. when a fresh valid artifact is selected, compiled activation materializes an agent-compatible `SKILL.md` plus `.loom/compiled/` metadata under the target skill directory and records the projection as `materialize`.
12. normal activation without `--compiled` continues to use portable source projection and must not require compiled artifacts.

### 11.0.3 `skill visibility`, `skill diagnose --agent codex`, `codex reconcile`

```bash
loom --json --root <root> skill diagnose <skill-id> --agent codex
loom --json --root <root> skill visibility <skill-id> --agent codex [--workspace <path>] [--profile <profile>]
loom --json --root <root> codex reconcile --dry-run [--binding <binding-id>] [--target <target-id>] [--allowlist <path>]
loom --json --root <root> codex reconcile --apply [--binding <binding-id>] [--target <target-id>]
loom --json --root <root> codex reconcile --apply --fix-config [--binding <binding-id>] [--target <target-id>]
```

`skill diagnose --agent codex` and `skill visibility` are read-only. `codex reconcile` defaults to dry-run unless `--apply` is supplied.

Rules:

1. visibility checks must separate projection existence from Codex visibility; a symlink alone is not enough.
2. dry-run must report planned `create_projection`, `repair_projection`, `remove_stale_projection`, `remove_stale_record`, `preserve_runtime_entry`, `preserve_external_entry`, `fix_config_disable`, and `manual_review` actions without mutation.
3. `--apply` may repair missing or drifted safe Loom-owned symlink projections and remove stale Loom-owned symlink projections plus stale records.
4. `--apply` without `--fix-config` must not edit Codex config.
5. `--apply --fix-config` may flip only safe active-skill `[[skills.config]] enabled = false` entries to `true`, validates TOML before replace, writes atomically, and returns `restart_required: true`.
6. malformed Codex config returns `SCHEMA_MISMATCH` for config repair and is never silently ignored.
7. runtime entries such as `.system` and `codex-primary-runtime`, plus non-Loom external entries, are preserved.
8. multiple active bindings sharing a Codex target are reconciled as a union of desired active skills.

### 11.0.3.1 `skill diagnose`

```bash
loom --json --root <root> skill diagnose <skill-id>
```

Default skill diagnosis observes registered projections without persisting the observation. For `copy` and `materialize` projections, it compares source and live projection content digests and reports a `projection_content_digest:<instance_id>` check plus the same `convergence` object used by inspect and visibility.

Rules:

1. healthy copy/materialize observations return matching source/materialized digests in `data.convergence.projections.items[]`
2. digest mismatches return `projections.state: "drifted"` and a structured `digest_mismatch` item error without changing `state/registry/projections.json`
3. missing source, missing live path, and unreadable source/live path are distinct machine-readable observation errors
4. symlink projections remain path-checked; content digest fields are for copy/materialize projections

### 11.0.4 `skill author new`

```bash
loom --json --root <root> skill author new <skill-id> [--template <basic|coding-workflow|scripted|reference-heavy>] [--description <text>] [--agent <agent>] [--dry-run]
```

Write command unless `--dry-run` is supplied.

Rules:

1. creates `skills/<skill-id>/SKILL.md` plus `references/`, `scripts/`, `assets/`, `evals/`, and `loom.skill.toml`
2. generated `SKILL.md` must pass current strict portable lint
3. `loom.skill.toml` is Loom-local management metadata and is ignored by portable agent-facing lint
4. `--dry-run` returns paths and file previews without writing files, registry state, Git refs, operation backlog, or command audit state
5. existing skill directories fail with `ARG_INVALID` and must not be overwritten
6. invalid portable skill names fail with `ARG_INVALID` before source skill files are created
7. generated skills are committed as registry source changes when not dry-run

### 11.0.5 `skill author draft`, `skill author extract`, `skill author rewrite`, `skill author tune-description`, `skill author generate-evals`, and `skill author apply-patch`

```bash
loom --json --root <root> skill author draft <skill-id> --from-session <path|id> [--agent <agent>] [--provider mock] [--dry-run]
loom --json --root <root> skill author extract <skill-id> --from-diff <path> [--provider mock] [--dry-run]
loom --json --root <root> skill author rewrite <skill-id> --instruction <text> [--provider mock] [--dry-run]
loom --json --root <root> skill author tune-description <skill-id> [--description <text>] [--provider mock] [--dry-run]
loom --json --root <root> skill author generate-evals <skill-id> [--task <text>] [--provider mock] [--dry-run]
loom --json --root <root> skill author apply-patch <patch-id> --idempotency-key <key>
```

Authoring generation commands create reviewable patch artifacts under
`state/patches/` by default and never mutate `skills/<skill-id>` source files.
`--dry-run` previews the same artifact shape without writing patch files. The
only enabled provider is deterministic `mock`; hosted/network providers are not
available in this slice. `skill author apply-patch` validates the patch id and
idempotency key, revalidates the reviewed source digest/ref, applies the patch
to an isolated staging copy, runs strict lint, safety, and mock eval gates, then
materializes and commits the source change only after those gates pass.

Rules:

1. prompt material must come from explicit session, diff, skill source, or eval inputs
2. prompt material is size-bounded and redacts secret-looking strings, URL credentials, token-like values, and sensitive env values before provider use
3. patch artifacts include `schema_version`, `patch_id`, `skill`, `action`, `goal`, `source_ref`, `source_digest`, provider, files, prompt material, validation plan, risk notes, JSON path, and patch path
4. generation commands write only `state/patches/skillpatch_*.json` and `.patch` plus normal command audit; they do not stage, commit, activate, release, or edit source files
5. `apply-patch` must never expose the raw idempotency key; success, replay, and failure details include only `idempotency_key_digest`
6. missing or malformed patch ids and missing `--idempotency-key` return typed `ARG_INVALID`
7. source digest/ref drift returns `CAPTURE_CONFLICT` without mutating source files
8. high-risk generated scripts, network access, destructive commands, or failed lint/eval gates block apply before commit
9. rerunning `apply-patch` with the same idempotency key and patch artifact returns the recorded result without applying the patch again

### 11.1 `skill add`

```bash
loom --json --root <root> skill add <path|git-url|github:owner/repo//subdir> --name <skill-id> [--ref <branch|tag|commit>] [--subdir <path>]
```

Write command.

Rules:

1. adds canonical source under `skills/<skill-id>`
2. must fail when target skill already exists
3. local directory imports use provider `local_path`
4. Git URL and local Git repository imports use provider `git`; `--ref` may be a branch, tag, or commit
5. `github:owner/repo//subdir` imports use provider `github` and clone `https://github.com/owner/repo.git`; this command must not require or duplicate `gh` authentication scope
6. `--subdir` selects a source subdirectory when it is not encoded in the GitHub locator
7. successful imports write `state/registry/sources.json` and deterministic root `loom.lock`
8. provenance records include provider, locator, requested ref, resolved commit when Git-backed, source tree hash when Git-backed, source subdir, artifact digest, import time, and importer version
9. provider resolution boundaries are defined in [SKILL_PROVIDER_BOUNDARY.md](SKILL_PROVIDER_BOUNDARY.md); `skill add` must not call `gh skill install` or write directly into agent host directories

### 11.1.1 `provider`, `catalog`, and `skill install`

```bash
loom --json --root <root> provider add <id> --kind <github|local> --url <url>
loom --json --root <root> provider list
loom --json --root <root> provider remove <id>
loom --json --root <root> catalog search <query> [--provider <provider-id>] [--allow-network]
loom --json --root <root> catalog show <locator>
loom --json --root <root> catalog preview <locator> [--ref <ref>]
loom --json --root <root> skill install <locator> --name <skill-id> [--ref <ref>] [--trust <third-party-unreviewed|reviewed>] [--review-evidence <id>] [--policy-profile <profile>] [--dry-run]
```

Provider writes persist sorted `state/registry/providers.json` records through the normal registry audit, commit, and sync/queue path. `provider list`, `catalog search`, `catalog show`, and `catalog preview` are read-only and do not seed provider state.

Rules:

1. provider ids are locator prefixes; built-in `github` and `local` providers are synthesized for read-only locator parsing
2. `team:` is reserved and unsupported in this version
3. unknown provider prefixes return `PROVIDER_NOT_FOUND`
4. provider URLs with userinfo or token-like query parameters fail with `ARG_INVALID` before persistence
5. catalog preview inspects files without executing scripts or build hooks
6. `skill install --dry-run` writes no skill directory, provenance file, `loom.lock`, trust state, target directory, Git ref, or operation backlog row beyond normal command audit
7. unpinned refs fail closed with `POLICY_BLOCKED`; local locators are pinned only by a matching `sha256:<digest>` ref and GitHub locators by a commit SHA
8. public installs default to `third-party-unreviewed`; `--trust reviewed` requires `--review-evidence`
9. pinned provider-backed install apply copies without symlinks, writes `skills/<skill-id>`, `state/registry/sources.json`, deterministic `loom.lock`, `state/registry/trust.json`, and a `skill.install` registry operation, but never auto-activates the skill
10. critical safety findings block install before any registry or skill mutation

### 11.1.2 `package plan`, `package build`, and `package verify`

```bash
loom --json --root <root> package plan <skill:<skill>|skillset:<skillset>> --format agent-skills-archive [--agent <agent>] [--output-plan <path>]
loom --json --root <root> package build <plan-artifact> --output <path> --idempotency-key <key>
loom --json --root <root> package verify <artifact> [--format agent-skills-archive]
```

Package planning is read-only. Package build writes only the requested outbound artifact and records command audit, but it does not mutate registry source, target directories, active projections, provider state, or operation backlog. Package verify is read-only.

Rules:

1. `package plan` resolves `skill:<id>`, `skillset:<id>`, or a bare id only when it is unambiguous
2. the first implemented format is `agent-skills-archive`; `codex-plugin`, `claude-plugin`, `npm`, and `github-release` return typed unsupported results until adapter metadata is wired
3. plans include source kind, source id, source ref, source digest, Loom version, gate status, and a redacted file manifest
4. plan/build/verify reject private registry state, local absolute paths, user-specific config, symlinks, hardlinks, and secret-looking material
5. build requires an idempotency key, loads a reviewed plan artifact, revalidates source digest and package gates, stages output, writes manifest/provenance/checksums, and rejects output inside packaged source or private registry state
6. verify checks the manifest, package format, checksums, forbidden content, source freshness when source is available, and portable skill lint
7. build output returns install and verify guidance only; package artifacts are not active-state, visibility, trust, or installed-state proof
8. publish/submission to external package hosts is deferred and must not bypass Loom registry authority when later implemented

### 11.1.3 `mcp requirement`, `mcp plan`, `mcp apply`, `mcp doctor`, and `mcp catalog`

```bash
loom --json --root <root> mcp requirement list --skill <skill> [--agent <agent>]
loom --json --root <root> mcp plan --skill <skill> --agent <agent> [--workspace <path>] [--output-plan <path>]
loom --json --root <root> mcp apply <plan-id|plan-artifact> --idempotency-key <key> [--approve <approval-token>...]
loom --json --root <root> mcp doctor --agent <agent> [--skill <skill>] [--workspace <path>]
loom --json --root <root> mcp catalog search <query>
loom --json --root <root> mcp catalog show <server>
```

MCP provisioning is plan-first. Requirement, doctor, and catalog commands are read-only. `mcp plan` writes an audited durable reviewed plan under `state/mcp/plans/<plan_id>.json` and may also write an explicit `--output-plan` artifact, but it does not write agent config, package installs, or secret values. Apply consumes a durable reviewed plan id or explicit plan artifact, requires an idempotency key and approval tokens for risky actions, revalidates source/config preimages, writes Codex config atomically, and never stores secret values.

Rules:

1. `mcp requirement list` merges `loom.skill.toml`, supported `SKILL.md` metadata, agent metadata, and compatibility suggestions without printing secret values
2. `mcp plan` returns missing/existing server status, resolved source policy, launcher tool availability, env names, redacted config diffs, risk summary, and approval requirements
3. pinned npm locators split scoped package names at the rightmost `@`; unpinned package, Git, local, or unknown sources are blocked or approval-required until immutable provenance is recorded
4. unsupported agents return `manual_configuration_required` actions instead of guessed config paths
5. `mcp doctor` and `skill diagnose` point to `mcp plan` when MCP dependency readiness fails
6. `mcp apply` fails closed when approvals, env vars, launcher tools, pinned sources, skill source digest, or Codex config preimage validation fail
7. `mcp apply` writes only reviewed Codex `mcp_servers` config, forwards environment variable names through `env_vars`, preserves unrelated server settings, and returns restart guidance when it changes config

### 11.1.4 `provision plan`, `provision doctor`, `provision apply`, `provision export`, and `provision import`

```bash
loom --json --root <root> provision plan --target devcontainer [--workspace <path>] [--agent codex] [--output-plan <path>]
loom --json --root <root> provision doctor --target devcontainer|codespaces|remote [--workspace <path>] [--agent <agent>] [--plan <plan-id|plan-artifact>]
loom --json --root <root> provision apply <plan-id|plan-artifact> --idempotency-key <key> [--approve <approval-token>...]
loom --json --root <root> provision export <plan-id|plan-artifact> --format devcontainer|shell|tar --output <path>
loom --json --root <root> provision import <artifact> --dry-run
```

Remote provisioning is plan-first. The implemented slices generate a read-only devcontainer plan and doctor report, reviewed shell/tar export artifacts, import dry-runs, durable reviewed plan-id replay, and gated apply for reviewed target files. They must not copy secrets, mutate registry state outside the apply idempotency record, or deploy remote environments. `--output-plan` and `provision export --format shell|tar --output <path>` write only explicitly requested local artifacts.

Rules:

1. `provision plan --target devcontainer` returns target kind, workspace/container paths, active views, dependency readiness, generated file previews, secret names, policy gates, Loom CLI prerequisite, and guard digests
2. Codex project active views use `<workspace>/.agents/skills`; the plan must not fall back to user-level `~/.codex/skills`
3. `git+https://...` registry remotes normalize to cloneable `https://...`; HTTP(S) userinfo is removed from clone/display URLs and represented as a redacted secret requirement
4. generated devcontainer setup previews use `set -euo pipefail`, require `loom`, do not print secret values, and check planned active skills without writing them
5. `provision doctor` is read-only and reports missing/different generated files, adapter paths, dependency readiness, secrets, policy, and next actions
6. `provision export --format shell` loads a reviewed plan id or artifact path, writes a deterministic shell artifact with digest metadata, and must not include secret values
7. `provision export --format tar` writes a deterministic portable artifact containing the reviewed plan, generated file previews, registry skill source files, materialized active-view files, manifest metadata, and checksums without secret values
8. `provision import <artifact> --dry-run` validates shell/tar artifact metadata/digests and reports review-only planned files without executing scripts, extracting archives, or writing target files
9. `provision apply <plan-id|plan-artifact>` requires an idempotency key and reviewed approval tokens when policy requires them; it revalidates guard digests, reviewed registry head reachability, credential-redacted registry clone URL, target preimages, target paths, and generated content digests before atomic writes, and repeated apply with the same key is idempotent
10. `provision plan` persists a durable reviewed plan under `state/provision/plans/<plan_id>.json`; `apply`, `export`, and `doctor --plan` load that durable plan id or an explicit reviewed artifact path without regenerating reviewed content from current registry state
11. non-dry-run `provision import` and `provision export --format devcontainer` remain deferred until their artifact validation and write gates are implemented

### 11.1.5 `policy org`, `approval`, and `roles`

```bash
loom --json --root <root> policy org init --bootstrap-admin <user>
loom --json --root <root> policy org show
loom --json --root <root> policy org check <action> [--skill <skill>] [--provider <provider-id>] [--sync-remote <remote>] [--agent <agent>]
loom --json --root <root> approval request <action> [--skill <skill>] [--provider <provider-id>] [--sync-remote <remote>] [--agent <agent>] [--reason <text>]
loom --json --root <root> approval list [--pending|--approved|--rejected]
loom --json --root <root> approval approve <request-id> [--comment <text>]
loom --json --root <root> approval reject <request-id> [--comment <text>]
loom --json --root <root> roles list
loom --json --root <root> roles grant <user-or-team> <viewer|author|reviewer|maintainer|admin>
loom --json --root <root> roles revoke <user-or-team> <viewer|author|reviewer|maintainer|admin>
```

First-slice org governance creates Git-tracked policy, role, and approval state. It does not yet enforce org policy inside every mutating command; callers can use `policy org check` and approval events as the audited decision layer until command-wide enforcement lands.

Rules:

1. fresh `policy org init` requires explicit `--bootstrap-admin`; existing policy init is idempotent and must not reset admins
2. policy state lives in `state/registry/org_policy.toml`; role grants live in deterministic `state/registry/roles.json`; approvals append to `state/registry/approvals.jsonl`
3. `policy org check` returns `allow`, `deny`, or `approval_required` with required roles, approval tokens, evidence, and request commands
4. `workspace.remote` is normalized to canonical policy action `workspace.remote.set`
5. blocked or quarantined skill trust state returns `deny` and cannot be bypassed by approval events
6. approval request reasons and decision comments are redacted before persistence
7. approve/reject commands require the current local actor to satisfy one of the request's required roles
8. role grant/revoke require current admin role and revoke must preserve at least one resolved non-team admin
9. malformed policy, role, or approval state fails closed with `STATE_CORRUPT`

## Continued contract

Sections 11.1.5 through 19 remain normative and continue in
[LOOM_CLI_CONTRACT_OPERATIONS.md](LOOM_CLI_CONTRACT_OPERATIONS.md).
