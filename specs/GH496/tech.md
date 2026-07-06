# GH496 Tech Spec: Telemetry Emitters And Feedback Signals

Issue: https://github.com/majiayu000/loom/issues/496
Product spec: `specs/GH496/product.md`
Status: Draft for implementation

## Design Summary

Extend the GH385 telemetry subsystem by adding explicit writer surfaces and
consumer evidence for event families that already exist in the schema:

1. add typed writer helpers for `skill.invocation`, `skill.error`, and
   `recommendation.feedback`;
2. expose explicit CLI hook commands for agent wrappers and recommendation
   feedback;
3. add instrumentation metadata to `telemetry report` so unconnected fields are
   not rendered as bare zeros;
4. expose per-skill usage/error/feedback summaries through
   `skill inspect --include-telemetry`;
5. feed bounded telemetry evidence into deterministic recommendation scoring.

Do not change telemetry storage from local JSONL or weaken redaction.

## Dependencies And Blocks

| Issue | Required capability |
|---|---|
| #385 | local telemetry config, event model, report aggregation, export, purge |
| #366 | `skill inspect` evidence surface |
| #378 | recommendation/resolve scoring surface |
| #495 | not required; real-agent eval runner remains separate |

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new or existing `src/cli/*` args module |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` if command classification changes |
| telemetry implementation | `src/commands/telemetry/model.rs`, `src/commands/telemetry/mod.rs`, `src/commands/telemetry/store.rs` |
| skill hook commands | existing skill command module or new `src/commands/skill_usage.rs` |
| recommendation evidence | `src/commands/skill_recommend.rs`, `src/commands/skill_recommend/evidence.rs` |
| inspect evidence | `src/commands/skill_inspect.rs`, `src/commands/skill_inspect/evidence.rs` if helper split is useful |
| tests | `tests/telemetry.rs`, `tests/skill_recommend_cli.rs`, `tests/skill_recommend_evidence.rs`, `tests/skill_inspect.rs` |
| docs/specs | `specs/GH496/*`, maybe `docs/LOOM_CLI_CONTRACT.md` if CLI contract docs are updated |

## Event Model

Reuse the existing `TelemetryEvent` shape and `TelemetryMetrics` fields:

```text
event_type: skill.invocation | skill.error | recommendation.feedback
skill_id
agent
workspace_hash
session_id_hash
metrics.tokens_in
metrics.tokens_out
metrics.commands
metrics.duration_ms
metrics.success
metrics.failure_category
metrics.feedback
privacy
```

Rules:

1. Do not add raw prompt, raw error, raw output, file path, or command text fields.
2. `skill.invocation` writes `metrics.success=true`.
3. `skill.error` writes `metrics.success=false` and requires `metrics.failure_category`.
4. `recommendation.feedback` writes `metrics.feedback`.
5. Optional `--task` input must not be serialized as raw text. If correlation is needed, add a redacted/hash-only field in a follow-up; do not smuggle it into existing text fields.
6. Agent id validation follows the existing telemetry writer rule: lowercase ASCII alphanumeric plus `-` or `_`.

## Writer Helpers

Add typed helpers near existing telemetry helpers:

```rust
pub(crate) struct SkillInvocationTelemetry<'a> {
    pub(crate) agent: Option<&'a str>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) tokens_in: Option<u64>,
    pub(crate) tokens_out: Option<u64>,
    pub(crate) commands: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
}

pub(crate) fn record_skill_invocation_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: SkillInvocationTelemetry<'_>,
) -> Result<Option<TelemetryEvent>, CommandFailure>;

pub(crate) fn record_skill_error_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: SkillErrorTelemetry<'_>,
) -> Result<Option<TelemetryEvent>, CommandFailure>;

pub(crate) fn record_recommendation_feedback_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: RecommendationFeedbackTelemetry<'_>,
) -> Result<Option<TelemetryEvent>, CommandFailure>;
```

The helpers should return `Ok(None)` only when telemetry is absent or disabled.
They must propagate validation, redaction, lock, and append failures.

## CLI Commands

Add:

```bash
loom skill used <skill> [options]
loom skill feedback <skill> --feedback accepted|rejected|ignored [options]
```

Implementation rules:

1. Validate skill id with `validate_skill_name`.
2. Validate that the skill exists in the current read model unless the command
   is explicitly documented as wrapper-only for deleted skills. The first slice
   should require existence.
3. `--success` and `--error` conflict; default is `--success`.
4. `--error` requires `--failure-category`.
5. Numeric metrics reject negative values through typed CLI parsing.
6. When telemetry is disabled or absent, return success with `recorded=false`
   and `reason=telemetry_disabled`; do not create telemetry state.
7. When telemetry is enabled and writing fails, return a typed error.

## Instrumentation Metadata

Add a small static registry for declared event emitters:

```text
skill.activation -> skill.activate, skill.deactivate, compiled activation
skill.deactivation -> skill.deactivate
skill.invocation -> skill.used
skill.eval -> skill.eval, skill eval run/trigger/compare
skill.safety -> skill.scan / safety evaluation path
skill.error -> skill.used --error
recommendation.feedback -> skill.feedback
```

`telemetry report` should include:

```json
{
  "instrumentation": {
    "skill.invocation": {
      "status": "instrumented",
      "emitters": ["skill.used"]
    }
  }
}
```

Aggregate block statuses should use this metadata:

1. if the event family has no emitter: `not_instrumented`;
2. if it has emitters but no matching events: `missing`;
3. if matching events exist: `available`.

This must not hide counts. Counts remain numeric, but status carries the truth
about whether zero means no data or no writer.

## Inspect Evidence

`skill_telemetry_summary` should expose enough per-skill data for inspect:

```json
{
  "usage": {
    "invocations": 3,
    "errors": 1,
    "last_invoked_at": "2026-07-06T00:00:00Z",
    "last_error_at": "2026-07-06T00:05:00Z",
    "status": "available"
  },
  "recommendation_feedback": {
    "accepted": 1,
    "rejected": 0,
    "ignored": 2,
    "status": "available"
  }
}
```

Rules:

1. Preserve existing fields used by current telemetry tests where possible.
2. Add fields rather than replacing the whole telemetry object.
3. `--include-telemetry` remains the opt-in inspect flag for this slice.
4. Disabled telemetry should produce an explicit disabled/missing state, not an
   invented empty success signal.

## Recommendation Evidence

Extend `RankingEvidence` with telemetry-derived inputs:

1. read the same telemetry event log through existing helpers;
2. filter by skill id and requested agent when provided;
3. optionally use workspace filter when `skill recommend` has a workspace;
4. compute bounded evidence:
   - successful invocations: small positive boost, capped;
   - recent errors or high error ratio: negative risk, capped;
   - accepted feedback: positive boost, capped;
   - rejected feedback: negative boost, capped;
   - ignored feedback: include as neutral evidence;
5. append every effect to `score_inputs` with stable fields such as
   `telemetry_usage`, `telemetry_error_rate`, and `recommendation_feedback`.

No telemetry data must leave the existing lexical/eval/dependency ranking
unchanged.

## Test Plan

Focused tests:

1. `skill used` with absent telemetry returns `recorded=false` and does not
   create `state/telemetry`.
2. enabled telemetry records `skill.invocation` with redacted workspace/session
   data and optional metrics.
3. `skill used --error --failure-category timeout` records `skill.error` and
   rejects missing failure category.
4. enabled telemetry records `recommendation.feedback` from `skill feedback`.
5. report instrumentation marks the three new event families as
   `instrumented`.
6. report status is `missing` when emitters exist but no matching events exist.
7. report status is `available` when matching invocation/error/feedback events
   exist.
8. `skill inspect --include-telemetry` includes invocation/error/feedback
   summary.
9. `skill recommend` ranking changes with accepted feedback and successful
   usage evidence.
10. `skill recommend` ranking is unchanged when telemetry is disabled or absent.
11. redaction tests prove raw task/error text is not persisted.

Suggested commands:

```bash
git diff --check
cargo test --test telemetry
cargo test --test skill_inspect
cargo test --test skill_recommend_cli
cargo test --test skill_recommend_evidence
cargo check --workspace --all-targets --all-features
```

## Rollback

The implementation should be isolated to command args/dispatch, telemetry
writer helpers, report aggregation, inspect/recommend evidence, and focused
tests. Rollback can remove the new CLI hook commands and helper calls without
changing existing telemetry config, export, purge, activation, eval, safety, or
registry state formats.

## Risks

1. Usage hooks may be called too often and inflate ranking. Mitigation: bounded
   weights and score inputs.
2. Error telemetry can leak private text. Mitigation: require controlled
   `failure_category` and reject raw error fields.
3. Disabled telemetry can be mistaken for zero usage. Mitigation:
   `recorded=false`, `missing`, and `not_instrumented` statuses.
4. Recommendation feedback can be over-interpreted. Mitigation: only explicit
   feedback records accepted/rejected/ignored; search results alone do not emit
   feedback.
