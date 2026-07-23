# Loom CLI Contract: extended command and workflow surfaces

This normative file continues sections 11.1.5 through 19 of
[LOOM_CLI_CONTRACT.md](LOOM_CLI_CONTRACT.md).

### 11.1.5 `instruction scan`, `instruction show`, `instruction classify`, `instruction doctor`, `instruction migrate-plan`

```bash
loom --json --root <root> instruction scan [--agent <agent>] [--workspace <path>]
loom --json --root <root> instruction show <instruction-id> [--workspace <path>]
loom --json --root <root> instruction classify <path>
loom --json --root <root> instruction doctor [--agent <agent>] [--workspace <path>] [--skill <skill>]
loom --json --root <root> instruction migrate-plan <instruction-id> [--workspace <path>] --to <skill|reference|keep-instruction> [--name <skill>] --dry-run
```

Read-only command group.

Rules:

1. scans known native instruction surfaces such as `AGENTS.md`, `CLAUDE.md`, Cursor rules, Windsurf files, and Copilot instructions without registering them as skills
2. `show` and `migrate-plan` resolve ids against `--workspace` when supplied, matching ids produced by `scan --workspace`
3. Copilot scans include active `AGENTS.md` surfaces; `.github/instructions/*.instructions.md` surfaces are path-specific, not always-on, and include parsed `applyTo` patterns when present
4. returns paths, adapter metadata, scope, precedence notes, signals, and warnings, but not raw instruction content
5. unsupported adapters or unknown instruction surfaces are reported explicitly when requested by the agent filter or classification path
6. `doctor --skill <skill>` compares instruction signals with one registry skill and reports duplicate guidance, conflicts, shadowing risks, prompt-budget risks, and missing adapter metadata
7. `migrate-plan` requires `--dry-run`; apply is deferred and non-dry-run migration returns `POLICY_BLOCKED`
8. migration plans contain reviewable `would_write` entries only and must not edit instruction files, skill files, registry state, Git refs, live targets, or operation backlog
9. portable skill lint remains strict: `AGENTS.md`, `CLAUDE.md`, `.mdc`, and custom instruction files are not accepted as `SKILL.md`

### 11.1.6 `skill provenance`

```bash
loom --json --root <root> skill provenance inspect <skill-id>
loom --json --root <root> skill provenance verify <skill-id>
loom --json --root <root> skill provenance outdated [<skill-id>] [--plan]
loom --json --root <root> skill provenance refresh <skill-id>
```

Mixed read/write command group.

Rules:

1. `inspect` is read-only and returns the recorded `sources.json` entry plus the matching `loom.lock` entry
2. `verify` is read-only and compares the current canonical skill digest against both recorded provenance and `loom.lock`
3. `outdated` is read-only and reports provider-backed records whose pinned refs differ from provider heads or current local provider digests
4. `outdated` rows include `skill_id`, `provider`, `current_ref`, `current_digest`, `candidate_ref`, `candidate_digest`, `candidate_trust`, `status`, `risk`, and `next_actions`
5. `outdated` status values are `up_to_date`, `outdated`, `unreachable`, `unpinned_candidate`, and `invalid_source`; provider failures must be reported as `unreachable`, not silently treated as clean
6. `outdated --plan` emits a JSON re-pin plan with `mutates=false` and `apply_required=true`; it must not edit skill content, `sources.json`, `loom.lock`, projection state, target directories, binding rules, Git refs, or live agent skill directories
7. unpinned provider heads are advisory only until resolved to immutable commit SHAs or `sha256:<digest>` refs
8. `refresh` is a write command; it recomputes the current canonical skill digest, updates `state/registry/sources.json` and `loom.lock`, and commits only provenance artifacts
9. `refresh` must not mutate projection state, target directories, binding rules, or live agent skill directories
10. `loom.lock` is generated from sorted source records so repeated writes are deterministic
11. missing skill sources return `SKILL_NOT_FOUND`; missing provenance records return `STATE_NOT_INITIALIZED`

### 11.2 `skill import-observed`

```bash
loom --json --root <root> skill import-observed [--target <target-id>]
```

Write command.

Rules:

1. imports real skill directories from observed targets into canonical `skills/<skill-id>`
2. top-level symlinks to skill directories are materialized into canonical `skills/<skill-id>` as real files
3. only directories containing `SKILL.md` or `skill.md` are treated as skills
4. existing canonical skills are skipped, not overwritten
5. `--target` must reference an observed target when supplied
6. this is not the removed legacy `skill import` command; it is an explicit bridge from discovered observed targets into the source registry

### 11.2.1 `skill monitor-observed`

```bash
loom --json --root <root> skill monitor-observed [--target <target-id>] [--once] [--interval-seconds <seconds>]
```

Write command.

Rules:

