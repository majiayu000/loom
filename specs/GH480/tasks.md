# GH480 Tasks: Skill Inspect Quality And Safety Evidence

Issue: https://github.com/majiayu000/loom/issues/480
Product spec: `specs/GH480/product.md`
Tech spec: `specs/GH480/tech.md`
Status: Draft for review

## Order

Evidence schema -> eval reader -> safety/policy reader -> text rendering -> tests/docs.

## Tasks

- [ ] `SP480-T001` Owner: inspect model | Dependencies: none | Done when: `skill inspect` JSON has explicit quality/safety evidence status fields instead of ambiguous null placeholders | Verify: `cargo test skill_inspect`
- [ ] `SP480-T002` Owner: eval evidence | Dependencies: `SP480-T001` | Done when: inspect reads latest eval evidence and reports status, timestamp, precision, recall, and malformed evidence errors | Verify: `cargo test skill_eval && cargo test skill_inspect`
- [ ] `SP480-T003` Owner: safety evidence | Dependencies: `SP480-T001` | Done when: inspect computes read-only trust/policy/safety decision and distinguishes blocked, quarantined, policy-blocked, not-run, and unavailable | Verify: `cargo test skill_safety && cargo test skill_inspect`
- [ ] `SP480-T004` Owner: human output | Dependencies: `SP480-T002`, `SP480-T003` | Done when: non-JSON inspect output renders the same statuses without implying missing evidence is healthy | Verify: `cargo test skill_inspect`
- [ ] `SP480-T005` Owner: read-only guard | Dependencies: all implementation tasks | Done when: inspect tests prove no registry/events/skills/live target mutation | Verify: `cargo test skill_inspect`
- [ ] `SP480-T006` Owner: verification | Dependencies: all prior tasks | Done when: full Rust checks pass | Verify: `cargo check && cargo test`
