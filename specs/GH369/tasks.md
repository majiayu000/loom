# GH369 Tasks: Real Agent Eval Harness

Issue: https://github.com/majiayu000/loom/issues/369
Product spec: `specs/GH369/product.md`
Tech spec: `specs/GH369/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the eval harness foundation in safe slices:

```text
offline compatibility + dry-run plan + mock runner + with/no-skill baseline report + report persistence
```

Do not implement:

```text
network-by-default runners, untrusted script execution, Panel dashboard, automatic activation without plan
```

## Tasks

- [ ] `SP369-T001` Owner: cli | Done when: current `skill eval <skill>` remains compatible and new offline/run/trigger/compare surfaces parse without ambiguity | Verify: `cargo test --test cli_surface`
- [ ] `SP369-T002` Owner: planner | Done when: `skill eval run --dry-run` returns an execution plan and resolved cases without running agents or mutating workspaces | Verify: `cargo test --test skill_eval`
- [ ] `SP369-T003` Owner: runner | Done when: runner trait supports offline_fixture, mock_agent, and opt-in codex_cli missing-runner errors | Verify: `cargo test --test skill_eval`
- [ ] `SP369-T004` Owner: baseline | Done when: mock runner compares with-skill and no-skill runs in isolated workspaces and reports pass-rate delta | Verify: `cargo test --test skill_eval`
- [ ] `SP369-T005` Owner: reporting | Done when: reports include version metadata, trigger precision/recall, available overhead metrics, cleanup result, redaction, and registry-owned persistence path | Verify: `cargo test --test skill_eval`
- [ ] `SP369-T006` Owner: compare | Done when: compare mode handles two refs or returns a typed unsupported error without mutating the skill working tree | Verify: `cargo test --test skill_eval`
- [ ] `SP369-T007` Owner: docs | Done when: CLI contract and specs document runner modes, dry-run, report persistence, safety limits, and verification commands | Verify: `git diff --check`

### SP369-T1: Preserve Offline Compatibility And Add CLI

Owner: implementation

Files:

- `src/cli/eval.rs`
- `src/cli.rs`
- `src/commands/mod.rs`

Done when:

- Existing `loom skill eval <skill>` tests pass unchanged.
- `skill eval offline`, `skill eval run`, `skill eval trigger`, and `skill eval compare` are exposed or equivalent compatible flags exist.
- CLI help clearly marks real runner behavior as opt-in.

Verify:

```bash
cargo test --test cli_surface
cargo test --test skill_eval
```

### SP369-T2: Add Eval Plan And Dry-Run

Owner: implementation

Files:

- `src/commands/skill_eval.rs`
- maybe `src/commands/skill_eval_runner.rs`

Done when:

- `run --dry-run` resolves skill version, cases, runner, baseline, run count, workspace, and output path.
- Dry-run writes nothing and invokes no runner.
- Invalid cases fail closed with typed errors.

Verify:

```bash
cargo test --test skill_eval
```

### SP369-T3: Add Runner Abstraction And Mock Runner

Owner: implementation

Done when:

- `offline_fixture` wraps existing behavior.
- `mock_agent` deterministically returns case results for CI.
- `codex_cli` is explicit opt-in and reports typed missing executable/config errors when unavailable.
- Process invocation uses array args only.

Verify:

```bash
cargo test --test skill_eval
```

### SP369-T4: Implement Baseline And Compare Reports

Owner: implementation
Depends on: SP369-T2, SP369-T3

Done when:

- With-skill/no-skill runs are isolated.
- Summary includes pass-rate delta and recommendation decision.
- `compare --from --to` either evaluates isolated versions or returns typed unsupported without mutating source.

Verify:

```bash
cargo test --test skill_eval
```

### SP369-T5: Persist Reports Safely

Owner: implementation

Done when:

- Default reports are written under registry-owned eval state or explicit output path.
- Response includes report path.
- Sensitive fields are redacted.
- Cleanup failure is represented in report.

Verify:

```bash
cargo test --test skill_eval
```

### SP369-T6: Verification And Handoff

Owner: implementation

Done when:

- Focused eval tests pass.
- Full compile check passes.
- SpecRail packet validation passes.
- PR body uses `Refs #369` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test skill_eval
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH369-eval-harness-spec/specs/GH369
```

## Handoff Notes

- Use `Refs #369` for partial implementation slices.
- Keep real agent runners opt-in and out of default CI.
- Do not treat eval success as safety proof.
- Do not persist raw secrets, env values, or unredacted sensitive prompts in default reports.
