# GH369 Product Spec: Real Agent Eval Harness

Issue: https://github.com/majiayu000/loom/issues/369
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

把 `loom skill eval` 从当前的 offline fixture checker 扩展成可比较真实 agent 效果的 eval harness，同时保持现有离线模式快速、无网络、CI 可运行。

目标比较维度：

1. with skill active vs no skill baseline;
2. old version vs new version;
3. different agents/models when explicitly available;
4. trigger quality, task pass rate, token/command/time overhead.

## Users

1. 单 skill 作者：想知道 skill 是否真的改善任务结果，而不只是 JSONL fixture 合格。
2. 维护者：需要可复现报告来决定是否推荐 activation、release、rollback 或 regression fix。
3. Agent：需要 dry-run plan、mock runner 和 typed failure，避免在 CI 中调用真实外部 agent。

## Scope For First Implementation

第一批实现应覆盖：

1. 保持现有 `loom skill eval <skill>` offline behavior。
2. 新增 subcommands 或兼容命令形态：

```bash
loom skill eval offline <skill> [--agent <agent> | --matrix <agent,agent>] [--model <model>]
loom skill eval run <skill> --agent codex --baseline no-skill [--workspace <path>] [--cases <path>] [--runs <n>] [--runner mock|codex-cli] [--dry-run]
loom skill eval trigger <skill> --agent codex [--cases evals/triggers.jsonl] [--runs <n>] [--runner mock|codex-cli]
loom skill eval compare <skill> --from <ref> --to <ref|working-tree> --agent codex [--cases <path>] [--runner mock|codex-cli]
```

3. `mock` runner 可在 CI 中比较 with-skill/no-skill。
4. `codex-cli` runner 必须显式 opt-in，且缺少可执行文件或授权时返回 typed error。
5. Eval report 持久化到 registry state 或指定 output path，不默认写回 skill source。

## Non-Goals

1. 不默认调用网络或真实 agent。
2. 不把 eval pass 当作 safety guarantee。
3. 不自动 activate/deactivate production user skills without a dry-run/apply plan.
4. 不运行 untrusted scripts unless an explicit unsafe/allow flag is introduced and documented.
5. 不把 prompt、secret、env value 原样写入默认报告；需要 redaction.
6. 不实现 Panel dashboard；#375 可消费稳定报告后再做 UI。

## Behavior Invariants

1. Existing offline eval behavior remains backward compatible.
2. `run --dry-run` returns a full execution plan and mutates nothing.
3. Real-agent evals run in temp workspaces or explicitly provided fixture workspaces.
4. With-skill and no-skill runs must be isolated; baseline state cannot leak.
5. Runner selection is explicit; CI defaults to `mock` or offline.
6. Report includes skill source version metadata: head tree OID, last source commit, compared refs when applicable.
7. Failed evals return typed `EVAL_FAILED` with the full report in `error.details.report`.
8. No data means blank/null, not fabricated pass/fail metrics.
9. Token/command/time metrics are included only when runner can measure them.
10. Cleanup failure is reported and does not hide the eval result.

## Eval Cases

Continue supporting existing JSONL files and add fields for real runs.

`evals/triggers.jsonl`:

```json
{"id":"t1","prompt":"Fix the failing CI in this repo","should_trigger":true,"expected_skill":"fixflow"}
{"id":"t2","prompt":"Summarize this README","should_trigger":false}
```

`evals/tasks.jsonl`:

```json
{
  "id": "task-1",
  "prompt": "Diagnose and fix the failing test suite.",
  "workspace_fixture": "fixtures/failing-tests",
  "checks": {
    "exit_code": 0,
    "files_changed": ["src/foo.rs"],
    "commands_contains": ["cargo test"],
    "outcome_contains": ["tests pass"],
    "max_tokens": 120000,
    "max_commands": 30
  }
}
```

## Output Contract

```json
{
  "skill": "fixflow",
  "agent": "codex",
  "mode": "real_agent_baseline",
  "runner": "mock",
  "summary": {
    "case_count": 12,
    "with_skill_pass_rate": 0.75,
    "without_skill_pass_rate": 0.58,
    "delta": 0.17,
    "trigger_precision": 0.91,
    "trigger_recall": 0.80,
    "token_overhead_ratio": 0.12,
    "command_overhead_ratio": -0.05
  },
  "skill_version": {
    "head_tree_oid": "...",
    "last_source_commit": "..."
  },
  "runs": [],
  "decision": {
    "recommend_activation": true,
    "reason": "positive pass-rate delta with acceptable overhead"
  },
  "security_model": {
    "eval_success_is_safety_guarantee": false
  }
}
```

## Acceptance Criteria

1. Existing `loom skill eval <skill>` offline fixture behavior still works.
2. `loom skill eval run <skill> --baseline no-skill --dry-run` returns an execution plan without running any agent.
3. A mock runner can compare with-skill and no-skill outcomes in CI.
4. Reports include pass-rate delta, trigger precision/recall when trigger cases exist, overhead metrics when available, and source version metadata.
5. `compare --from --to` can evaluate two source refs or working tree against the same cases.
6. Reports can be persisted outside the skill source with deterministic filenames or explicit output path.
7. Failed evals use typed `EVAL_FAILED` errors with full report details.
8. Tests cover offline compatibility, dry-run, mock with/without baseline, version compare, report persistence, missing runner, cleanup failure, and redaction of sensitive fields.
