# Loom registry model CLI Contract

Updated: 2026-06-11
Status: Implemented

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
7. `meta.sync_state`, when present, is the authoritative top-level sync status for agent decisions. Command-specific fields such as `data.remote.sync_state` are detail views for diagnostics.

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

## 8. Error Codes

Base error codes:

1. `ARG_INVALID`
2. `DEPENDENCY_CONFLICT`
3. `SCHEMA_MISMATCH`
4. `STATE_CORRUPT`
5. `STATE_NOT_INITIALIZED`
6. `PROVIDER_NOT_FOUND`
7. `SKILL_NOT_FOUND`
8. `BINDING_NOT_FOUND`
9. `TARGET_NOT_FOUND`
10. `TRASH_ENTRY_NOT_FOUND`
11. `TARGET_NOT_MANAGED`
12. `TARGET_AGENT_MISMATCH`
13. `PROJECTION_CONFLICT`
14. `PROJECTION_METHOD_UNSUPPORTED`
15. `POLICY_BLOCKED`
16. `EVAL_FAILED`
17. `CAPTURE_CONFLICT`
18. `AUDIT_ERROR`
19. `LOCK_BUSY`
20. `REMOTE_UNREACHABLE`
21. `REMOTE_DIVERGED`
22. `PUSH_REJECTED`
23. `REPLAY_CONFLICT`
24. `QUEUE_BLOCKED`
25. `GIT_ERROR`
26. `IO_ERROR`
27. `INTERNAL_ERROR`

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
    "sync_state": "LOCAL_ONLY"
  },
  "agent_dir_defaults": {
    "agent_dirs": [
      { "agent": "claude", "env_var": "CLAUDE_SKILLS_DIR", "path": "/home/me/.claude/skills" },
      { "agent": "codex", "env_var": "CODEX_SKILLS_DIR", "path": "/home/me/.codex/skills" }
    ]
  }
}
```

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
loom --json --root <root> skill deps <skill-id> [--agent <agent>] [--workspace <path>]
loom --json --root <root> skill compile <skill-id> --dry-run [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile --skill <skill-id> --dry-run [--agent <agent>] [--profile <profile>]
loom --json --root <root> skill compile list <skill-id>
loom --json --root <root> skill compile verify <skill-id> [--artifact <artifact-id>]
loom --json --root <root> skill visibility <skill-id> --agent codex [--workspace <path>] [--profile <profile>]
loom --json --root <root> skill search <query> [--agent <agent>] [--profile <profile>] [--status <status>] [--trust <trust>] [--workspace <path>] [--active] [--for-task] [--semantic] [--explain]
```

Read-only commands.

Rules:

1. `skill list`, `skill inspect --brief`, and `skill search` reuse the same union read model as `GET /api/v1/skills`.
2. `skill inspect` returns the canonical single-skill status model with stable top-level keys: `skill`, `source`, `spec`, `provenance`, `runtime`, `dependencies`, `quality`, `safety`, `telemetry`, `compiled`, and `next_actions`.
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
13. `skill visibility --agent codex` is a read-only Codex active-view proof. It reports source, active rule, target, symlink projection, Codex `skills.config` disables, runtime entries, external entries, and restart recommendations without claiming current-session hot reload.
14. read commands must not mutate registry state, Git refs, Git index, live targets, or the operation backlog.
15. trust metadata comes from `state/registry/trust.json`; absent metadata is `unknown`.
16. `skill deps` is read-only and reports runtime dependency readiness for tools, MCP servers, environment variables, and network expectations without printing secret values.
15. `skill compile --dry-run`, `skill compile list`, and `skill compile verify` are read-only; they never replace portable `SKILL.md` as the source of truth.
16. `skill inspect --include-telemetry` reads the same local telemetry summary used by `telemetry report`; without the flag, `telemetry` is `null`.

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

Default skill diagnosis observes registered projections. For `copy` and `materialize` projections, it compares source and live projection content digests, writes the latest observation back to `state/registry/projections.json`, and reports a `projection_content_digest:<instance_id>` check. `skill diagnose --agent codex` remains read-only as specified above.

Rules:

1. healthy copy/materialize observations record matching `source_tree_digest`, `materialized_tree_digest`, `last_observed_at`, and `last_observed_error: null`
2. digest mismatches record `health: "drifted"`, `observed_drift: true`, and `last_observed_error: "digest_mismatch"`
3. missing source, missing live path, and unreadable source/live path are distinct machine-readable observation errors
4. symlink projections remain path-checked; content digest fields are for copy/materialize projections

### 11.0.4 `skill new`

