# GH377 Tech Spec: Skillsets And Bundles

Issue: https://github.com/majiayu000/loom/issues/377
Product spec: `specs/GH377/product.md`
Status: Implementation update for lifecycle completion PR

## Design Summary

Implement `skillset` as a registry-state feature that reuses single-skill lifecycle primitives:

1. Add a top-level `loom skillset` command group.
2. Persist skillsets in `state/registry/skillsets.json`.
3. Reuse existing skill inventory read model to validate and summarize members.
4. Reuse single-skill activation/deactivation for member projection.
5. Reuse single-skill offline eval reports for member eval aggregation.
6. Release and rollback skillset definitions with Git tags/refs.

This avoids duplicate activation, eval, trust, or skill source version state.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli/skillset.rs`, `src/cli.rs` |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs`, `src/commands/meta.rs` |
| implementation | `src/commands/skillset_cmds.rs` |
| tests | `tests/skillset_cli.rs`, `tests/cli_surface.rs` |
| docs/specs | `specs/GH377/*`, `README.md`, `docs/LOOM_CLI_CONTRACT.md` |

## Data Model

Persist a deterministic JSON file:

```text
state/registry/skillsets.json
```

Shape:

```json
{
  "schema_version": 1,
  "skillsets": [
    {
      "id": "coding-flow",
      "description": "Skills for coding tasks.",
      "members": [
        {
          "skill_id": "fixflow",
          "role": "execution",
          "required": true
        }
      ],
      "created_at": "2026-06-30T00:00:00Z",
      "updated_at": "2026-06-30T00:00:00Z"
    }
  ]
}
```

Rules:

1. Sort `skillsets` by `id`.
2. Sort `members` by `skill_id`.
3. Write pretty JSON with trailing newline, matching existing registry style when possible.
4. Treat absent file as empty model.
5. Reject malformed JSON with typed error rather than overwriting.

## CLI Types

Add:

```rust
Command::Skillset { command: SkillsetCommand }
```

Subcommands:

```rust
enum SkillsetCommand {
    Create(SkillsetCreateArgs),
    Add(SkillsetAddArgs),
    Remove(SkillsetMemberArgs),
    Show(SkillsetShowArgs),
    Lint(SkillsetShowArgs),
    Activate(SkillsetActivateArgs),
    Deactivate(SkillsetActivateArgs),
    Eval(SkillsetEvalArgs),
    Release(SkillsetReleaseArgs),
    Rollback(SkillsetRollbackArgs),
}
```

Argument notes:

1. `SkillsetCreateArgs`: `name`, `--description`.
2. `SkillsetAddArgs`: `name`, `skill`, `--role`, `--required`, `--optional`.
3. `--required` and `--optional` conflict. Default is required.
4. `Show` and `Lint` accept name only; global `--json` controls envelope.
5. `Activate` and `Deactivate` accept `name`, `--agent`, `--scope`, `--workspace`, `--profile`, and `--dry-run`.
6. `Eval` accepts `name`, `--agent`, and `--baseline no-skill|single-skills`.
7. `Release` accepts `name` and `version`.
8. `Rollback` accepts `name` and `--to <version|ref>`.

## Validation

Use existing `validate_skill_name` for:

1. skillset id
2. member skill id

Use `build_skill_read_model` to check skill existence.

Typed failures:

1. duplicate skillset: `ARG_INVALID` or a more specific existing error if available.
2. missing skillset: `SKILL_NOT_FOUND` if no better code exists.
3. missing member skill on add: `SKILL_NOT_FOUND`.
4. duplicate member: `ARG_INVALID`.
5. missing member on remove: `SKILL_NOT_FOUND`.
6. malformed `skillsets.json`: `INTERNAL_ERROR` or existing parse/state error pattern.
7. required member activation/eval failure: preserve the underlying typed error code and include member context.
8. invalid release target: `POLICY_BLOCKED` with lint details.
9. unsafe version/ref token: `ARG_INVALID`.

## Command Behavior

### create

1. Ensure write repo/layout readiness.
2. Load existing skillsets.
3. Validate id.
4. Reject duplicate id.
5. Write state.
6. Stage/commit registry state following existing write-command patterns.
7. Return the created skillset and next actions.

### add

