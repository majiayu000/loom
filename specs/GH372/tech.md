# GH372 Tech Spec: Skill Improve And Regression Workflow

Issue: https://github.com/majiayu000/loom/issues/372
Product spec: `specs/GH372/product.md`
Status: Draft for implementation

## Design Summary

Build a preflight aggregator over existing and planned single-skill checks:

1. Create one `SkillPreflightReport`.
2. Implement read-only `skill improve` and `skill regression`.
3. Add `--preflight` to `skill save` and `skill release`.
4. Reuse lint, safety, deps, eval, diff, and git drift helpers.
5. Block mutating save/release when gates fail.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new `src/cli/improve.rs`, existing save/release args |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| preflight module | new `src/commands/skill_preflight.rs` |
| save/release integration | `src/commands/skill_cmds.rs`, `src/commands/skill_cmds/save.rs`, release code paths |
| checks | lint, future #370 safety, #371 deps, #369 eval, existing diff/history helpers |
| tests | new `tests/skill_preflight.rs`, extend save/release tests |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH372/*` |

## Report Model

```rust
struct SkillPreflightReport {
    skill: String,
    baseline: String,
    target: String,
    checks: BTreeMap<String, CheckStatus>,
    regressions: Vec<RegressionFinding>,
    recommendation: PreflightRecommendation,
    mutation_allowed: bool,
}
```

Check statuses:

```text
pass
warning
fail
skipped
unknown
```

## Improve

`skill improve` is read-only:

1. validate skill exists;
2. resolve baseline default `HEAD`;
3. detect skill source drift;
4. run lint;
5. run safety/deps when available, otherwise mark skipped;
6. run offline eval;
7. optionally run real eval compare only when `--real-eval` is set;
8. produce recommendation.

## Regression

`skill regression` should compare baseline and target:

1. materialize or inspect both refs without mutating the working tree;
2. run comparable checks on both;
3. emit only regressions, not every unchanged finding;
4. fail with typed error when regression threshold is exceeded.

If safe ref materialization is not available in the first implementation, return typed unsupported for that sub-path rather than checking out refs destructively.

## Save/Release Preflight

Before mutating:

1. run preflight on selected skill;
2. if failed, return typed error with `error.details.report`;
3. if passed, proceed with existing save/release flow;
4. preserve existing rollback behavior.

No preflight command should stage unrelated files.

## Test Plan

Focused tests:

1. improve no drift returns recommendation keep/save as appropriate.
2. improve with safe drift reports lint/eval pass.
3. regression detects lint regression.
4. regression detects safety regression when #370 is available or via fixture stub.
5. regression detects eval regression.
6. save --preflight passes and commits when gates pass.
7. save --preflight blocks and does not commit when gates fail.
8. release --preflight blocks dirty/failed state and rejects a missing or
   self-referential release baseline.
9. report is serializable and stable.

Suggested commands:

```bash
git diff --check
cargo test --test skill_preflight
cargo test --test skill_eval
cargo check --workspace --all-targets --all-features
```

For a spec-only PR:

```bash
SPEC_RAIL_REPO=/path/to/specrail
python3 "$SPEC_RAIL_REPO/checks/check_workflow.py" --repo "$SPEC_RAIL_REPO" --spec-dir specs/GH372
```

## Rollback

Rollback can remove:

1. improve/regression CLI;
2. preflight module;
3. save/release `--preflight` flags;
4. tests and docs updates.

Existing save/release behavior must remain unchanged when `--preflight` is absent.

## Risks

1. Preflight blocks too much while dependent checks are incomplete. Mitigation: skipped/unknown statuses until #369/#370/#371 land.
2. Regression comparison mutates working tree. Mitigation: use temp materialization or typed unsupported.
3. Save/release bypasses gates. Mitigation: one shared preflight function for dry-run and mutating flows.