1. scans observed targets for directories containing `SKILL.md` or `skill.md`
2. imports new observed skills into canonical `skills/<skill-id>`
3. updates existing canonical skills when materialized file content differs from the observed source
4. top-level symlinks to skill directories are materialized as real files
5. duplicate skill names found in later observed targets are skipped for that cycle
6. observed deletions are not propagated automatically
7. `--once` runs one scan and exits; without it, the command polls every `--interval-seconds`
8. `--target` must reference an observed target when supplied

### 11.3 `skill lint`

```bash
loom --json --root <root> skill lint <skill-id> [--strict | --portable | --compat | --fix] [--agent <agent>] [--quality]
```

Read-only command.

Rules:

1. default `--strict` mode requires uppercase `SKILL.md`, valid YAML frontmatter, portable `name`, matching directory/name identity, and a useful `description`
2. `--portable` is an alias for strict portable Agent Skills compliance
3. `--compat` accepts legacy `skill.md` loading but returns typed warning findings
4. `--fix` returns a read-only plan for safe normalizations such as `skill.md` to `SKILL.md`; it must not mutate files
5. `--agent codex` and `--agent claude` add target-agent compatibility sections and warnings, including configured active skill directory name collisions
6. `--quality` adds non-fatal maintainability findings for trigger quality, size, eval fixtures, script layout, deeply nested references, and runtime dependency declarations/readiness
7. strict portable lint rejects descriptions above 1024 characters
7. strict lint failures return `SCHEMA_MISMATCH` with the full report in `error.details.report`
8. the report includes `entrypoint`, `frontmatter`, `sections`, `findings`, `summary`, and `fix_plan`

### 11.3.1 `skill policy`

```bash
loom --json --root <root> skill policy <skill-id> [--policy-profile <profile>]
```

Read-only command.

Rules:

1. reports declared frontmatter capabilities under `capabilities.filesystem`, `capabilities.shell`, `capabilities.network`, and `capabilities.secrets`
2. scans source files for scripts, executable files, binary-looking content, large files, generated artifact directories, suspicious shell patterns, and prompt-injection heuristics
3. includes provenance digest status when `state/registry/sources.json` and `loom.lock` contain the skill
4. default profile is `safe-capture`; built-in profiles are `safe-capture`, `audit-only`, `deny-risky`, and `strict`
5. `audit-only` and `safe-capture` report findings without blocking projection
6. `deny-risky` and `strict` mark high-risk findings as blockers
7. unknown profile names are valid organizational hooks but must produce a `policy_profile_unknown` warning until an implementation handles them
8. policy checks are heuristic signals, not a sandbox, malware verdict, or guarantee that a skill is safe

### 11.3.2 `skill scan`, `skill trust`, `skill quarantine`, `skill unquarantine`

```bash
loom --json --root <root> skill scan <skill-id> [--mode install|activate|release] [--strict]
loom --json --root <root> skill trust <skill-id> --level <local-draft|reviewed|team-approved|third-party-unreviewed|blocked|quarantined>
loom --json --root <root> skill quarantine <skill-id> [--reason <text>]
loom --json --root <root> skill unquarantine <skill-id>
```

`scan` is read-only. `trust`, `quarantine`, and `unquarantine` are write commands that update registry-owned trust metadata and command audit state.

Rules:

1. trust metadata is stored in `state/registry/trust.json`, sorted by `skill_id`, and never written to portable `SKILL.md`
2. absent trust metadata means `trust=unknown` and `quarantined=false`
3. malformed `trust.json` fails closed with typed state errors and must not be overwritten by read commands
4. `skill scan` returns the safety-only view: `decision`, `trust`, severity `summary`, structured `findings`, and `activation_allowed`
5. findings include stable ids, severity, path, optional line, message, and suggested action
6. `blocked` and `quarantined` skills fail projection and activation before target mutation
7. `quarantine` preserves source files and reports existing active projections as requiring manual cleanup review; it does not delete target files
8. `unquarantine` clears quarantine without elevating trust to `reviewed` or `team-approved`
9. safety scans are heuristic review signals, not a sandbox, malware verdict, or guarantee that a skill is safe

### 11.3.3 `skill deps`

```bash
loom --json --root <root> skill deps <skill-id> [--agent <agent>] [--workspace <path>]
```

Read-only command.

Rules:

1. dependency declarations are read from `loom.skill.toml`, `SKILL.md` metadata/compatibility text, scripts, and agent metadata with deterministic precedence
2. tool checks use PATH lookup and optional argv-based `--version` probes with a timeout; they must not use shell interpolation
3. env checks report only presence and `redacted=true`; values, lengths, prefixes, and hashes must not be printed
4. MCP checks inspect local config when supported; unsupported agents return `configured="unknown"` / `enabled="unknown"` instead of a false pass
5. missing required tools, env vars, or MCP config set `ready=false` with actionable `next_actions`
6. network expectations are inferred from declarations/scripts without making network calls
7. the same readiness helper feeds `skill inspect`, `skill diagnose`, and `skill lint --quality`

