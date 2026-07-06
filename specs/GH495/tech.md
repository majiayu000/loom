# GH495 Technical Spec

Issue: https://github.com/majiayu000/loom/issues/495
Product spec: `specs/GH495/product.md`
Status: Draft for implementation

## Design Summary

Add a real Codex CLI runner beside the existing mock runner:

- keep `MockAgentRunner` as the default;
- add `CodexCliRunner` in `src/commands/skill_eval_harness/codex_cli.rs`;
- route `skill eval run` through a runner factory based on `EvalRunnerArg`;
- route `skill eval trigger --runner codex-cli` through the real runner instead of lexical fixture inference;
- make `ensure_runner_available` perform only executable/authorization checks for Codex CLI;
- keep `skill eval compare --runner codex-cli` fail-loud until cross-ref real execution is implemented.

## Codex Invocation

`CodexCliRunner` invokes:

```text
codex exec --json --cd <case-workspace> --skip-git-repo-check --sandbox workspace-write --output-last-message <tmp-file> <prompt>
```

The process current directory is also set to the case workspace. The prompt differs for:

- with-skill task variant;
- no-skill baseline variant;
- trigger decision case.

The runner captures stdout/stderr to temp files outside the case workspace, polls the child process with a timeout, and kills it on timeout. The default timeout is bounded and can be overridden for tests by `LOOM_EVAL_CODEX_TIMEOUT_MS`.

## Trace Parsing

The parser treats stdout as JSONL:

- every non-empty line must be valid JSON;
- text-like fields are collected as observable output;
- command-like fields are collected for command checks;
- usage/token fields are collected for max-token checks;
- the `--output-last-message` file wins over collected text when present.

Unparseable JSONL, empty trace, spawn failure, and timeout are runner failures and return `CommandFailure` with `ErrorCode::EvalFailed`.

## Scoring

Task scoring reuses the existing checks:

- `outcome_contains` against the final output;
- `commands_contains` against parsed commands;
- `files_changed` against a before/after workspace digest snapshot;
- `exit_code` against Codex process exit code;
- `max_tokens` and `max_commands` against parsed metrics.

Trigger scoring asks Codex to return a JSON object containing a trigger boolean, then compares that observed value to the fixture expectation.

## Compile Integration

Add `--require-real-eval` to `skill compile`.

- Default compile path remains `offline_fixture`.
- With `--require-real-eval`, compile runs `skill eval run` using `EvalRunnerArg::CodexCli` and embeds `CompileEvalEvidence.mode = "real_codex_cli"` when the eval passes.
- Real runner infrastructure failures propagate as command failures.
- Real eval case failures mark the eval gate failed and keep the artifact non-valid, matching existing compile gate semantics.

## Compatibility

1. Default CLI behavior remains mock/offline.
2. Existing offline reports still use `mode: offline_fixture`.
3. Existing mock baseline tests remain valid.
4. New report fields are additive except Codex CLI run reports use `mode: real_codex_cli`.

## Test Plan

Focused tests:

1. fake Codex CLI success for `skill eval run --runner codex-cli`;
2. fake Codex CLI trigger decision for `skill eval trigger --runner codex-cli`;
3. unparseable JSONL fails with typed `runner_trace_unparseable`;
4. missing executable and missing authorization gates remain typed;
5. `skill compile --require-real-eval` embeds `real_codex_cli` evidence.

Suggested commands:

```bash
git diff --check
cargo test --test skill_eval
cargo test --test skill_compile
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

## Rollback

Remove `CodexCliRunner`, restore `ensure_runner_available` to return `runner_unsupported`, remove `--require-real-eval`, and delete the GH495 tests/specs. No state migration is introduced.
