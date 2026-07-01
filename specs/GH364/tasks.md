# GH364 Tasks: Skill New Scaffolding

Product spec: `specs/GH364/product.md`

Tech spec: `specs/GH364/tech.md`

## Implementation Tasks

- [x] `SP364-T001` Owner: cli | Done when: CLI args for `skill new` live outside `src/cli.rs` and expose template, description, agent, and dry-run flags | Verify: `cargo check --workspace --all-targets --all-features`
- [x] `SP364-T002` Owner: cli | Done when: `SkillCommand::New` dispatches to `skill.new` with dry-run excluded from durable audit writes | Verify: `cargo test --test skill_new_cli`
- [x] `SP364-T003` Owner: command | Done when: `src/commands/skill_new.rs` stages generated files, strict-lints them, parses `loom.skill.toml`, renames atomically, commits, and queues sync when needed | Verify: `cargo test --test skill_new_cli`
- [x] `SP364-T004` Owner: command | Done when: generated skeleton includes `SKILL.md`, references, scripts, assets, eval stubs, and Loom-local manifest without nested YAML metadata | Verify: `cargo test --test skill_new_cli`
- [x] `SP364-T005` Owner: command | Done when: `--dry-run`, invalid names, and existing skills fail or preview without partial writes | Verify: `cargo test --test skill_new_cli`
- [x] `SP364-T006` Owner: docs | Done when: README, CLI contract, and GH364 spec packet document the command and non-goals | Verify: `git diff --check`
- [x] `SP364-T007` Owner: verification | Done when: focused and full Rust tests plus SpecRail packet validation pass | Verify: `cargo test`

## Verification

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test --test skill_new_cli
cargo test --test skill_lint
cargo test --test cli_surface
cargo test
```

Use `Fixes #364` only if every acceptance criterion is implemented and verified.