```bash
loom --json --root <root> skill new <skill-id> [--template <basic|coding-workflow|scripted|reference-heavy>] [--description <text>] [--agent <agent>] [--dry-run]
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

### 11.0.5 `skill draft`, `skill extract`, `skill rewrite`, `skill tune-description`, `skill generate-evals`, and `skill apply-patch`

```bash
loom --json --root <root> skill draft <skill-id> --from-session <path|id> [--agent <agent>] [--provider mock] [--dry-run]
loom --json --root <root> skill extract <skill-id> --from-diff <path> [--provider mock] [--dry-run]
loom --json --root <root> skill rewrite <skill-id> --instruction <text> [--provider mock] [--dry-run]
loom --json --root <root> skill tune-description <skill-id> [--description <text>] [--provider mock] [--dry-run]
loom --json --root <root> skill generate-evals <skill-id> [--task <text>] [--provider mock] [--dry-run]
loom --json --root <root> skill apply-patch <patch-id> --idempotency-key <key>
```

Authoring generation commands create reviewable patch artifacts under
`state/patches/` by default and never mutate `skills/<skill-id>` source files.
`--dry-run` previews the same artifact shape without writing patch files. The
only enabled provider is deterministic `mock`; hosted/network providers are not
available in this slice. `skill apply-patch` validates the patch id and
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

### 11.1.3 `mcp requirement`, `mcp plan`, `mcp doctor`, and `mcp catalog`

```bash
loom --json --root <root> mcp requirement list --skill <skill> [--agent <agent>]
loom --json --root <root> mcp plan --skill <skill> --agent <agent> [--workspace <path>]
loom --json --root <root> mcp doctor --agent <agent> [--skill <skill>] [--workspace <path>]
loom --json --root <root> mcp catalog search <query>
loom --json --root <root> mcp catalog show <server>
```

MCP provisioning is plan-first. The first slice is read-only and must not install packages, execute MCP servers, write agent config, write secrets, mutate registry state, or change live target directories.

Rules:

1. `mcp requirement list` merges `loom.skill.toml`, supported `SKILL.md` metadata, agent metadata, and compatibility suggestions without printing secret values
2. `mcp plan` returns missing/existing server status, resolved source policy, launcher tool availability, env names, redacted config diffs, risk summary, and approval requirements
3. pinned npm locators split scoped package names at the rightmost `@`; unpinned package, Git, local, or unknown sources are blocked or approval-required until immutable provenance is recorded
4. unsupported agents return `manual_configuration_required` actions instead of guessed config paths
5. `mcp doctor` and `skill diagnose` point to `mcp plan` when MCP dependency readiness fails
6. apply is intentionally absent until durable plan revalidation, idempotency keys, approval validation, atomic config writes, and secret non-storage are implemented

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
loom --json --root <root> skill provenance refresh <skill-id>
```

Mixed read/write command group.

Rules:

1. `inspect` is read-only and returns the recorded `sources.json` entry plus the matching `loom.lock` entry
2. `verify` is read-only and compares the current canonical skill digest against both recorded provenance and `loom.lock`
3. `refresh` is a write command; it recomputes the current canonical skill digest, updates `state/registry/sources.json` and `loom.lock`, and commits only provenance artifacts
4. `refresh` must not mutate projection state, target directories, binding rules, or live agent skill directories
5. `loom.lock` is generated from sorted source records so repeated writes are deterministic
6. missing skill sources return `SKILL_NOT_FOUND`; missing provenance records return `STATE_NOT_INITIALIZED`

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

### 11.3.7 `workflow create`, `workflow show`, `workflow plan`, `workflow preflight`, `workflow run`

```bash
loom --json --root <root> workflow create <workflow-id> --file <workflow.json> [--dry-run]
loom --json --root <root> workflow create <workflow-id> --from-skillset <skillset-id> --dry-run
loom --json --root <root> workflow show <workflow-id>
loom --json --root <root> workflow plan <workflow-id> --agent <agent> --workspace <path>
loom --json --root <root> workflow preflight <plan-id>
loom --json --root <root> workflow run <workflow-id> --agent <agent> --workspace <path> [--dry-run]
```

`workflow create` writes `state/registry/workflows.json` unless `--dry-run` is supplied. `workflow show`, `workflow preflight`, and `workflow run --dry-run` are read-only. `workflow plan` writes an auditable guarded plan under `state/registry/workflow_plans.json` without executing nodes.

Rules:

1. workflow definitions are explicit DAGs with `workflow_id`, `nodes`, `edges`, `external_inputs`, and `policy`
2. cycles, self-edges, missing edge endpoints, oversized plans, excessive depth, and missing required upstream outputs fail with `ARG_INVALID`
3. `workflow plan` requires each node skill source to exist and fails with `SKILL_NOT_FOUND` for missing skills
4. blocked or quarantined skill trust fails with `POLICY_BLOCKED`; workflow planning must not silently skip unsafe nodes
5. plans record root, registry head, workflow digest, skill source digests, ordered node ids, activation steps, required approvals, risks, and `safe_to_run=false`
6. `workflow preflight` rechecks stored plan guards against the current registry root, Git head, workflow digest, and skill digests
7. `workflow run` is a deferred surface in this version; non-dry-run execution fails with `POLICY_BLOCKED` until apply gates are implemented
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
   missing
