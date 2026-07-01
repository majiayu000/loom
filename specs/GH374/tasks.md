# GH374 Tasks: Single-Skill Lifecycle Docs

Issue: https://github.com/majiayu000/loom/issues/374
Product spec: `specs/GH374/product.md`
Tech spec: `specs/GH374/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the documentation slice:

```text
README single-skill quick start + lifecycle docs + Codex visibility docs + dry-run migration guide
```

Do not implement:

```text
new CLI behavior, hidden Codex config repair, automatic full-mirror cleanup
```

## Tasks

- [ ] `SP374-T001` Owner: docs-readme | Done when: README introduces single-skill lifecycle before advanced registry/projection internals and labels unavailable commands honestly | Verify: `git diff --check`
- [ ] `SP374-T002` Owner: docs-lifecycle | Done when: `docs/SINGLE_SKILL_LIFECYCLE.md` defines the lifecycle terms and command sequence with implemented/planned status called out | Verify: `rg -n "active view|disabled-by-config|restart-required|--json" docs/SINGLE_SKILL_LIFECYCLE.md`
- [ ] `SP374-T003` Owner: docs-codex | Done when: `docs/CODEX_SKILL_VISIBILITY.md` explains `~/.agents/skills`, legacy `${CODEX_HOME:-~/.codex}/skills`, the CWD-to-repo-root `.agents/skills` search chain, symlink canonicalization, path-based config disables, and restart guidance | Verify: `rg -n "\\.agents/skills|CODEX_HOME|skills.config|restart|--json" docs/CODEX_SKILL_VISIBILITY.md`
- [ ] `SP374-T004` Owner: docs-migration | Done when: `docs/MIGRATING_TO_ACTIVE_VIEW.md` documents dry-run-first audit, allowlist, activation, reconcile, config disable repair/reporting, and verification | Verify: `rg -n "dry-run|allowlist|reconcile|doctor|--json" docs/MIGRATING_TO_ACTIVE_VIEW.md`
- [ ] `SP374-T005` Owner: docs-consistency | Done when: existing docs that imply Codex full-mirror behavior are corrected or labeled legacy and link to the new active-view docs | Verify: `rg -n "full[- ]mirror|legacy|active view" README.md docs`
- [ ] `SP374-T006` Owner: regression | Done when: documentation diffs are whitespace-clean and repository check/test still pass | Verify: `git diff --check && cargo check --workspace --all-targets --all-features && cargo test`

### SP374-T1: Update README Quick Start

Owner: documentation

Files:

- `README.md`

Done when:

- README shows a single-skill quick start before target, binding, and
  projection internals.
- Planned commands are not presented as currently shipped.
- Advanced projection examples remain available later for power users.
- README links to the new lifecycle, Codex visibility, and migration docs.

Verify:

```bash
git diff --check
```

### SP374-T2: Add Single-Skill Lifecycle Guide

Owner: documentation
Depends on: SP374-T1

Files:

- `docs/SINGLE_SKILL_LIFECYCLE.md`

Done when:

- The guide defines source, target, active view, active, installed, visible,
  enabled, disabled-by-config, and restart-required.
- The guide explains when to use `new`, `add`, `save`, `capture`, `lint`,
  `inspect`, `doctor`, `eval`, `release`, and `rollback`.
- Automation examples use `--json` where output is parsed.
- Upstream Agent Skills links are present.

Verify:

```bash
rg -n "source|target|active view|visible|disabled-by-config|restart-required|--json" docs/SINGLE_SKILL_LIFECYCLE.md
```

### SP374-T3: Add Codex Visibility Guide

Owner: documentation
Depends on: SP374-T2

Files:

- `docs/CODEX_SKILL_VISIBILITY.md`

Done when:

- The guide names the preferred user root, legacy user root, and project root.
- The guide explains why filesystem projection alone does not prove visibility.
- The guide explains canonical `SKILL.md` path disables and skill name disables.
- New-session or restart guidance is explicit.
- Planned reconcile examples are marked planned if not implemented.

Verify:

```bash
rg -n "\\.agents/skills|CODEX_HOME|\\.codex/skills|skills.config|canonical|restart|--json" docs/CODEX_SKILL_VISIBILITY.md
```

### SP374-T4: Add Active-View Migration Guide

Owner: documentation
Depends on: SP374-T3

Files:

- `docs/MIGRATING_TO_ACTIVE_VIEW.md`

Done when:

- Migration starts with read-only audit.
- The allowlist is explicit operator input.
- Apply steps are separated from dry-run steps.
- The guide says existing full-mirror projections are migration input, not
  desired active state.
- Verification and recovery steps are documented.

Verify:

```bash
rg -n "read-only|dry-run|allowlist|full-mirror|doctor|rollback|--json" docs/MIGRATING_TO_ACTIVE_VIEW.md
```

### SP374-T5: Correct Conflicting Legacy Docs

Owner: documentation
Depends on: SP374-T2, SP374-T3, SP374-T4

Files:

- `docs/LOOM_COMPLETE_GUIDE_ZH.md`
- `docs/LOOM_STATE_MIGRATION_NOTES.md`
- `docs/plan/codex-active-view-projection-spec.md`
- other docs found by search

Done when:

- Docs that imply Codex should mirror every registry skill are corrected or
  marked legacy.
- Historical design docs keep context but link to the new user-facing docs.
- No unrelated docs are rewritten.

Verify:

```bash
rg -n "full[- ]mirror|\\.codex/skills|active view|legacy" README.md docs
```

### SP374-T6: Verify Documentation Slice

Owner: testing
Depends on: SP374-T1, SP374-T2, SP374-T3, SP374-T4, SP374-T5

Done when:

- Documentation diff is whitespace-clean.
- Rust compile and full test suites pass from this session.
- No command availability claim exceeds the implementation state at submission
  time.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #374` for a spec-only or partial docs PR. Use `Fixes #374` only when
README, the three new docs, and conflicting legacy doc corrections are all
complete.