### 11.3.4 `skill eval`

```bash
loom --json --root <root> skill eval <skill-id> [--agent <agent> | --matrix <agent,agent>] [--model <model>]
loom --json --root <root> skill eval offline <skill-id> [--agent <agent> | --matrix <agent,agent>] [--model <model>]
loom --json --root <root> skill eval run <skill-id> --agent <agent> --baseline no-skill [--workspace <path>] [--cases <path>] [--runs <n>] [--runner mock|codex-cli] [--dry-run] [--output <path>]
loom --json --root <root> skill eval trigger <skill-id> --agent <agent> [--cases <path>] [--runs <n>] [--runner mock|codex-cli] [--output <path>]
loom --json --root <root> skill eval compare <skill-id> --from <ref> --to <ref|working-tree> --agent <agent> [--cases <path>] [--runner mock|codex-cli] [--output <path>]
loom --json --root <root> skill improve <skill-id> [--agent <agent>] [--workspace <path>] [--baseline <ref>] [--real-eval] [--dry-run]
loom --json --root <root> skill regression <skill-id> [--agent <agent>] [--from <ref>] [--to <ref|working-tree>]
```

The legacy flat command and `offline` subcommand are read-only. `run`, `trigger`, and `compare`
persist reports under `state/registry/evals/<skill-id>/` by default or to explicit `--output`.

Fixture layout:

```text
skills/<skill-id>/evals/
├── triggers.jsonl
├── tasks.jsonl
└── graders/
```

Rules:

1. `triggers.jsonl` contains positive and negative trigger cases with `prompt`/`input`, expected trigger labels, and optional observed trigger labels
2. `tasks.jsonl` contains offline task fixtures with output, trace, metrics, permissions used, deterministic checks, and optional artifact checks
3. `eval <skill-id>` remains an alias for `eval offline <skill-id>`; `--agent` stamps one agent id into the report and `--matrix` replays fixtures across a comma-separated matrix
4. `run --dry-run` returns a plan with resolved cases and writes no reports, starts no runner, and mutates no workspace
5. the default `mock` runner deterministically compares with-skill and no-skill baselines in isolated temp workspaces; `codex-cli` is explicit opt-in and returns typed `EVAL_FAILED` when unavailable or not authorized
6. reports include per-case status, with/without pass rates, delta, trigger precision/recall when trigger cases exist, available token/command/duration overhead, cleanup status, skill version metadata, and report path
7. with-skill failures, trigger failures, missing runners, report persistence errors, and cleanup failures return typed errors with the full report in `error.details.report`; no-skill baseline failures are comparison evidence, not command failure by themselves
8. default reports do not persist raw prompts or secrets; explicit output paths are caller-controlled
9. eval success is quality evidence only and must not be treated as a safety guarantee

### 11.3.5 `skill improve`, `skill regression`

```bash
loom --json --root <root> skill improve <skill-id> [--agent <agent>] [--workspace <path>] [--baseline <ref>] [--real-eval] [--dry-run]
loom --json --root <root> skill regression <skill-id> [--agent <agent>] [--from <ref>] [--to <ref|working-tree>]
```

Both commands are read-only and must not stage files, create commits, create tags, mutate registry state, or check out refs destructively.

Rules:

1. `skill improve` returns one `SkillPreflightReport` with stable top-level keys: `schema_version`, `skill`, `mode`, `baseline`, `target`, `checks`, `regressions`, `recommendation`, `mutation_allowed`, and `details`.
2. Checks include source drift, portable or agent-specific lint, safety scan, dependency readiness, `SKILL.md` size, offline eval fixtures, real-eval status, and security diff when comparing two refs.
3. Check statuses are `pass`, `warning`, `fail`, `skipped`, or `unknown`. Missing optional evidence must be `warning`, `skipped`, or `unknown`, never a fabricated pass.
4. `--real-eval` does not run a real agent in this version; it marks `real_eval` as `unknown` and points callers to explicit eval compare workflows.
5. `skill regression` compares `--from` to `--to` or the working tree without destructive checkout and fails with `POLICY_BLOCKED` when lint, high/critical safety, dependency readiness, offline eval, or size gates fail.
6. Blocking regression failures include the full report under `error.details.report`.
7. `source_drift` is advisory for commit/release decisions; failed or unknown gates other than source drift block mutation.
8. The size gate fails when `SKILL.md` exceeds 800 lines without a `references/` directory and warns when references exist.
9. `skill regression --to <ref>` materializes the selected skill and security metadata into a temporary root before running checks, rather than checking out refs or reading the current working tree.

