# GH496 Product Spec: Telemetry Emitters And Feedback Signals

Issue: https://github.com/majiayu000/loom/issues/496
Parent: https://github.com/majiayu000/loom/issues/385
Status: Draft for implementation
Locale: zh-CN

## Goal

补齐 GH385 telemetry 基础设施中已经声明但没有真实 writer 的生产信号闭环：

```text
skill.invocation -> skill.error -> recommendation.feedback
```

完成后，`loom telemetry report`、`skill inspect --include-telemetry` 和
`skill recommend` 不再只能看到 activation/eval/safety 的离线或生命周期信号，而是可以基于真实使用、错误和推荐反馈给出可审计的证据。

## Users

1. Agent wrapper 作者：需要用一个稳定的本地命令记录某个 skill 被实际调用、成功或失败。
2. Skill 维护者：需要在 `skill inspect` 里看到某个 skill 的近期使用和错误状态，而不是只看到 eval/safety。
3. 推荐系统使用者：需要 `skill recommend` 能把真实使用、错误率和推荐反馈纳入排序证据。
4. 仪表盘/报表消费者：需要知道某个 telemetry 字段是没有数据、未接线，还是已经有真实事件。

## Non-Goals

1. 不实现 hosted telemetry、远端上传或跨机器同步。
2. 不记录 raw prompt、raw output、源码片段、环境变量或 secret。
3. 不实现 #495 的 real-agent eval runner。
4. 不重写 GH385 的 telemetry config、event storage、export、purge 基础设施。
5. 不把 command audit 自动导入 telemetry。
6. 不引入 LLM/semantic ranking；推荐排序仍然是 deterministic scoring。
7. 不把没有 telemetry 数据的 skill 当作失败或低质量；缺失数据必须显式标记为 `missing` 或 `not_instrumented`。

## Behavior Invariants

1. Telemetry 仍然是 local-only opt-in：未配置或 disabled 时不写 `state/telemetry/events.jsonl`。
2. `skill.invocation`、`skill.error`、`recommendation.feedback` 每类至少有一个真实生产 emitter。
3. 显式 telemetry hook 命令在 telemetry disabled 时必须返回结构化状态，不能假装已记录事件。
4. 已启用 telemetry 时，事件写入失败必须返回 typed error；不能静默 fallback 到无事件。
5. 所有事件继续通过 GH385 的 redaction 和 validation 路径写入。
6. `skill.error` 只能记录结构化失败分类和数值指标，不能记录原始错误输出、prompt 或文件内容。
7. `recommendation.feedback` 只能记录用户或调用方明确表达的 accepted/rejected/ignored，不从普通搜索结果自动推断。
8. `telemetry report` 必须区分：
   - `available`: 有真实事件；
   - `missing`: emitter 已接线但当前过滤条件没有事件；
   - `not_instrumented`: 该聚合字段对应的事件类型没有任何授权 emitter。
9. `skill inspect --include-telemetry` 必须展示 per-skill usage/error/feedback 摘要，并保留 telemetry disabled/missing 状态。
10. `skill recommend` 可以消费 telemetry 证据，但所有分值影响必须可解释并出现在 `score_inputs`。
11. 没有 telemetry 证据时，推荐排序不能伪造零值优势或惩罚；该 evidence block 应保持 missing/blank。
12. Agent、workspace、skillset filters 必须复用现有 telemetry report 语义；不新增不一致的过滤逻辑。

## User-Facing CLI

新增一个轻量 hook 入口，用于 agent wrapper 或外部编排器记录实际 skill 使用：

```bash
loom skill used <skill> \
  [--agent <agent>] \
  [--workspace <path>] \
  [--session-id <id>] \
  [--tokens-in <n>] \
  [--tokens-out <n>] \
  [--commands <n>] \
  [--duration-ms <n>] \
  [--success|--error] \
  [--failure-category <category>]
```

新增一个显式推荐反馈入口：

```bash
loom skill feedback <skill> \
  --feedback accepted|rejected|ignored \
  [--agent <agent>] \
  [--workspace <path>] \
  [--session-id <id>] \
  [--task <text>]
```

Notes:

1. `skill used` 默认记录 successful `skill.invocation`。
2. `skill used --error` 记录 `skill.error`，并要求 `--failure-category`。
3. `skill feedback` 记录 `recommendation.feedback`。
4. `--task` 只用于本地事件关联或 hash，不得以 raw text 写入 telemetry event。
5. 命令输出必须包含 `recorded: true|false`、`event_type`、`event_id`（若写入）和 `reason`（若未写入）。

## JSON Output Expectations

`loom skill used demo --agent codex --success --json`:

```json
{
  "skill": "demo",
  "event_type": "skill.invocation",
  "recorded": true,
  "event_id": "evt_...",
  "telemetry": {
    "enabled": true,
    "mode": "local-only"
  }
}
```

`loom skill used demo --agent codex --error --failure-category timeout --json`:

```json
{
  "skill": "demo",
  "event_type": "skill.error",
  "recorded": true,
  "event_id": "evt_...",
  "failure_category": "timeout"
}
```

`loom skill feedback demo --feedback accepted --agent codex --json`:

```json
{
  "skill": "demo",
  "event_type": "recommendation.feedback",
  "recorded": true,
  "event_id": "evt_...",
  "feedback": "accepted"
}
```

`telemetry report` should expose event-family instrumentation:

```json
{
  "instrumentation": {
    "skill.invocation": {
      "status": "instrumented",
      "emitters": ["skill.used"]
    },
    "skill.error": {
      "status": "instrumented",
      "emitters": ["skill.used --error"]
    },
    "recommendation.feedback": {
      "status": "instrumented",
      "emitters": ["skill.feedback"]
    }
  }
}
```

## Recommendation Behavior

`skill recommend` and `skill resolve` should add telemetry evidence only when it
exists under the requested filters:

1. recent successful invocations may add a small bounded positive score;
2. recent `skill.error` events or high error ratio add a bounded risk penalty;
3. accepted recommendation feedback adds a bounded positive score;
4. rejected feedback adds a bounded negative score;
5. ignored feedback is evidence but should not be treated as rejection;
6. all effects must appear under `score_inputs` with stable field names.

## Acceptance Criteria

1. `skill.invocation`, `skill.error`, and `recommendation.feedback` each have at least one production emitter.
2. Disabled or absent telemetry does not create event state, and explicit hook commands report `recorded=false` with a structured reason.
3. Enabled telemetry writes redacted events for successful usage, failed usage, and recommendation feedback.
4. `skill.error` requires a structured `failure_category` and never persists raw error text.
5. `telemetry report` includes instrumentation status for each declared event family and labels uninstrumented fields as `not_instrumented`.
6. Usage/error/feedback report aggregates become `available` when matching events exist and `missing` when emitters exist but no events match.
7. `skill inspect --include-telemetry` shows per-skill invocation count, error count, last invoked timestamp, last error timestamp, and recommendation feedback summary.
8. `skill recommend` includes telemetry-based `score_inputs` and bounded rank adjustments when telemetry evidence exists.
9. `skill recommend` ranking is unchanged when telemetry is disabled or no matching telemetry exists.
10. Tests cover enabled writes, disabled no-write behavior, redaction, report statuses, inspect telemetry summary, and recommend score adjustment.

## Open Questions

1. Whether `skill feedback` should later accept a recommendation id once recommendations persist durable ids.
2. Whether command wrappers should call `skill used` directly or a future lower-level `telemetry emit` command.
3. Whether `skill.error` should later include a controlled error-code vocabulary shared with command envelopes.
