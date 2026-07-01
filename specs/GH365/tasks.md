# GH365 Tasks: Expanded Skill Lint

Issue: https://github.com/majiayu000/loom/issues/365
Product spec: `specs/GH365/product.md`
Tech spec: `specs/GH365/tech.md`
Status: Draft for implementation

## Scope

Complete the expanded lint behavior:

```text
portable YAML parser + --portable + --agent + --quality report sections + collision/reference checks
```

Do not implement:

```text
remote spec fetching, automatic fixes, mutating fix-apply mode
```

## Tasks

- [x] `SP365-T001` Owner: cli | Done when: `--portable`, `--agent <agent>`, and `--quality` parse while existing lint flags remain valid | Verify: `cargo test --test skill_lint`
- [x] `SP365-T002` Owner: lint | Done when: frontmatter parsing uses `yaml_serde` and rich Agent Skills fields no longer fail only because nested YAML exists | Verify: `cargo test --test skill_lint`
- [x] `SP365-T003` Owner: lint | Done when: report includes portable, agent, quality, resources, and progressive disclosure sections | Verify: `cargo test --test skill_lint`
- [x] `SP365-T004` Owner: docs | Done when: CLI contract and GH365 specs document the first lint expansion slice | Verify: `git diff --check`
- [x] `SP365-T005` Owner: lint | Done when: lint rejects overlong descriptions, `--agent` reports configured active skill name collisions, `--agent claude` accepts Claude fields, and `--quality` reports vague/oversized/deep-reference risks | Verify: `cargo test --test skill_lint`

### SP365-T1: Add CLI Flags

Owner: implementation

Files:

- `src/cli.rs`
- `src/cli/skill_lint_args.rs`

Done when:

- `--portable`, `--agent <agent>`, and `--quality` parse successfully.
- Existing `--strict`, `--compat`, and `--fix` behavior remains available.
- `src/cli.rs` stays below the hard 800-line ceiling.

Verify:

```bash
cargo test --test skill_lint
```

### SP365-T2: Replace Frontmatter Parser

Owner: implementation

Files:

- `Cargo.toml`
- `Cargo.lock`
- `src/commands/skill_lint.rs`
- `src/commands/skill_lint/frontmatter.rs`

Done when:

- Nested YAML frontmatter parses through `yaml_serde`.
- Rich fields no longer fail only because nested YAML exists.
- Existing strict validation still fails malformed YAML and missing required fields.

Verify:

```bash
cargo test --test skill_lint
```

### SP365-T3: Add Agent And Quality Sections

Owner: implementation

Files:

- `src/commands/skill_lint.rs`
- `src/commands/skill_lint/sections.rs`

Done when:

- Report includes stable `sections`.
- `--agent codex` warns on Claude-only fields.
- `--agent claude` recognizes Claude-only fields without warning.
- `--agent codex` and `--agent claude` warn on configured active skill name collisions.
- `--quality` emits non-fatal warnings for eval/script/size/description/reference-depth quality.

Verify:

```bash
cargo test --test skill_lint
```

### SP365-T4: Update Contract And Tests

Owner: implementation

Files:

- `docs/LOOM_CLI_CONTRACT.md`
- `tests/skill_lint.rs`
- `specs/GH365/*`

Done when:

- Contract documents the new flags and report fields.
- Tests cover rich YAML, agent checks, active collision checks, quality checks, and existing strict failures.

### SP365-T5: Complete Remaining Lint Checks

Owner: implementation
Depends on: SP365-T3

Files:

- `src/commands/skill_lint.rs`
- `src/commands/skill_lint/sections.rs`
- `tests/skill_lint.rs`

Done when:

- Descriptions above 1024 characters fail strict portable lint.
- `--agent claude` accepts Claude-only fields without warning.
- Configured Codex/Claude skill directories are scanned for same-name active copies and reported as warnings.
- `--quality` reports vague descriptions, oversized `SKILL.md`, missing eval fixtures, unclear scripts, and deeply nested references.

Verify:

```bash
cargo test --test skill_lint
```

Verify:

```bash
git diff --check
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
```

## Handoff Notes

Use `Fixes #365` once this packet and implementation are verified.
