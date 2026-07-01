# GH367 Tasks: Single-Skill Activate, Deactivate, And Active List

Issue: https://github.com/majiayu000/loom/issues/367
Product spec: `specs/GH367/product.md`
Tech spec: `specs/GH367/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the first safe activation slice:

```text
activate/deactivate/active list + dry-run plans + managed-target symlink path + idempotent repair
```

Do not implement:

```text
Codex config repair, adapter v2 discovery roots, eval gating, trust/quarantine, unsafe copy/materialize deletion
```

## Tasks

- [ ] `SP367-T001` Owner: cli | Done when: `skill activate`, `skill deactivate`, and `skill active list` parse with agent/scope/workspace/profile/target/dry-run selectors while `src/cli.rs` remains below 800 lines | Verify: `cargo test --test cli_surface`
- [ ] `SP367-T002` Owner: planner | Done when: activate/deactivate dry-run returns deterministic action plans and performs no registry, source, target, pending queue, or config mutation | Verify: `cargo test --test skill_activation`
- [ ] `SP367-T003` Owner: activation | Done when: activate creates or reuses a managed target/binding/rule/projection, preserves policy/method gates, and is idempotent | Verify: `cargo test --test skill_activation`
- [ ] `SP367-T004` Owner: deactivation | Done when: deactivate removes the selected active rule and only safe Loom-owned symlink projections while copy/materialize removal fails closed | Verify: `cargo test --test skill_activation`
- [ ] `SP367-T005` Owner: active-list | Done when: active list reports desired active rules, actual projections, filesystem health, and missing/conflict states | Verify: `cargo test --test skill_activation`
- [ ] `SP367-T006` Owner: docs | Done when: CLI contract and specs document activation boundaries, deferred visibility/config behavior, and verification commands | Verify: `git diff --check`

### SP367-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/cli/skill_activation_args.rs`
- `src/commands/mod.rs`

Done when:

- `loom skill activate --help`, `loom skill deactivate --help`, and `loom skill active list --help` expose the expected selectors.
- `--dry-run` is available for mutating commands.
- CLI additions do not push `src/cli.rs` over the hard 800-line ceiling.

Verify:

```bash
cargo test --test cli_surface
```

### SP367-T2: Add Activation Planner

Owner: implementation

Files:

- `src/commands/skill_activation.rs`

Done when:

- Planner can resolve existing managed target/binding/projection state.
- Planner can describe create, reuse, repair, remove, and blocker actions.
- Dry-run returns the plan without changing files or registry state.
- Project scope requires `--workspace`.

Verify:

```bash
cargo test --test skill_activation
```

### SP367-T3: Implement Activate Apply

Owner: implementation
Depends on: SP367-T2

Done when:

- Existing source skill can be activated into a managed symlink target.
- Existing `skill project` policy, method, symlink probe, projection, rollback, op log, and autosync/queue semantics are reused or preserved.
- Re-running activate returns noop or repair-only behavior.
- Missing safe projection is repaired.

Verify:

```bash
cargo test --test skill_activation
```

### SP367-T4: Implement Deactivate Apply

Owner: implementation
Depends on: SP367-T2

Done when:

- Deactivate removes the selected active rule.
- Deactivate removes only a symlink projection that resolves to the canonical source.
- Registry skill source is preserved.
- Copy/materialize projection removal fails closed unless safe backup/capture semantics are included and tested.

Verify:

```bash
cargo test --test skill_activation
```

### SP367-T5: Implement Active List

Owner: implementation

Done when:

- Active list joins rules, bindings, targets, projections, and filesystem health.
- Statuses distinguish healthy, missing projection, target missing, source missing, orphaned/conflict, and visibility unknown.
- Agent/scope/profile/workspace selectors filter output deterministically.

Verify:

```bash
cargo test --test skill_activation
```

### SP367-T6: Verification And Handoff

Owner: implementation

Done when:

- Focused activation tests pass.
- Full compile check passes.
- SpecRail packet validation passes.
- PR body uses `Refs #367` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test skill_activation
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom/specs/GH367
```

## Handoff Notes

- Use `Refs #367` for partial implementation slices.
- Do not claim Codex runtime visibility or config enablement; #368 owns that proof.
- Do not delete copy/materialize target content without a reviewed recovery path.
- Activation must reuse existing managed projection safety gates rather than bypassing them.