### 11.3.6 `skillset create`, `skillset add`, `skillset remove`, `skillset show`, `skillset lint`, `skillset activate`, `skillset deactivate`, `skillset eval`, `skillset release`, `skillset rollback`

```bash
loom --json --root <root> skillset create <skillset-id> [--description <text>]
loom --json --root <root> skillset add <skillset-id> <skill-id> [--role <role>] [--required|--optional]
loom --json --root <root> skillset remove <skillset-id> <skill-id>
loom --json --root <root> skillset show <skillset-id>
loom --json --root <root> skillset lint <skillset-id>
loom --json --root <root> skillset activate <skillset-id> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--dry-run]
loom --json --root <root> skillset deactivate <skillset-id> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--dry-run]
loom --json --root <root> skillset eval <skillset-id> --agent <agent> [--baseline no-skill|single-skills]
loom --json --root <root> skillset release <skillset-id> <version>
loom --json --root <root> skillset rollback <skillset-id> --to <version|ref>
```

`create`, `add`, `remove`, non-dry-run `activate`, non-dry-run `deactivate`, `release`, and `rollback` are write commands. `show`, `lint`, `eval`, and dry-run activation/deactivation are read-only/preview commands.

Rules:

1. skillsets are persisted in `state/registry/skillsets.json`
2. absent `skillsets.json` means no skillsets exist; it is not a registry corruption error
3. `skillset add` accepts only skills present in the current skill inventory read model
4. a skill can appear at most once in a skillset
5. member `role` is optional advisory metadata and does not imply execution order
6. members are required by default; `--optional` marks a member optional
7. `skillset show` includes each member's current skill read-model summary when available and marks drifted missing members
8. `skillset lint` validates member existence, duplicate members, empty skillsets, and required/optional counts
9. `skillset activate --dry-run` returns a per-member activation plan without target writes
10. `skillset activate` and `skillset deactivate` reuse the single-skill activation/deactivation path for each member
11. required member activation failures fail closed with typed errors; partial activation failures include rollback results and recovery commands
12. `skillset eval` aggregates member offline eval reports and reports detected `skillsets/<id>/evals/` fixtures as deferred end-to-end work
13. `skillset release` tags the current skillset definition as `release/skillset/<id>/<version>`
14. `skillset rollback --to <version|ref>` restores only that skillset definition from the resolved ref and does not check out member skill source files

### 11.3.7 `workflow create`, `workflow show`, `workflow plan`, and `workflow preflight`

```bash
loom --json --root <root> workflow create <workflow-id> --file <workflow.json> [--dry-run]
loom --json --root <root> workflow create <workflow-id> --from-skillset <skillset-id> --dry-run
loom --json --root <root> workflow show <workflow-id>
loom --json --root <root> workflow plan <workflow-id> --agent <agent> --workspace <path>
loom --json --root <root> workflow preflight <plan-id>
```

`workflow create` writes `state/registry/workflows.json` unless `--dry-run` is supplied. `workflow show` and `workflow preflight` are read-only. `workflow plan` writes an auditable guarded plan under `state/registry/workflow_plans.json` without executing nodes.

Rules:

1. workflow definitions are explicit DAGs with `workflow_id`, `nodes`, `edges`, `external_inputs`, and `policy`
2. cycles, self-edges, missing edge endpoints, oversized plans, excessive depth, and missing required upstream outputs fail with `ARG_INVALID`
3. `workflow plan` requires each node skill source to exist and fails with `SKILL_NOT_FOUND` for missing skills
4. blocked or quarantined skill trust fails with `POLICY_BLOCKED`; workflow planning must not silently skip unsafe nodes
5. plans record root, registry head, workflow digest, skill source digests, ordered node ids, activation steps, required approvals, risks, and `safe_to_run=false`
6. `workflow preflight` rechecks stored plan guards against the current registry root, Git head, workflow digest, and skill digests
7. `workflow run` is hidden from the public command surface until workflow apply gates are implemented; if invoked for compatibility, `--dry-run` returns `status=deferred`, and non-dry-run returns `ARG_INVALID` with `status=deferred`, `hidden=true`, and `safe_to_run=false`
8. `--from-skillset` is preview-only until workflow apply semantics are implemented

### 11.4 `skill project`

```bash
loom --json --root <root> skill project <skill-id> --binding <binding-id> [--target <target-id>] [--method <symlink|copy|materialize>]
```

Write command.

Success response:

```json
{
  "projection": {
    "instance_id": "inst_loom_bind_claude_project_a",
    "skill_id": "loom",
    "binding_id": "bind_claude_project_a",
    "target_id": "target_claude_default",
    "method": "symlink",
    "materialized_path": "/Users/foo/.../skills/loom",
    "health": "healthy",
    "observed_drift": false,
    "source_tree_digest": null,
    "materialized_tree_digest": null,
    "last_observed_at": "2026-07-03T05:00:00Z",
    "last_observed_error": null
  }
}
```

