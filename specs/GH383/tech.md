# GH383 Tech Spec: Assisted Skill Authoring

Issue: https://github.com/majiayu000/loom/issues/383
Product spec: `specs/GH383/product.md`
Status: Implemented

## Current State

`loom skill new` creates deterministic scaffolding and validates generated
source with strict lint before writing. `skill lint`, `skill eval`, and
`skill policy` already provide local quality and safety signals. Command audit
redaction exists and should be reused for prompt material.

GH383 should add patch generation and application around those gates rather
than letting a provider write files directly.

Relevant files:

- `src/commands/skill_new.rs`
- `src/commands/skill_lint.rs`
- `src/commands/skill_eval.rs`
- `src/commands/skill_policy.rs`
- `src/commands/event_store.rs`
- tests for skill new/lint/eval/policy

## Provider Abstraction

Recommended provider trait:

```rust
trait AuthoringProvider {
    fn generate_patch(&self, request: AuthoringRequest) -> Result<SkillPatchArtifact>;
}
```

Provider implementations:

- `mock`: deterministic fixture provider for tests.
- future hosted/local providers behind explicit config.

Core must not depend on one hosted provider. Network providers require explicit
opt-in and redacted prompt preview.

## Patch Store

Recommended state:

```text
state/patches/skillpatch_<id>.json
state/patches/skillpatch_<id>.patch
```

Patch JSON:

```json
{
  "schema_version": 1,
  "patch_id": "skillpatch_...",
  "skill": "fixflow",
  "goal": "improve trigger precision",
  "source_ref": "HEAD",
  "source_digest": "sha256:...",
  "provider": "mock",
  "files": [],
  "validation_plan": [],
  "risk_notes": [],
  "created_at": "2026-07-01T00:00:00Z"
}
```

Patch files are artifacts, not applied source.

## Prompt And Redaction

Prompt material must be assembled from explicitly selected sources:

- selected session file/id
- selected git diff range
- selected skill source files
- selected eval case files

Before provider calls:

- redact secrets, env values, URLs with credentials, and token-like strings
- summarize large files
- report included file paths and byte counts
- require explicit opt-in for raw private session content

## Apply-Patch

`skill apply-patch` must:

1. Validate patch id and idempotency key.
2. Load patch artifact.
3. Revalidate source ref and source digest.
4. Apply patch in staging.
5. Run lint, policy/safety scan, and eval preflight.
6. Stop on high-risk unreviewed changes.
7. Move staged files into source only after validation.
8. Commit/audit the source change.
9. Return recovery information.

## Tests

Focused tests:

1. draft command emits patch artifact and writes no source files.
2. extract from diff emits a deterministic patch.
3. rewrite emits validation plan and risk notes.
4. tune-description adds positive/negative trigger cases.
5. generate-evals writes fixture diffs only to the artifact.
6. apply-patch requires idempotency key.
7. source-ref/source-digest mismatch fails.
8. high-risk generated script is blocked until reviewed.
9. redaction removes secrets from prompt artifacts.
10. mock provider makes tests deterministic.

## Verification

```bash
git diff --check
cargo test --test skill_authoring
cargo test --test skill_lint
cargo test --test skill_eval
cargo test --test skill_policy
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

The implementation covers deterministic `mock` generation, explicit redacted
prompt material, `state/patches/skillpatch_*.json` plus `.patch` artifacts, and
a guarded `apply-patch` path. Apply requires an idempotency key, revalidates
source digest/ref, validates an isolated staging copy, runs strict lint,
policy/safety, and mock eval gates, materializes source only after gates pass,
commits the changed skill path, and records an idempotent replay result.

Use `Fixes #383` for the implementation PR once focused and full verification
are green.
