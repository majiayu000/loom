# GH372 Tasks: Skill Improve And Regression Workflow

Issue: https://github.com/majiayu000/loom/issues/372
Product spec: `specs/GH372/product.md`
Tech spec: `specs/GH372/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the preflight foundation:

```text
skill improve + skill regression + save/release --preflight gates + shared report
```

Do not implement:

```text
LLM patch generation, real-agent eval by default, destructive ref checkout, policy override
```

## Tasks

- [ ] `SP372-T001` Owner: cli | Done when: `skill improve --real-eval`, `skill regression`, `skill save --preflight`, and `skill release --preflight --baseline <ref>` parse while legacy save/release remain unchanged | Verify: `cargo test --test cli_surface`
- [ ] `SP372-T002` Owner: preflight | Done when: a shared read-only `SkillPreflightReport` aggregates drift, lint, safety, deps, eval, diff, and recommendation statuses | Verify: `cargo test --test skill_preflight`
- [ ] `SP372-T003` Owner: regression | Done when: regression compares baseline to target/working-tree without destructive checkout and reports threshold failures | Verify: `cargo test --test skill_preflight`
- [ ] `SP372-T004` Owner: save-release | Done when: save/release preflight blocks failed gates before staging/tagging and includes full report in error details | Verify: `cargo test --test skill_preflight`
- [ ] `SP372-T005` Owner: docs | Done when: CLI contract and specs document preflight checks, skipped dependencies, thresholds, and verification commands | Verify: `git diff --check`

### SP372-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/cli/improve.rs`
- save/release args
- `src/commands/mod.rs`

Done when:

- New commands parse.
- Real-agent eval requires the explicit `--real-eval` flag.
- Release preflight requires a baseline that is not the candidate ref itself.
- Save/release without `--preflight` keep current behavior.
- Help output documents read-only default.

Verify:

```bash
cargo test --test cli_surface
```

### SP372-T2: Add Preflight Report

Owner: implementation

Files:

- `src/commands/skill_preflight.rs`

Done when:

- Report is stable JSON.
- Missing dependent checks become skipped/unknown, not fabricated pass.
- Recommendation is deterministic.

Verify:

```bash
cargo test --test skill_preflight
```

### SP372-T3: Add Regression Gate

Owner: implementation

Done when:

- Lint/eval/security/dependency regressions are reported.
- Unsafe ref materialization returns typed unsupported instead of destructive checkout.
- Thresholds are documented constants.

Verify:

```bash
cargo test --test skill_preflight
```

### SP372-T4: Integrate Save/Release Preflight

Owner: implementation

Done when:

- Failed preflight blocks before commit/tag.
- Passing preflight proceeds through existing save/release.
- Rollback behavior remains intact.

Verify:

```bash
cargo test --test skill_preflight
cargo test --test write
```

### SP372-T5: Verification And Handoff

Owner: implementation

Done when:

- Focused tests pass.
- Full compile check passes.
- SpecRail packet validation passes.
- PR body uses `Refs #372` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test skill_preflight
cargo check --workspace --all-targets --all-features
SPEC_RAIL_REPO=/path/to/specrail
python3 "$SPEC_RAIL_REPO/checks/check_workflow.py" --repo "$SPEC_RAIL_REPO" --spec-dir specs/GH372
```

## Handoff Notes

- Use `Refs #372` for partial implementation slices.
- Keep improve/regression read-only.
- Do not destructively checkout refs for comparison.
- Keep save/release behavior unchanged unless `--preflight` is used.