Rules:

1. `binding_id` is mandatory
2. if `--target` is absent, Loom may use `default_target_id` from binding metadata
3. if multiple targets are possible and no default exists, the command must fail explicitly
4. before mutating target directories, the command evaluates unified safety using trust metadata and the binding's `policy_profile`
5. if trust state or the selected profile blocks projection, the command fails with `POLICY_BLOCKED` and must not create or replace the live skill directory
6. successful copy/materialize projection records initial source/live content digests and observation timestamp; successful symlink projection records path observation without content digests

### 11.5 `skill commit`

```bash
loom --json --root <root> skill commit <skill-id> [--message <msg>] [--from-projection | --from-source] [--binding <binding-id>] [--instance <instance-id>] [--preflight]
```

Write command.

Success response:

```json
{
  "skill": "loom",
  "direction": "source",
  "commit": "abc123",
  "noop": false
}
```

Rules:

1. exactly one dirty side is selected automatically
2. dirty source plus dirty projection fails with `COMMIT_DIRECTION_AMBIGUOUS`
3. use `--from-source` or `--from-projection` to resolve ambiguity
4. neither dirty side returns `noop: true`

### 11.6 `skill release`

```bash
loom --json --root <root> skill release <skill-id> [<version> | --anchor] [--preflight --baseline <ref>]
```

Acts on canonical source only.

Rules:

1. without `--preflight`, release preserves the existing behavior
2. with `--preflight`, `--baseline <ref>` is required and must not be `HEAD` or `working-tree`
3. the selected skill must be clean before a preflighted release
4. failed gates return `POLICY_BLOCKED` with the full report in `error.details.report`; invalid baseline or dirty source returns `ARG_INVALID`
5. a passing preflight proceeds through the existing tag, registry operation, rollback, and autosync path

### 11.9 `skill rollback`

```bash
loom --json --root <root> skill rollback <skill-id> --to <ref>
```

Acts on canonical source only.

Success response should include:

1. `recovery_ref`
2. resulting source revision
3. `source_restored: true`
4. `registry_restored: true`
5. `live_projection_reconciled`
6. `projection_reconciliation`

Rules:

1. rollback restores the canonical source and records registry audit state; it
   does not silently claim that live agent projections were updated
2. copy and materialize projections default to `recovery_plan_only`; rollback
   reports them as `requires_projection_reapply=true` until the user runs the
   returned recovery command
3. existing symlink projections are reported as `symlink_noop` only when the
   projection path is a symlink that resolves to the restored source; missing,
   dangling, wrong-target, or non-symlink paths are reported with a reapply
   command
4. `projection_reconciliation.items[]` includes `instance_id`, `skill_id`,
   `binding_id`, `target_id`, `materialized_path`, `method`, `status`,
   `live_path_exists`, `requires_projection_reapply`, and `next_action`
5. `projection_reconciliation.next_actions[]` contains exact executable
   `loom --json --root <root> skill project <skill-id> --binding <binding-id>
   --target <target-id> --method <method>` commands when Loom can reapply a
   projection safely, or `manual_review_required` when registry evidence is
   missing or the projection was produced by compiled activation
6. compiled activation projections are reported with `compiled_activation`
   evidence and a `manual_review_required` action; Loom must not emit a raw
   `skill project --method materialize` recovery command for them because that
   would replace the compiled artifact view with source materialization
7. if registry snapshot loading fails after rollback, the response keeps
   `ok=true` for the source rollback but sets
   `projection_reconciliation.status="registry_unavailable"`, includes a
   structured `error`, sets `live_projection_reconciled=false`, and adds a
   warning to `meta.warnings`
8. no-op rollback success returns `source_restored=false` and
   `registry_restored=false`; when registry state already exists, rollback still
   evaluates registered live projections before setting
   `live_projection_reconciled`, and when registry state is absent it returns
   `projection_reconciliation.status="noop"` without initializing registry state
9. if registry state was absent before a non-noop rollback, rollback records
   audit state but reports `projection_reconciliation.status="registry_missing"`
   and `live_projection_reconciled=false` because there was no pre-existing
   projection evidence to verify

### 11.10 `skill diff`

```bash
loom --json --root <root> skill diff <skill-id> <from> <to>
loom --json --root <root> skill diff --security <skill-id> <from> <to>
```

Read-only.

Rules:

1. default diff returns the raw Git patch for `skills/<skill-id>`
2. `--security` returns structured security-relevant changed paths and findings only
3. security diff highlights changed scripts, security-relevant metadata, references, and new network, secret, destructive, shell-execution, or prompt-injection patterns
4. missing refs or Git failures return typed Git errors

