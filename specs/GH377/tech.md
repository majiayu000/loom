# GH377 Tech Spec: Skillsets And Bundles

Issue: https://github.com/majiayu000/loom/issues/377
Product spec: `specs/GH377/product.md`
Status: Draft for implementation

## Design Summary

Implement the first mergeable `skillset` slice as a registry-state feature:

1. Add a top-level `loom skillset` command group.
2. Persist skillsets in `state/registry/skillsets.json`.
3. Reuse existing skill inventory read model to validate and summarize members.
4. Add create/add/remove/show/lint only.
5. Defer activation/eval/release/rollback until their blocking primitives exist.

This keeps #377 useful without inventing duplicate activation, eval, or trust state.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs` |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| implementation | new `src/commands/skillset_cmds.rs` |
| tests | new `tests/skillset_cli.rs`, maybe `tests/cli_surface.rs` |
| docs/specs | `specs/GH377/*`, maybe `docs/LOOM_CLI_CONTRACT.md` if CLI contract update is required |

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
}
```

Argument notes:

1. `SkillsetCreateArgs`: `name`, `--description`.
2. `SkillsetAddArgs`: `name`, `skill`, `--role`, `--required`, `--optional`.
3. `--required` and `--optional` conflict. Default is required.
4. `Show` and `Lint` accept name only; global `--json` controls envelope.

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

## Audit And Git Behavior

Write commands should be durable and auditable:

1. `skillset.create`
2. `skillset.add`
3. `skillset.remove`

Read commands:

1. `skillset.show`
2. `skillset.lint`

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
2. Activation defaults could prematurely encode #367 decisions. Mitigation: defer activation defaults in this slice.
3. Future role vocabulary may need constraints. Mitigation: keep role free-form and non-semantic in v1.
