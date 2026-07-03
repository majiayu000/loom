# GH480 Tech Spec - Skill inspect quality and safety evidence

Issue: https://github.com/majiayu000/loom/issues/480
Product spec: `specs/GH480/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`skill inspect` builds a lifecycle status card but emits placeholder quality fields and an unknown safety policy. Text output then renders these placeholders as no eval evidence and unknown policy.

## 2. Proposed Design

1. Add a read-only evidence loader for latest eval reports under the existing eval report location.
2. Normalize eval evidence into `quality.status`, `last_eval`, `trigger_precision`, `trigger_recall`, `summary`, and `evidence_error`.
3. Reuse safety/policy evaluation helpers to compute a read-only `safety.policy`.
4. Preserve inspect read-only behavior: no command event, no registry mutation, no live target writes.
5. Update text output to render evidence status directly.

## 3. Affected Areas

1. `src/commands/skill_inspect.rs`: JSON model and evidence loading.
2. `src/main.rs`: human inspect card rendering.
3. `src/commands/skill_eval_harness/report.rs` or `src/commands/skill_eval.rs`: shared summary reading if needed.
4. `src/commands/skill_safety.rs` and `src/commands/skill_policy.rs`: read-only safety/policy helpers.
5. `tests/skill_inspect.rs`

## 4. Data Contract

Suggested quality object:

1. `quality.status`: `not_run`, `passed`, `failed`, `stale`, `malformed`, `unavailable`
2. `quality.last_eval`
3. `quality.trigger_precision`
4. `quality.trigger_recall`
5. `quality.evidence_path`
6. `quality.evidence_error`

Suggested safety object:

1. `safety.trust`
2. `safety.policy`
3. `safety.decision`
4. `safety.finding_count`
5. `safety.evidence_error`

## 5. Verification Plan

1. `cargo test skill_inspect`
2. `cargo test skill_eval`
3. `cargo test skill_safety`
4. `cargo check`
5. `cargo test`

## 6. Rollback Plan

If evidence loading is unstable, keep the new schema but mark evidence as `unavailable` with errors. Do not revert to ambiguous null fields.

## 7. Product Mapping

1. Product invariant 1 maps to eval report reader.
2. Product invariant 2 maps to missing metric reason fields.
3. Product invariant 3 maps to safety/policy helper integration.
4. Product invariant 5 maps to read-only snapshot tests.
