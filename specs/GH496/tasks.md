# GH496 Tasks: Telemetry Emitters And Feedback Signals

Issue: https://github.com/majiayu000/loom/issues/496
Product spec: `specs/GH496/product.md`
Tech spec: `specs/GH496/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the first complete telemetry-emitter slice:

```text
skill used + skill feedback + report instrumentation + inspect/recommend consumers
```

Do not implement:

```text
hosted telemetry
real-agent eval runner
durable recommendation ids
Panel dashboard changes
raw prompt/output/error persistence
```

## Tasks

### SP496-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- existing or new `src/cli/*` args module
- `src/commands/mod.rs`
- `src/commands/helpers.rs` if read/write classification changes

Done when:

- `loom skill used <skill>` parses.
- `loom skill used <skill> --error --failure-category <category>` parses.
- `loom skill feedback <skill> --feedback accepted|rejected|ignored` parses.
- `--success` and `--error` conflict.
- `--error` without `--failure-category` fails before writing.
- command ids are stable.

Verify:

```bash
cargo test --test cli_surface
```

### SP496-T2: Add Telemetry Writer Helpers

Owner: implementation
Depends on: SP496-T1

Files:

- `src/commands/telemetry/mod.rs`
- `src/commands/telemetry/model.rs`
- `src/commands/telemetry/store.rs` only if helper return shape requires it

Done when:

- `record_skill_invocation_telemetry` writes `skill.invocation`.
- `record_skill_error_telemetry` writes `skill.error`.
- `record_recommendation_feedback_telemetry` writes `recommendation.feedback`.
- helpers return `Ok(None)` only for absent/disabled telemetry.
- enabled write failures propagate typed errors.
- redaction and validation are reused from existing telemetry storage.

Verify:

```bash
cargo test --test telemetry
```

### SP496-T3: Implement Hook Commands

Owner: implementation
Depends on: SP496-T1, SP496-T2

Files:

- existing skill command module or new `src/commands/skill_usage.rs`
- `tests/telemetry.rs`

Done when:

- absent telemetry returns `recorded=false` and does not create `state/telemetry`.
- enabled `skill used` records a redacted `skill.invocation` event.
- enabled `skill used --error --failure-category timeout` records `skill.error`.
- enabled `skill feedback --feedback accepted` records `recommendation.feedback`.
- raw task text, raw error text, env values, and file contents are not persisted.

Verify:

```bash
cargo test --test telemetry
```

### SP496-T4: Add Report Instrumentation Status

Owner: implementation
Depends on: SP496-T2

Files:

- `src/commands/telemetry/mod.rs`
- `tests/telemetry.rs`

Done when:

- `telemetry report` returns an `instrumentation` object for declared event families.
- `skill.invocation`, `skill.error`, and `recommendation.feedback` are marked `instrumented`.
- fields with emitters but no matching events report `status=missing`.
- fields without emitters report `status=not_instrumented`.
- numeric counts remain present and are not used as the sole truth signal.

Verify:

```bash
cargo test --test telemetry
```

### SP496-T5: Wire Inspect Telemetry Evidence

Owner: implementation
Depends on: SP496-T4

Files:

- `src/commands/skill_inspect.rs`
- `src/commands/skill_inspect/evidence.rs` if a split keeps the file smaller
- `tests/skill_inspect.rs`
- `tests/telemetry.rs`

Done when:

- `skill inspect --include-telemetry` includes per-skill invocation count.
- it includes error count and last error timestamp when error events exist.
- it includes recommendation feedback accepted/rejected/ignored summary.
- disabled or absent telemetry is explicit and does not look like zero usage.
- existing inspect fields remain stable.

Verify:

```bash
cargo test --test skill_inspect
cargo test --test telemetry
```

### SP496-T6: Add Recommendation Telemetry Evidence

Owner: implementation
Depends on: SP496-T4

Files:

- `src/commands/skill_recommend.rs`
- `src/commands/skill_recommend/evidence.rs`
- `tests/skill_recommend_cli.rs`
- `tests/skill_recommend_evidence.rs`

Done when:

- recommendation scoring reads telemetry through existing local event-log helpers.
- matching invocation evidence can add a bounded positive score input.
- matching error evidence can add a bounded risk/penalty score input.
- accepted feedback can add a bounded positive score input.
- rejected feedback can add a bounded negative score input.
- ignored feedback is represented without acting as rejection.
- disabled or absent telemetry leaves existing ranking unchanged.
- every telemetry effect appears in `score_inputs`.

Verify:

```bash
cargo test --test skill_recommend_cli
cargo test --test skill_recommend_evidence
```

### SP496-T7: Final Verification

Owner: implementation
Depends on: SP496-T1, SP496-T2, SP496-T3, SP496-T4, SP496-T5, SP496-T6

Done when:

- focused tests cover every acceptance criterion in `product.md`.
- formatting and compile checks pass.
- no implementation claims cover hosted telemetry, real-agent eval, or Panel UI changes.

Verify:

```bash
git diff --check
cargo test --test telemetry
cargo test --test skill_inspect
cargo test --test skill_recommend_cli
cargo test --test skill_recommend_evidence
cargo check --workspace --all-targets --all-features
```

## Handoff Notes

- Use `Fixes #496` only if all tasks in this packet are implemented in one PR.
- If implementation is split, use `Refs #496` until the final slice satisfies all acceptance criteria.
- Do not treat telemetry disabled/absent as usage zero in report, inspect, or recommend paths.
- Do not store raw prompt, raw output, raw error text, file contents, env values, or secrets in telemetry events.
