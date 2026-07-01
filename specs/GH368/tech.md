# GH368 Tech Spec: Codex Visibility Doctor And Active-View Reconcile

Issue: https://github.com/majiayu000/loom/issues/368
Product spec: `specs/GH368/product.md`
Status: Draft for implementation

## Design Summary

Add a Codex-specific visibility planner and reconcile command:

1. Parse Codex `config.toml` into a minimal view of `[[skills.config]]` disable entries.
2. Join registry rules, targets, projections, target filesystem entries, and config disables.
3. Produce a read-only visibility result for one skill.
4. Produce a dry-run reconcile plan for one Codex target/binding.
5. Apply only actions already present in the plan.
6. Preserve runtime and external entries.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new `src/cli/codex_args.rs`, maybe `src/cli/skill_visibility_args.rs` |
| command dispatch | `src/commands/mod.rs` |
| Codex commands | new `src/commands/codex_cmds.rs` |
| visibility planner | new `src/commands/codex_visibility.rs` |
| config parser/patcher | new `src/codex_config.rs` or `src/commands/codex_config.rs` |
| diagnosis integration | `src/commands/skill_diagnose.rs`, future `skill_inspect` integration from #366 |
| docs/tests | `docs/LOOM_CLI_CONTRACT.md`, new `tests/codex_visibility.rs`, maybe `tests/cli_surface.rs` |
| dependencies | `Cargo.toml`, `Cargo.lock` if a TOML editing crate is added |

## Dependency Choice

Prefer a structured TOML parser/editor instead of string manipulation. If the current dependency set has no TOML support, add a focused dependency such as `toml_edit` so config repair can preserve unrelated user config and validate the patched file.

Do not implement config repair with ad hoc line deletion.

## Codex Config View

Minimal internal model:

```rust
struct CodexConfigView {
    path: PathBuf,
    entries: Vec<CodexSkillConfigEntry>,
    parse_warnings: Vec<String>,
}

struct CodexSkillConfigEntry {
    index: usize,
    path: Option<PathBuf>,
    name: Option<String>,
    enabled: Option<bool>,
    source_kind: ConfigEntrySourceKind,
    raw_span: Option<ConfigSpan>,
}
```

Required matching:

1. canonical `SKILL.md` path match;
2. exact skill name match;
3. disabled when `enabled=false`;
4. no match on substring or non-canonical path guesses.

## Planner Inputs

```rust
struct CodexVisibilityRequest {
    skill: Option<String>,
    binding_id: Option<String>,
    target_id: Option<String>,
    workspace: Option<PathBuf>,
    allowlist_path: Option<PathBuf>,
    apply: bool,
    fix_config: bool,
}
```

Inputs:

1. `RegistrySnapshot`;
2. selected Codex target(s);
3. selected binding or all bindings sharing a target;
4. source skill directory;
5. target directory entries;
6. Codex config view;
7. optional allowlist for migration cleanup.

## Reconcile Algorithm

For a selected Codex target:

1. Load all active registry rules for bindings that share the target.
2. Build the desired active set as a union of those rules.
3. Load recorded projection instances for the target.
4. Scan target directory entries.
5. Classify each filesystem entry:
   - Loom-owned projection;
   - Codex runtime entry;
   - external entry;
   - broken/stale symlink;
   - unknown conflict.
6. Compare desired, recorded, and filesystem state.
7. Add actions:
   - create/repair missing desired symlink projection;
   - remove stale Loom-owned symlink when no desired rule remains;
   - remove stale projection records only when filesystem ownership is clear;
   - preserve runtime/external entries;
   - fix config disables only when `--fix-config` is allowed;
   - mark legacy full-mirror rules for manual review or allowlist removal.
8. Set `safe_to_apply=false` when any action needs manual review.

## Apply Rules

`--apply` must:

1. Rebuild the plan immediately before applying.
2. Refuse to apply if the current plan differs from an optional plan id/hash provided by a future flow; for v1, at least return the plan snapshot in the response.
3. Apply only `safe=true` actions.
4. Use existing projection symlink safety checks.
5. Commit registry changes through existing registry commit/autosync/queue flow.
6. Record `codex.reconcile` operation with action ids and affected skill ids.

`--apply --fix-config` additionally:

1. Reads and parses current config.
2. Patches only safe disabled entries.
3. Writes temp config next to the original.
4. Parses the temp config.
5. Atomically renames it over the original.
6. Sets `restart_required=true`.

## Runtime Entry Policy

Preserve entries with names known to be Codex-owned runtime/system entries:

```text
.system
codex-primary-runtime
```

The implementation may keep the list small and explicit. Unknown non-Loom entries should be `preserve_external_entry`, not deleted.

## Diagnosis Integration

Extend `skill diagnose` when `--agent codex` or related Codex state exists:

1. `codex_active_rule_exists:<binding_id>`
2. `codex_projection_path_exists:<instance_id>`
3. `codex_projection_is_symlink:<instance_id>`
4. `codex_projection_points_to_source:<instance_id>`
5. `codex_config_not_disabled_by_path:<skill>`
6. `codex_config_not_disabled_by_name:<skill>`
7. `codex_runtime_entry_classification:<path>`
8. `codex_restart_required`

Checks should include `next_action` and details with config path, target path, canonical source path, and projection path when relevant.

## Error Handling

Typed failures:

1. missing registry snapshot;
2. no Codex target selected;
3. target not managed when apply would mutate;
4. target agent mismatch;
5. target path unreadable;
6. malformed Codex config;
7. unsafe config entry;
8. unsafe filesystem entry;
9. symlink canonicalization failure;
10. rollback failure.

Do not downgrade malformed config or unsafe entries to warnings when apply/fix-config was requested.

## Test Plan

Focused tests:

1. visibility reports active rule, projection exists, and config not disabled.
2. path disable blocks visibility.
3. name disable blocks visibility.
4. broken symlink reports error and repair action.
5. target entry under `.system` is preserved.
6. external entry is preserved.
7. dry-run reconcile mutates nothing.
8. apply repairs missing Loom-owned symlink projection.
9. apply removes stale Loom-owned symlink and stale record.
10. apply without `--fix-config` does not edit config.
11. apply with `--fix-config` patches only safe entries and writes atomically.
12. malformed TOML blocks config repair.
13. multiple bindings sharing one target preserve the union desired set.
14. legacy rule cleanup requires allowlist or manual review.

Suggested commands:

```bash
git diff --check
cargo test --test codex_visibility
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
```

For a spec-only PR:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH368-codex-visibility-spec/specs/GH368
```

## Rollback

Rollback can remove:

1. Codex CLI args and command module;
2. visibility planner;
3. Codex config parser/patcher;
4. diagnosis integration;
5. focused tests and docs updates.

Runtime rollback after a bad apply must preserve external entries and skill sources. Projection repairs are limited to Loom-owned symlinks so rollback can restore registry state and recreate/remove symlinks using recorded action ids.

## Risks

1. Config repair could remove user-authored disables. Mitigation: safe-entry proof, manual_review fallback, structured TOML edit.
2. Reconcile could delete external skill entries. Mitigation: delete only Loom-owned symlinks resolving to registry source.
3. Shared Codex target could lose another binding's active skill. Mitigation: desired set is union of all active rules for the target.
4. Visibility claim could be overstated. Mitigation: no `visible=true` unless source, projection, symlink, config, and target checks all pass; still report restart recommendation when needed.
