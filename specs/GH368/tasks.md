# GH368 Tasks: Codex Visibility Doctor And Active-View Reconcile

Issue: https://github.com/majiayu000/loom/issues/368
Product spec: `specs/GH368/product.md`
Tech spec: `specs/GH368/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the Codex visibility foundation in safe slices:

```text
visibility planner + skill diagnosis/visibility output + reconcile dry-run + safe apply + guarded fix-config
```

Do not implement:

```text
general adapter v2, non-Codex config repair, activation UX, marketplace/provider behavior
```

## Tasks

- [ ] `SP368-T001` Owner: cli | Done when: `skill diagnose/visibility --agent codex` and `codex reconcile` command surfaces parse with dry-run/apply/fix-config selectors | Verify: `cargo test --test cli_surface`
- [ ] `SP368-T002` Owner: config | Done when: Codex config parser detects `skills.config` path/name disables, malformed TOML, and safe patch candidates without ad hoc string deletion | Verify: `cargo test --test codex_visibility`
- [ ] `SP368-T003` Owner: planner | Done when: visibility planner joins registry rules, targets, projections, filesystem entries, symlink canonicalization, config disables, runtime entries, and external entries | Verify: `cargo test --test codex_visibility`
- [ ] `SP368-T004` Owner: dry-run | Done when: `codex reconcile --dry-run` returns all proposed actions and mutates no registry, target, source, pending queue, or config file | Verify: `cargo test --test codex_visibility`
- [ ] `SP368-T005` Owner: apply | Done when: `codex reconcile --apply` repairs/removes only safe Loom-owned symlink projections and never edits config without `--fix-config` | Verify: `cargo test --test codex_visibility`
- [ ] `SP368-T006` Owner: fix-config | Done when: `--apply --fix-config` atomically repairs only safe active-skill disables and reports `restart_required=true` | Verify: `cargo test --test codex_visibility`
- [ ] `SP368-T007` Owner: docs | Done when: CLI contract and specs document visibility proof, dry-run/apply boundaries, config repair policy, and restart requirements | Verify: `git diff --check`

### SP368-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/cli/codex_args.rs`
- maybe `src/cli/skill_visibility_args.rs`
- `src/commands/mod.rs`

Done when:

- `loom codex reconcile --dry-run` parses.
- `--apply`, `--fix-config`, `--binding`, `--target`, and `--allowlist` parse.
- Single-skill Codex visibility can be requested through `skill diagnose --agent codex` or `skill visibility`.
- `src/cli.rs` stays below the hard 800-line ceiling.

Verify:

```bash
cargo test --test cli_surface
```

### SP368-T2: Add Codex Config Parser And Patcher

Owner: implementation

Files:

- `Cargo.toml`
- `Cargo.lock`
- `src/codex_config.rs` or `src/commands/codex_config.rs`

Done when:

- Parser reads `[[skills.config]]` path/name entries.
- Disabled entries are matched by canonical path or exact name.
- Malformed TOML returns typed error.
- Patch writer preserves unrelated config and validates temp TOML before rename.

Verify:

```bash
cargo test --test codex_visibility
```

### SP368-T3: Build Visibility Planner

Owner: implementation

Files:

- `src/commands/codex_visibility.rs`

Done when:

- Planner classifies desired active set, recorded projections, target filesystem entries, runtime entries, external entries, config disables, and restart requirements.
- Multiple bindings sharing one target are handled as a union.
- Plan categories match the product spec.

Verify:

```bash
cargo test --test codex_visibility
```

### SP368-T4: Add Single-Skill Diagnosis Output

Owner: implementation
Depends on: SP368-T3

Files:

- `src/commands/skill_diagnose.rs`
- future `src/commands/skill_inspect.rs` integration when #366 lands

Done when:

- Codex checks explain active rule, target path, projection path, symlink target, config disables, runtime entries, external entries, and restart requirement.
- Missing data is explicit and not silently treated as success.

Verify:

```bash
cargo test --test codex_visibility
```

### SP368-T5: Implement Reconcile Dry-Run And Apply

Owner: implementation
Depends on: SP368-T3

Files:

- `src/commands/codex_cmds.rs`

Done when:

- Dry-run mutates nothing.
- Apply repairs missing safe symlink projections.
- Apply removes stale Loom-owned symlinks and stale records.
- Apply preserves runtime and external entries.
- Apply without `--fix-config` does not edit config.

Verify:

```bash
cargo test --test codex_visibility
```

### SP368-T6: Implement Fix-Config

Owner: implementation
Depends on: SP368-T2, SP368-T3, SP368-T5

Done when:

- `--apply --fix-config` patches only safe active-skill disables.
- Patched config is written atomically.
- Malformed or unsafe config entries block repair with manual review.
- Response includes `restart_required=true`.

Verify:

```bash
cargo test --test codex_visibility
```

### SP368-T7: Verification And Handoff

Owner: implementation

Done when:

- Focused Codex visibility tests pass.
- Full compile check passes.
- SpecRail packet validation passes.
- PR body uses `Refs #368` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test codex_visibility
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH368-codex-visibility-spec/specs/GH368
```

## Handoff Notes

- Use `Refs #368` for partial implementation slices.
- Never edit Codex config without `--apply --fix-config`.
- Never remove external or runtime entries.
- Never claim current-session Codex visibility without runtime proof; report restart/new-session requirement instead.
