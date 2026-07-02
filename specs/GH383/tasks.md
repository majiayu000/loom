# GH383 Tasks: Assisted Skill Authoring

Issue: https://github.com/majiayu000/loom/issues/383
Product spec: `specs/GH383/product.md`
Tech spec: `specs/GH383/tech.md`
Status: Implemented

## Scope For First PR

Implement guarded patch-artifact authoring:

```text
mock provider + redacted prompt material + patch artifact store + apply-patch validation gates
```

Do not implement:

```text
automatic activation, unreviewed release, hidden context uploads, direct provider writes to source
```

## Tasks

- [x] `SP383-T001` Owner: provider | Done when: authoring provider abstraction has a deterministic mock provider and no hard dependency on a hosted model | Verify: `cargo test --test skill_authoring`
- [x] `SP383-T002` Owner: redaction | Done when: prompt material is explicit, size-bounded, and redacts secrets/env values/token-like strings before provider calls | Verify: `cargo test --test skill_authoring`
- [x] `SP383-T003` Owner: patch-store | Done when: draft/extract/rewrite/tune/generate commands create patch artifacts and do not mutate source by default | Verify: `cargo test --test skill_authoring`
- [x] `SP383-T004` Owner: eval-generation | Done when: tune-description and generate-evals can add positive/negative trigger cases and task fixture stubs as patch diffs | Verify: `cargo test --test skill_authoring && cargo test --test skill_eval`
- [x] `SP383-T005` Owner: apply-patch | Done when: apply-patch requires idempotency key, revalidates source ref/digest, stages patch, runs lint/scan/eval, and commits only after validation | Verify: `cargo test --test skill_authoring`
- [x] `SP383-T006` Owner: safety | Done when: high-risk generated scripts or new network/destructive behavior are blocked or marked review-required before apply | Verify: `cargo test --test skill_authoring && cargo test --test skill_policy`
- [x] `SP383-T007` Owner: regression | Done when: focused and full repository checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP383-T1: Add Authoring Provider Abstraction

Owner: backend

Files:

- new authoring command module
- provider abstraction module
- mock provider tests

Done when:

- Mock provider produces deterministic patch artifacts.
- Hosted/local providers are behind explicit config.
- Core behavior does not require network access.

Verify:

```bash
cargo test --test skill_authoring
```

### SP383-T2: Add Prompt Material Redaction

Owner: security
Depends on: SP383-T1

Done when:

- Prompt input sources are explicit.
- Private session content requires opt-in.
- Secrets, credentials, env values, and token-like strings are redacted.
- Prompt artifact reports included paths and byte counts.

Verify:

```bash
cargo test --test skill_authoring
```

### SP383-T3: Add Patch Artifact Commands

Owner: backend
Depends on: SP383-T1, SP383-T2

Done when:

- `skill draft`, `skill extract`, `skill rewrite`, `skill tune-description`,
  and `skill generate-evals` emit patch artifacts.
- Default behavior writes no source files.
- Artifacts include validation plan and risk notes.

Verify:

```bash
cargo test --test skill_authoring
```

### SP383-T4: Add Eval And Description Tuning

Owner: backend
Depends on: SP383-T3

Done when:

- Description tuning can patch frontmatter description.
- Positive and negative trigger cases are generated as JSONL diffs.
- Eval fixture generation stays reviewable.

Verify:

```bash
cargo test --test skill_eval
```

### SP383-T5: Add Apply-Patch Gate

Owner: backend
Depends on: SP383-T3

Done when:

- Apply requires idempotency key.
- Source ref and digest are revalidated.
- Patch applies in staging first.
- Lint, policy/safety, and eval gates run before source mutation.
- Successful apply commits/audits changes and returns recovery info.

Verify:

```bash
cargo test --test skill_authoring
```

### SP383-T6: Add Safety Regression Gates

Owner: security
Depends on: SP383-T5

Done when:

- Generated scripts are non-executable or review-required.
- New network/destructive behavior is flagged by policy scan.
- High-risk unreviewed changes block apply.

Verify:

```bash
cargo test --test skill_policy
```

### SP383-T7: Full Verification

Owner: testing
Depends on: SP383-T1, SP383-T2, SP383-T3, SP383-T4, SP383-T5, SP383-T6

Done when:

- Focused tests cover every acceptance criterion.
- Full check and test suites pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

This implementation covers deterministic mock provider generation, redacted
prompt material, reviewable patch artifacts, guarded staging validation,
idempotent `apply-patch`, source commit, and high-risk generated-script
blocking.

Use `Fixes #383` once focused and full verification are green.