6. if registry snapshot loading fails after rollback, the response keeps
   `ok=true` for the source rollback but sets
   `projection_reconciliation.status="registry_unavailable"`, includes a
   structured `error`, sets `live_projection_reconciled=false`, and adds a
   warning to `meta.warnings`
7. no-op rollback success returns the same reconciliation fields with
   `projection_reconciliation.status="noop"`, `source_restored=false`, and
   `registry_restored=false`; it does not initialize registry state
8. if registry state was absent before a non-noop rollback, rollback records
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
loom --json --root <root> telemetry report [--skill <skill-id>] [--skillset <skillset-id>] [--agent <agent>] [--workspace <path>] [--since <date>]
loom --json --root <root> telemetry export --format jsonl|csv --output <path> [--redacted]
loom --json --root <root> telemetry purge [--before <date>] --dry-run
loom --json --root <root> telemetry purge [--before <date>] --confirm <token>
```

`status` and `report` are read-only. `enable`, `disable`, and confirmed
`purge` mutate only `state/telemetry`. `export` writes only the explicit output
path and must reject output under registry state.

Rules:

1. telemetry is local-first and opt-in; absent config reports `enabled=false`.
2. enabled config uses `state/telemetry/config.json`; events use append-only
   `state/telemetry/events.jsonl`.
3. telemetry events use schema version 1, typed event families, hashed
   workspace/session identifiers, and `privacy.raw_prompt_stored=false`,
   `privacy.raw_code_stored=false`, `privacy.redacted=true`.
4. disabled telemetry must not append telemetry events.
5. existing malformed event lines are surfaced as quarantined warnings in
   status/report/export/purge responses; they are not silently dropped.
6. `telemetry report` summarizes usage, value, cost, drift, risk, and
   recommendation feedback. Missing upstream evidence must be reported as
   `missing`, not zero usage.
7. `telemetry export --format jsonl|csv` emits redacted typed events only and
   skips malformed lines with warnings.
8. `telemetry purge --dry-run` returns matching event count, byte impact, and a
   confirmation token; `--confirm` must match the current dry-run token before
   atomically rewriting telemetry event state.
9. `skill eval`, `skill scan`, `skill activate`, and `skill deactivate` append
   redacted telemetry events only when telemetry is enabled.
10. Panel Telemetry consumes the same backend read model at
    `/api/v1/telemetry/report` and preserves missing evidence as missing.

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
loom --json --root <root> apply <plan-id> --idempotency-key <key> [--approve <token[,token]>]
```

`plan use` creates a durable, audited plan for the same target/binding/projection setup that `loom use --apply` performs. Plan creation must not mutate registry state, Git refs, operation backlog, or live target directories; its only durable write is the command-audit event under `state/events/commands.jsonl`.

The top-level `plan` command owns durable plan creation. The top-level `apply` command owns guarded plan execution.

The plan JSON schema is versioned separately from the binary package version at `docs/schemas/agent-plan-v1.schema.json`. Current plans use `protocol_version: "1.0"` and `schema_version: "1.0"`.

Rules:

1. `plan use` validates the skill exists, records the current registry `HEAD`, records the current skill source digest, freezes resolved workspace and target-root paths, lists effects/conflicts/risks, and returns a `plan_id`
2. `apply` loads the plan from command-audit events and validates current `--root`, registry `HEAD`, skill source digest, required approvals, and idempotency key before mutation
3. `apply` is safe to retry with the same `--idempotency-key`; successful responses include `idempotency_key_digest`, not the raw key, and a successful prior apply for the same plan/key digest returns the recorded result with `idempotent_replay=true`
4. reusing an idempotency key for a different plan returns `DEPENDENCY_CONFLICT` with `conflict.code=IDEMPOTENCY_KEY_REUSED`
5. missing approval tokens return `POLICY_BLOCKED` with `conflict.code=APPROVAL_REQUIRED`, `retryable=true`, `event_cursor`, and suggested `--approve` actions
6. stale plans return `DEPENDENCY_CONFLICT` with a typed conflict such as `PLAN_STALE`, `PLAN_SOURCE_DRIFT`, or `PLAN_ROOT_MISMATCH`
7. successful apply returns the lower-level use result plus `recovery.rollback_supported=true`, a `rollback_token`, and rollback commands when available

## 13. Sync Commands

### 13.1 `sync status`

```bash
loom --json --root <root> sync status
```

Read-only.

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
3. `skill new`
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
