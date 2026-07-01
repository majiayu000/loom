# GH389 Tasks: Instruction Surface Inventory

Issue: https://github.com/majiayu000/loom/issues/389
Product spec: `specs/GH389/product.md`
Tech spec: `specs/GH389/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement only the read-only instruction boundary foundation:

```text
instruction scan + show + classify + doctor + migrate-plan --dry-run
```

Do not implement in the first PR:

```text
silent edits to instruction files, automatic skill conversion, native precedence
ownership, private memory persistence, or arbitrary instruction files accepted
as skills
```

## Tasks

- [ ] `SP389-T1` Owner: implementation | Done when: instruction scan/show/classify/doctor/migrate-plan CLI parses and command ids classify dry-run commands as read-only | Verify: `cargo test --test cli_surface`
- [ ] `SP389-T2` Owner: implementation | Done when: adapter-driven discovery reports AGENTS.md, nested scope, and supported agent rule surfaces without mutation | Verify: `cargo test --test instruction_surfaces`
- [ ] `SP389-T3` Owner: implementation | Done when: classification marks scope, precedence, always-on status, skill-like workflow signals, suggestions, and unsupported surfaces explicitly | Verify: `cargo test --test instruction_surfaces`
- [ ] `SP389-T4` Owner: implementation | Done when: doctor reports duplicate/conflicting guidance between skills and always-on instructions without invented precedence claims | Verify: `cargo test --test instruction_surfaces`
- [ ] `SP389-T5` Owner: implementation | Done when: migrate-plan emits reviewable no-write plans for keep/reference/extract/review actions | Verify: `cargo test --test instruction_surfaces`
- [ ] `SP389-T6` Owner: implementation | Done when: skill lint boundary rejects non-SKILL.md instruction files and docs/specs cover migration rules | Verify: `cargo test --test skill_lint && git diff --check`

### SP389-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs` or a split instruction args module
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom instruction scan [--agent <agent>] [--workspace <path>] [--json]` parses.
- `loom instruction show <instruction-id> [--json]` parses.
- `loom instruction classify <path> [--json]` parses.
- `loom instruction doctor [--agent <agent>] [--workspace <path>] [--skill <skill>] [--json]` parses.
- `loom instruction migrate-plan <instruction-id> --to <target> --dry-run [--json]` parses.
- commands are read-only while apply is deferred.

Verify:

```bash
cargo test --test cli_surface
```

### SP389-T2: Implement Adapter-Driven Discovery

Owner: implementation
Depends on: SP389-T1

Done when:

- scan discovers root `AGENTS.md`.
- scan discovers nested `AGENTS.md` and reports nested scope.
- scan discovers at least one adapter-specific rule surface when metadata
  supports it.
- unsupported or unknown surfaces are reported explicitly when relevant.
- scan is bounded and read-only.
- project/user skill roots remain classified as skills, not instructions.

Verify:

```bash
cargo test --test instruction_surfaces
```

### SP389-T3: Implement Classification

Owner: implementation
Depends on: SP389-T2

Done when:

- records include instruction id, agent, kind, scope, path, applies-to,
  precedence, always-on status, skill-like workflow flag, suggested action, and
  warnings.
- classifiers detect trigger-like phrases, long procedures, script/tool
  references, repeated task blocks, and safety/policy instructions.
- unknown precedence remains unknown rather than fabricated.
- private memory or transcript content is not persisted.

Verify:

```bash
cargo test --test instruction_surfaces
```

### SP389-T4: Implement Instruction Doctor

Owner: implementation
Depends on: SP389-T2, SP389-T3

Done when:

- doctor compares relevant instructions with a selected skill.
- duplicate guidance is reported with source file and skill references.
- conflicting guidance is reported with scope and suggested action.
- prompt-budget and shadowing risks are reported where detectable.
- missing adapter metadata is explicit.

Verify:

```bash
cargo test --test instruction_surfaces
```

### SP389-T5: Implement Migration Plans

Owner: implementation
Depends on: SP389-T3

Done when:

- keep-instruction plans explain why no file changes are needed.
- move-to-reference plans propose target reference files and source edits.
- extract-skill plans propose draft `SKILL.md` and optional references split.
- review-conflict plans ask for human review when unsafe.
- dry-run writes no files.

Verify:

```bash
cargo test --test instruction_surfaces
```

### SP389-T6: Preserve Skill Boundary And Final Checks

Owner: implementation
Depends on: SP389-T1, SP389-T2, SP389-T3, SP389-T4, SP389-T5

Done when:

- portable lint still rejects `AGENTS.md`, `CLAUDE.md`, `.mdc`, and custom
  instruction files as skills.
- CLI and migration docs explain the instruction/skill boundary.
- tests cover first-slice acceptance criteria.
- repository checks pass.

Verify:

```bash
git diff --check
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

- Use `Refs #389` for a first-slice PR unless scan, show, classify, doctor,
  migrate-plan, lint boundary, and every acceptance criterion are implemented.
- Do not use `Fixes #389` until instruction scan/classify/doctor/migrate-plan
  and lint boundary tests are complete.
- Do not silently modify high-context instruction files.