### 11.11 `telemetry`

```bash
loom --json --root <root> telemetry status
loom --json --root <root> telemetry enable [--local-only]
loom --json --root <root> telemetry disable
loom --json --root <root> telemetry ingest --agent claude|codex|all [--since <date>] [--dry-run]
loom --json --root <root> telemetry report [--skill <skill-id>] [--skillset <skillset-id>] [--agent <agent>] [--workspace <path>] [--since <date>]
loom --json --root <root> telemetry export --format jsonl|csv --output <path> [--redacted]
loom --json --root <root> telemetry purge [--before <date>] --dry-run
loom --json --root <root> telemetry purge [--before <date>] --confirm <token>
loom --json --root <root> skill used <skill-id> [--agent <agent>] [--workspace <path>] [--session-id <id>] [--tokens-in <n>] [--tokens-out <n>] [--commands <n>] [--duration-ms <n>] [--success | --error] [--failure-category <category>]
loom --json --root <root> skill feedback <skill-id> --feedback <accepted|rejected|ignored> [--agent <agent>] [--workspace <path>] [--session-id <id>] [--task <text>]
```

`status`, `report`, and `ingest --dry-run` are read-only. Durable `ingest`,
`enable`, `disable`, and confirmed `purge` mutate only `state/telemetry`.
`export` writes only the explicit output path and must reject output under
registry state.

Rules:

1. telemetry is local-first and opt-in; absent config reports `enabled=false`.
2. enabled config uses `state/telemetry/config.json`; events use append-only
   `state/telemetry/events.jsonl`.
3. telemetry readers accept event schema versions 1 through 3. New writers use
   version 3, typed event families, hashed workspace/session/task identifiers,
   and `privacy.raw_prompt_stored=false`, `privacy.raw_code_stored=false`,
   `privacy.redacted=true`. Version 3 adds validated `observed_skill_name` for
   unmatched invocations; it never stores raw transcript or source paths.
4. disabled telemetry must not append telemetry events.
5. existing malformed event lines are surfaced as quarantined warnings in
   status/report/export/purge responses; they are not silently dropped.
6. `telemetry report` summarizes usage, value, cost, drift, risk, sync, and
   recommendation feedback. Missing upstream evidence must be reported as
   `missing`, not zero usage; deferred hosted/sync evidence is
   `not_instrumented`.
7. `telemetry export --format jsonl|csv` emits redacted typed events only and
   skips malformed lines with warnings.
8. `telemetry purge --dry-run` returns matching event count, byte impact, and a
   confirmation token; `--confirm` must match the current dry-run token before
   atomically rewriting telemetry event state.
9. `skill eval`, `skill scan`, `skill activate`, `skill deactivate`,
   `skill used`, and `skill feedback` append redacted telemetry events only
   when telemetry is enabled.
10. `telemetry report` returns an `instrumentation` map for declared event
   families so consumers can distinguish `available`, `missing`, and
   `not_instrumented` states from numeric zero counts.
11. `skill used --error` persists only structured failure categories and
   numeric metrics, never raw errors, prompts, outputs, env values, or file
   contents.
12. recommendation telemetry evidence uses only events inside the configured
   retention window, matches feedback to the requested task when present, and
   exposes both recent usage counts and recent error rate in `score_inputs`.
13. Panel Telemetry consumes the same backend read model at
    `/api/v1/telemetry/report` and preserves missing evidence as missing.
14. `telemetry ingest` accepts only tracked structured Claude/Codex invocation
    anchors. Free-text mentions are not invocations; unknown identity-less
    shapes and invalid observed names are counted under stable rejected reasons
    without echoing raw values.
15. Durable ingest requires enabled telemetry. It scans source logs outside the
    workspace lock, then compare-and-commits deterministic event IDs and
    `state/telemetry/ingest_cursor.json` under the lock. Events are flushed
    before the cursor is atomically replaced; a concurrent cursor change causes
    a bounded rescan.
16. Cursor source keys and event identities are domain-separated hashes. The
    cursor stores only generation, committed newline offset, boundary hash, and
    earliest covered `--since`; it never stores raw paths. Partial trailing
    records do not advance the committed offset or count as malformed.
17. Cursor continuity is deliberately bounded so normal append scans do not
    reread a potentially multi-gigabyte committed prefix. Loom guarantees reset
    for truncation, source replacement or generation change, same-size rewrite,
    and edits intersecting the bounded hash immediately before the committed
    offset. An arbitrary earlier-prefix rewrite combined with file growth is not
    guaranteed detectable; producers that rewrite logs non-append-only must
    delete `state/telemetry/ingest_cursor.json` to force a full rescan.
18. `telemetry report --skill <name>` filters and groups both registered
    `skill_id` and validated unmatched `observed_skill_name` through one
    normalized redacted query model. Agentless events match only an unfiltered
    agent query.

