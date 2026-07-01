# GH366 Tasks: Single-Skill Inspect And Status Read Model

Issue: https://github.com/majiayu000/loom/issues/366
Product spec: `specs/GH366/product.md`
Tech spec: `specs/GH366/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the first complete `skill inspect` read-model slice:

```text
CLI + read-only status model + runtime projection classifier + JSON/human output + focused tests
```

Do not implement:

```text
skill activate/deactivate/list, Codex config repair, adapter v2 discovery roots, eval runner changes, safety scan/trust state
```

## Tasks

- [ ] `SP366-T001` Owner: cli | Done when: `loom skill inspect <skill> [--agent <agent>] [--workspace <path>] [--profile <id>]` parses and dispatches while `src/cli.rs` remains below 800 lines | Verify: `cargo test --test cli_surface`
- [ ] `SP366-T002` Owner: inspect-model | Done when: `SkillStatusModel` returns source, spec/lint, provenance-style, quality, safety, and next action fields without mutating registry/source/target state | Verify: `cargo test --test skill_inspect`
- [ ] `SP366-T003` Owner: runtime | Done when: runtime output separates active rule, projection, materialized path, generic projection findings, and unknown agent-specific visibility | Verify: `cargo test --test skill_inspect`
- [ ] `SP366-T004` Owner: output | Done when: JSON uses stable keys and human output prints a compact status card with no false visibility success | Verify: `cargo test --test skill_inspect`
- [ ] `SP366-T005` Owner: docs | Done when: CLI contract and specs document inspect behavior, deferred agent semantics, and verification commands | Verify: `git diff --check`

### SP366-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/cli/skill_inspect_args.rs`
- `src/commands/mod.rs`

Done when:

- `loom skill inspect <skill>` appears in help.
- `--agent`, `--workspace`, and `--profile` parse.
- Global JSON output mode works for inspect.
- `src/cli.rs` stays under the hard 800-line ceiling.

Verify:

```bash
cargo test --test cli_surface
```

### SP366-T2: Build Status Model

Owner: implementation

Files:

- `src/commands/skill_inspect.rs` or `src/commands/skill_status.rs`
- existing shared helper modules only when reuse is clean

Done when:

- Missing skill returns typed `SKILL_NOT_FOUND`.
- Existing source returns source path, entrypoint presence, lint/spec status, source drift fields when available, and default unknown quality/safety sections.
- The command is read-only; registry state, target files, and source files do not change.

Verify:

```bash
cargo test --test skill_inspect
```

### SP366-T3: Classify Runtime State

Owner: implementation

Files:

- `src/commands/skill_inspect.rs` or a small shared runtime status helper
- tests using synthetic registry snapshots and temp target paths

Done when:

- `installed_in_registry`, `active_rule_present`, `projected_to_target`, and `materialized_path_exists` are separate fields.
- Missing materialized path, missing source, broken symlink, target mismatch, and missing binding/target produce findings and next actions.
- `visible_to_agent`, `enabled_by_agent_config`, and `restart_required` are `unknown` unless the implementation has direct proof.
- `--agent` filters runtime sections without hiding source/spec.

Verify:

```bash
cargo test --test skill_inspect
```

### SP366-T4: Add Output And Next Actions

Owner: implementation

Files:

- `src/commands/skill_inspect.rs`
- maybe `docs/LOOM_CLI_CONTRACT.md`

Done when:

- JSON keys match the product spec.
- Human output is compact and derived from the same model.
- Next actions cover missing lint, missing projection, unknown visibility, missing eval evidence, and unknown safety.

Verify:

```bash
cargo test --test skill_inspect
git diff --check
```

### SP366-T5: Verification And Handoff

Owner: implementation

Done when:

- Focused tests pass.
- Full Rust compile check passes.
- SpecRail packet validation passes when the external checker is available.
- PR body uses `Refs #366` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test skill_inspect
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom/specs/GH366
```

## Handoff Notes

- Use `Refs #366` for a partial implementation PR until inspect returns the full read model and focused tests prove the acceptance criteria.
- Do not claim Codex config visibility, activation success, eval quality, or safety/trust readiness without the follow-up issues that own those signals.
- Keep `unknown` explicit where the current code lacks proof.
