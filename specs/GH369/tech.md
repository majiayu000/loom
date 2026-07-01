# GH369 Tech Spec: Real Agent Eval Harness

Issue: https://github.com/majiayu000/loom/issues/369
Product spec: `specs/GH369/product.md`
Status: Draft for implementation

## Design Summary

Extend the existing `skill_eval` module without breaking offline eval:

1. Keep the current JSONL offline evaluator as `offline_fixture`.
2. Introduce an eval command router for offline/run/trigger/compare modes.
3. Add a runner trait with `mock` and opt-in `codex-cli` implementations.
4. Add an eval plan object used by dry-run and apply.
5. Persist reports outside the skill source by default.
6. Preserve typed `EVAL_FAILED` behavior for failed cases.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli/eval.rs`, `src/cli.rs` |
| command dispatch | `src/commands/mod.rs` |
| eval implementation | `src/commands/skill_eval.rs`, maybe new `src/commands/skill_eval_runner.rs` |
| report persistence | registry state helpers or new `state/registry/evals/` helper |
| tests | `tests/skill_eval.rs`, maybe new `tests/skill_eval_runner.rs` |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH369/*` |

## CLI Compatibility

Current command:

```bash
loom skill eval <skill> [--agent <agent> | --matrix <agent,agent>] [--model <model>]
```

must continue to run offline fixture eval.

Implementation options:

1. Introduce `SkillEvalCommand` subcommands while preserving the flat form through a compatibility parser.
2. Keep flat `SkillEvalArgs` and add optional mode flags.

Prefer explicit subcommands if clap can preserve backward compatibility cleanly:

```rust
enum SkillEvalCommand {
    Offline(SkillEvalOfflineArgs),
    Run(SkillEvalRunArgs),
    Trigger(SkillEvalTriggerArgs),
    Compare(SkillEvalCompareArgs),
}
```

Do not remove the old command shape in the same PR.

## Runner Interface

Use a small trait:

```rust
trait SkillEvalRunner {
    fn prepare(&self, plan: &EvalPlan) -> Result<EvalRunEnvironment>;
    fn run_case(&self, env: &EvalRunEnvironment, case: &EvalCase) -> Result<EvalCaseResult>;
    fn cleanup(&self, env: EvalRunEnvironment) -> Result<CleanupResult>;
}
```

Initial runners:

1. `offline_fixture`: wraps current behavior.
2. `mock_agent`: deterministic runner for CI.
3. `codex_cli`: shells out only when explicitly selected and executable/config are present.

Use array argument lists for any process invocation. Never build shell command strings from eval case content.

## Eval Plan

Plan fields:

```rust
struct EvalPlan {
    skill: String,
    agent: String,
    mode: EvalMode,
    runner: EvalRunnerKind,
    baseline: Option<EvalBaseline>,
    cases_path: Vec<PathBuf>,
    runs: u32,
    workspace: Option<PathBuf>,
    dry_run: bool,
    output_path: Option<PathBuf>,
    skill_version: SkillEvalVersion,
}
```

`run --dry-run` returns this plan plus resolved cases and estimated actions. It must not activate a skill, run an agent, write a report, or mutate fixture workspaces.

## Baseline Isolation

With-skill/no-skill evaluation must isolate state:

1. Use temp workspaces copied from fixtures for each run.
2. Use explicit activation plan or runner-level environment switch, not global hidden state.
3. Restore or discard temp workspace after each run.
4. Record cleanup result.
5. If cleanup fails, report it without hiding case outcome.

## Report Persistence

Default report location should be registry-owned, for example:

```text
state/registry/evals/<skill>/<timestamp>-<mode>.json
```

Rules:

1. Do not write reports into `skills/<skill>` unless `--output` points there explicitly.
2. Deterministic tests may inject timestamp or output path.
3. Redact env values, secrets, and raw prompts when a runner marks them sensitive.
4. Include report path in command response.

## Metrics

Summaries:

1. `with_skill_pass_rate`;
2. `without_skill_pass_rate`;
3. `delta`;
4. `trigger_precision`;
5. `trigger_recall`;
6. `token_overhead_ratio`;
7. `command_overhead_ratio`;
8. `duration_overhead_ratio`.

When data is unavailable, fields are `null`.

## Version Compare

`compare --from --to` should:

1. resolve both refs or `working-tree`;
2. materialize each source version in isolated temp dirs;
3. run the same cases and runner for both versions;
4. report delta and regressions;
5. avoid changing the actual skill source working tree.

If materializing refs safely is not available in the first implementation, return a typed unsupported error rather than mutating the working tree.

## Error Handling

Typed failures:

1. missing skill;
2. missing or invalid cases;
3. unsupported runner;
4. runner executable missing;
5. unsafe workspace fixture path;
6. eval case failed;
7. report persistence failed;
8. cleanup failed.

Failed cases return `EVAL_FAILED` with report details, preserving current behavior.

## Test Plan

Focused tests:

1. current offline eval command still passes existing fixtures.
2. missing fixtures remain blank successful report with warning.
3. failing offline cases still return `EVAL_FAILED`.
4. `run --dry-run` returns plan and mutates nothing.
5. mock runner compares with-skill/no-skill pass-rate delta.
6. trigger command reports precision/recall.
7. compare command handles mock refs or typed unsupported safely.
8. report persistence writes registry-owned report and response includes path.
9. codex-cli runner missing executable returns typed error.
10. cleanup failure is represented in report.
11. sensitive fields are redacted.

Suggested commands:

```bash
git diff --check
cargo test --test skill_eval
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
```

For a spec-only PR:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH369-eval-harness-spec/specs/GH369
```

## Rollback

Rollback can remove:

1. new eval subcommands;
2. runner trait and runner implementations;
3. report persistence helper;
4. focused tests and docs updates.

Existing offline eval behavior should remain unchanged or be restored by reverting to the pre-runner `cmd_skill_eval` path.

## Risks

1. Real runner could mutate user workspace. Mitigation: temp workspace by default and explicit fixture/workspace handling.
2. Shell injection through prompts/cases. Mitigation: array argv only, no shell string construction.
3. CI flakiness from real agents. Mitigation: mock runner for CI and real runner opt-in only.
4. Sensitive data in reports. Mitigation: redaction and no raw env/secret persistence.