1. Ensure write repo/layout readiness.
2. Validate skillset id and skill id.
3. Check member skill exists in current read model.
4. Reject duplicate member.
5. Add member and write state.
6. Return updated skillset.

### remove

1. Ensure write repo/layout readiness.
2. Validate ids.
3. Reject unknown skillset or unknown member.
4. Remove member only from skillset; never delete skill source.
5. Return updated skillset.

### show

1. Read state.
2. Return full skillset with member summaries from `build_skill_read_model`.
3. Missing or drifted member should appear with `missing=true`.

### lint

1. Read state.
2. Validate member existence.
3. Validate no duplicate members even if manual file drift created duplicates.
4. Warn when member list is empty.
5. Invalid when required members are missing.
6. Return structured findings.

### activate

1. Read the skillset and current inventory.
2. For every member, call single-skill activation dry-run to enforce existence, lint/safety/trust, and target planning.
3. Required member preflight failure aborts before mutation with the underlying typed error.
4. Optional member preflight failure is reported as skipped with a warning.
5. `--dry-run` returns `activation_plan` and summary only.
6. Non-dry-run calls single-skill activation for each ready member.
7. If a later member fails after earlier mutations, deactivate already activated non-noop members in reverse order and return rollback results plus recovery commands.

### deactivate

1. Read the skillset.
2. Call single-skill deactivation dry-run or apply for each member.
3. Required member failure aborts with typed member context; optional member failure is skipped with warning.

### eval

1. Run existing offline member eval for each member with the requested agent.
2. Aggregate `case_count`, `passed`, `failed`, `skipped`, token/command counts, permissions, and aggregate score.
3. Return `EVAL_FAILED` with the aggregate report when any required member has failing cases.
4. `skillsets/<name>/evals/` end-to-end fixtures are detected and reported as deferred; no fake end-to-end pass/fail is produced.

### release

1. Validate the skillset exists and `lint` is valid.
2. Create annotated tag `release/skillset/<name>/<version>` at current HEAD.
3. Return the tag and released ref.

### rollback

1. Resolve `--to` as `release/skillset/<name>/<version>` first, then as a raw safe Git ref.
2. Read `state/registry/skillsets.json` from that ref with `git show`.
3. Replace only the matching skillset record in the current file.
4. Commit registry state if changed; no member skill source files are checked out.

## Audit And Git Behavior

Write commands should be durable and auditable:

1. `skillset.create`
2. `skillset.add`
3. `skillset.remove`
4. `skillset.activate`
5. `skillset.deactivate`
6. `skillset.release`
7. `skillset.rollback`

Read commands:

1. `skillset.show`
2. `skillset.lint`
3. `skillset.eval`

Follow existing `skill` command behavior:

1. write layout is initialized;
2. registry state is staged;
3. commit message is deterministic and human-readable;
4. command audit records intent when command audit is enabled.

## Test Plan

Focused integration tests in `tests/skillset_cli.rs`:

1. create shows persisted empty skillset.
2. duplicate create fails without changing state.
3. add existing skill with role and required flag.
4. add missing skill fails.
5. add duplicate member fails.
6. remove member preserves source skill directory.
7. remove missing member fails.
8. show includes member skill read-model summary.
9. lint empty skillset returns warning and valid true.
10. lint detects manually introduced missing required member.
11. activate dry-run returns plans without target writes.
12. activate applies members through single-skill activation.
13. partial activation failure rolls back projected members or reports recovery.
14. eval aggregates member eval results.
15. release tags the skillset definition and rollback restores it.

Suggested commands:

```bash
cargo test --test skillset_cli
cargo check --workspace --all-targets --all-features
git diff --check
```

If docs/contracts are updated, also run any existing doc consistency checks in the repository.

## Rollback

The implementation is isolated to:

1. `state/registry/skillsets.json` as a new optional state file.
2. a new CLI command group.
3. tests and specs.

Rollback can remove the command group and ignore/delete the optional state file without changing existing skill source/projection behavior.

## Risks

1. File-level state may become a second source of truth if later skill inspect logic is duplicated. Mitigation: use `build_skill_read_model` for summaries.
2. End-to-end skillset eval fixtures are detected but not run. Mitigation: return explicit deferred status and keep closing language out of partial PRs if this remains required for #377 closure.
3. Future role vocabulary may need constraints. Mitigation: keep role free-form and non-semantic in v1.