## 12. Human-Friendly Use Flow

### 12.1 `use`

```bash
loom --json --root <root> use <skill-id> --agents <agent[,agent]> [--scope <user|project>] [--workspace <path>] [--profile <id>] [--method <symlink|copy|materialize>] [--target-root <path>] [--adopt] [--apply]
```

Plan-first command. Without `--apply`, it is read-only. With `--apply`, it compiles the plan into explicit `target add`, `workspace binding add`, and `skill project` operations.

Rules:

1. validates that `<skill-id>` is an existing registry skill before planning or applying
2. `--agents` must include at least one supported agent
3. default scope is `project`; project scope uses a `path_prefix` workspace matcher and user scope uses a `name=user` matcher
4. default workspace is the current directory for project scope; user scope does not require `--workspace`
5. target resolution uses adapter discovery roots for the selected scope when available; fallback roots remain under `<root>/targets/<scope>/<agent>/skills`
6. `--target-root` means the exact target skills directory and does not append `<agent>/skills`
7. applying into an existing directory that is not already a managed Loom target requires `--adopt`; without it the command fails with `TARGET_NOT_MANAGED`
8. plan mode returns target/binding/projection steps and an explicit next command containing `--apply`
9. apply mode returns every target, binding, projection, and operation id created or reused by the lower-level commands
10. apply mode returns rollback commands for removing the generated binding and then cleaning orphaned projections
11. this command does not replace lower-level `target`, `workspace binding`, or `skill project` commands

### 12.2 Durable Agent Plan/Apply

```bash
loom --json --root <root> plan use <skill-id> --agents <agent[,agent]> [--scope <user|project>] [--workspace <path>] [--profile <id>] [--method <symlink|copy|materialize>] [--target-root <path>]
loom --json --root <root> plan converge <skill-id> [--from-source | --from-projection --instance <id>] [--agent <agent>] [--workspace <path>] [--profile <id>] [--require-runtime] [--accept-restart-required] [--push-remote]
loom --json --root <root> apply <plan-id> --idempotency-key <key> [--approve <token[,token]>]
loom --json --root <root> apply <convergence-plan-id> --plan-digest <digest> --idempotency-key <key>
```

`plan use` creates a durable, audited plan for the same target/binding/projection setup that `loom use --apply` performs. Plan creation must not mutate registry state, Git refs, operation backlog, or live target directories; its only durable write is the command-audit event under `state/events/commands.jsonl`.

`plan converge` creates a typed immutable plan for one Skill change. It resolves only existing active bindings and rules selected by agent, workspace, and profile; records source, registry checkpoint, projection, visibility, required-axis, acceptance, and remote-policy evidence; and returns a canonical `plan_digest`. Schema 1.3 also records the exact selected input digest, method-aware dirty evidence for every selected projection, strict preflight results for the actual source or projection input bytes, fail-closed input conflicts, and both the caller's raw workspace argument and its plan-time normalized binding. The digest excludes the random `plan_id`, so identical evidence and selectors produce the same digest. Planning writes only command audit and reports the actual `execution_enabled` and `safe_to_apply` gate results. It emits no apply next action.

Convergence plans require the exact non-empty `--plan-digest` returned by planning. Missing or mismatched confirmation fails before any domain write. An enabled plan with `safe_to_apply=true` executes one recoverable local transaction, then checks adapter visibility and performs requested remote transport last. Reuse the same plan id, digest, and idempotency key for interrupted or remote-pending retries. `restart_required` remains visible evidence even when explicitly accepted; `SYNCED` never proves runtime visibility. Existing `plan use` records remain compatible and do not require `--plan-digest`.

The top-level `plan` command owns durable plan creation. The top-level `apply` command owns guarded plan execution.

The plan JSON schema is versioned separately from the binary package version at `docs/schemas/agent-plan-v1.schema.json`. Current plans use `protocol_version: "1.0"`; `plan use` uses `schema_version: "1.0"`, while the additive typed `plan converge` shape uses `schema_version: "1.3"`. Stored convergence plans with schema `1.1` or `1.2` are rejected with `PLAN_SCHEMA_UNSUPPORTED` and must be recreated and reviewed.

Rules:

1. `plan use` validates the skill exists, records the current registry `HEAD`, records the current skill source digest, freezes resolved workspace and target-root paths, lists effects/conflicts/risks, and returns a `plan_id`
2. `apply` loads the plan from command-audit events and validates current `--root`, registry `HEAD`, skill source digest, required approvals, and idempotency key before mutation
3. `apply` is safe to retry with the same `--idempotency-key`; successful responses include `idempotency_key_digest`, not the raw key, and a successful prior apply for the same plan/key digest returns the recorded result with `idempotent_replay=true`
4. reusing an idempotency key for a different plan returns `DEPENDENCY_CONFLICT` with `conflict.code=IDEMPOTENCY_KEY_REUSED`
5. missing approval tokens return `POLICY_BLOCKED` with `conflict.code=APPROVAL_REQUIRED`, `retryable=true`, `event_cursor`, and suggested `--approve` actions
6. stale plans return `DEPENDENCY_CONFLICT` with a typed conflict such as `PLAN_STALE`, `PLAN_SOURCE_DRIFT`, or `PLAN_ROOT_MISMATCH`
7. successful apply returns the lower-level use result plus `recovery.rollback_supported=true` and explicit rollback commands when available; no public rollback token is emitted until a token consumer exists

## 13. Sync Commands

### 13.1 `sync status`

```bash
loom --json --root <root> sync status
```

Read-only.

The primary field is `data.registry_transport`; `data.remote` remains an exact
compatibility mirror for the current major version. `sync status` does not
claim projection convergence or agent visibility.

### 13.2 `sync push`

```bash
loom --json --root <root> sync push
```

Write command.

Acts on source and operation history, not on live target directories.

### 13.3 `sync pull`

```bash
loom --json --root <root> sync pull
```

Write command.

### 13.4 `sync replay`

```bash
loom --json --root <root> sync replay
```

Write command.

## 14. Ops Commands

### 14.1 `ops list`

```bash
loom --json --root <root> ops list
```

Read-only.

`data.ops` contains actionable registry operation rows only. The canonical
`data.operation_counts` object separates:

1. `actionable_operations`
2. `local_journal_events`
3. `unpushed_history_events`
4. `local_only_history_events`

The buckets are mutually exclusive. In a healthy local-only registry, three
succeeded/unacknowledged journal rows and 400 unique history events report
`0 / 3 / 0 / 400`, not 403 pending operations. Compatibility aliases are:

- `count = actionable_operations`
- `journal_events = actionable_operations + local_journal_events`
- `history_events = unpushed_history_events + local_only_history_events`

When origin is configured, history comparison uses unique event IDs from the
local branch and cached `origin/loom-history`. This command does not fetch, so
the tracking ref can be stale. Parse or Git read failures return a structured
error instead of silently reporting zero.

### 14.2 `ops retry`

```bash
loom --json --root <root> ops retry
```

Write command.

### 14.3 `ops purge`

```bash
loom --json --root <root> ops purge
```

Write command.

### 14.4 `ops history diagnose`

```bash
loom --json --root <root> ops history diagnose
```

Read-only.

### 14.5 `ops history repair`

```bash
loom --json --root <root> ops history repair --strategy <local|remote>
```

Write command.

## 15. Migration Policy

Migration commands are intentionally removed from the runtime CLI surface.

Rules:

1. no in-tool `legacy-to-registry` migration entrypoint
2. operators must register targets explicitly with `target add`
3. binding resolution must be explicit with `workspace binding add`

## 16. Response Requirements by Command Type

### 16.1 Pure Reads

Examples:

1. `workspace status`
2. `workspace doctor`
3. `target list`
4. `skill diff`
5. `sync status`
6. `ops list`

Requirements:

1. no registry `op_id`
2. no registry, Git, live-target, or operation-backlog write side effects
3. command-event audit write is expected

### 16.2 Writes

Examples:

1. `workspace binding add`
2. `target add`
3. `skill author new`
4. `skill import-observed`
5. `skill monitor-observed`
6. `skill project`
7. `skill commit`
8. `skill release`
9. `sync push`

Requirements:

1. `meta.op_id` is mandatory
2. selector identities must be echoed in `data`

## 17. Minimal Agent Workflow

Recommended agent-safe sequence:

```bash
loom --json --root "$ROOT" workspace binding list
loom --json --root "$ROOT" target list
loom --json --root "$ROOT" skill project model-onboarding --binding bind_claude_project_a
loom --json --root "$ROOT" skill commit model-onboarding --from-projection --binding bind_claude_project_a
loom --json --root "$ROOT" skill release model-onboarding --anchor
```

Why this is safe:

1. binding is explicit
2. projection is explicit
3. commit direction is explicit when automatic drift detection is ambiguous
4. revision history stays on source

## 18. Rejected CLI Shapes

These command shapes are explicitly rejected for registry state:

1. `loom skill link <skill> --target claude`
2. `loom init --from-agent both --target both`
3. any command that treats `claude` as an execution identity without binding resolution
4. any command that mutates live directories based only on a guessed default path

## 19. Acceptance Criteria

The CLI contract is acceptable only if:

1. every write can be called non-interactively
2. every projection write is binding-scoped
3. every response needed by agents is available in `--json`
4. no core workflow depends on path guessing
5. projection and commit errors are structured and typed
