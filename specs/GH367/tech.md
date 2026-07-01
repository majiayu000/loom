# GH367 Tech Spec: Single-Skill Activate, Deactivate, And Active List

Issue: https://github.com/majiayu000/loom/issues/367
Product spec: `specs/GH367/product.md`
Status: Draft for implementation

## Design Summary

Build activation as a high-level planner/apply layer over existing projection primitives:

1. Add CLI args in a dedicated module to avoid growing `src/cli.rs` beyond the hard ceiling.
2. Add a focused command module for activation planning, apply, deactivation, and active list.
3. Reuse existing registry state files: targets, bindings, rules, projections, operations, observations.
4. Reuse `skill project` safety primitives where practical: skill existence, policy gate, managed target check, projection method validation, symlink probe, projection materialization, rollback, and operation logging.
5. Add explicit dry-run plan output before mutation.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new `src/cli/skill_activation_args.rs` |
| command dispatch | `src/commands/mod.rs` |
| implementation | new `src/commands/skill_activation.rs` |
| shared projection helpers | `src/commands/skill_cmds.rs`, `src/commands/projections.rs`, `src/commands/helpers.rs` if helper extraction is needed |
| tests | new `tests/skill_activation.rs`, extend `tests/cli_surface.rs` |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH367/*` |

## CLI Types

Add subcommands under `SkillCommand`:

```rust
Activate(SkillActivateArgs),
Deactivate(SkillDeactivateArgs),
Active { command: SkillActiveCommand },
```

Suggested args:

```rust
pub struct SkillActivateArgs {
    pub skill: String,
    #[arg(long)]
    pub agent: String,
    #[arg(long, default_value = "user")]
    pub scope: ActivationScope,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "symlink")]
    pub method: ProjectionMethod,
    #[arg(long)]
    pub dry_run: bool,
}
```

`Deactivate` uses the same selectors but no projection method. `Active list` uses agent/scope/workspace/profile filters and is read-only.

## Planning Model

Use a plan object for dry-run and apply:

```rust
struct ActivationPlan {
    skill: String,
    agent: String,
    scope: String,
    profile: String,
    target_id: Option<String>,
    binding_id: Option<String>,
    target_path: PathBuf,
    method: ProjectionMethod,
    safe_to_apply: bool,
    actions: Vec<ActivationAction>,
    warnings: Vec<String>,
    blockers: Vec<ActivationBlocker>,
}
```

Action ids:

```text
ensure_target
ensure_binding
ensure_active_rule
ensure_projection
repair_projection
remove_active_rule
remove_projection
manual_review
```

## Target And Binding Resolution

Resolution order:

1. If `--target` is provided, load that target and require matching `agent` plus `ownership=managed`.
2. Otherwise derive a default managed target path from the selected agent and scope:
   - Codex user: `$HOME/.agents/skills` preferred.
   - Codex project: `<workspace>/.agents/skills`.
   - Claude user/project: use the current built-in adapter default dirs when available.
3. If no safe default path exists, fail with typed error and next action to create a managed target.
4. Reuse an existing managed target for the same agent/path, or plan a target create action.
5. Use a deterministic binding id for activation so repeated activate calls find the same desired state.
6. Project scope requires `--workspace`; user scope may use a stable global matcher/profile.

This is a v1 resolver. #373 may replace hard-coded/default root resolution with adapter discovery roots.

## Activate Apply

Apply sequence:

1. Lock workspace and ensure write repo readiness.
2. Validate skill name and source existence.
3. Run compatible lint and existing policy gate.
4. Resolve or create managed target.
5. Resolve or create deterministic activation binding.
6. Validate projection method against target capabilities.
7. For symlink, run the existing filesystem symlink probe before removing any existing projection.
8. Upsert active `RegistryBindingRule`.
9. Create or repair projection using existing `project_skill_to_target` behavior.
10. Save registry rules/projections atomically with rollback.
11. Record `skill.activate` operation and projection observation.
12. Commit registry state and run existing autosync/queue behavior.

Do not call the public CLI recursively. Extract shared helpers from `cmd_project` if needed so activate and project share safety logic.

## Deactivate Apply

Apply sequence:

1. Lock workspace and ensure write repo readiness.
2. Resolve the selected active rule and projection.
3. Build a dry-run plan first.
4. Remove the selected active rule from `RegistryRulesFile`.
5. Remove the matching projection record only for the selected binding/target/skill.
6. Delete live filesystem projection only when:
   - method is `symlink`;
   - path is a symlink;
   - canonical symlink target resolves to the registry source skill directory.
7. For `copy` or `materialize`, fail closed unless this implementation adds safe backup/capture semantics and tests.
8. Record `skill.deactivate` operation and observation.
9. Commit registry state and run existing autosync/queue behavior.

Never delete the canonical source under `skills/<skill>`.

## Active List

Read-only active list should join:

1. rules matching selected agent/scope/profile/workspace;
2. bindings and targets for those rules;
3. projections for the selected skill/binding/target;
4. filesystem existence/health.

Statuses:

```text
healthy
missing_projection
disabled_by_config
needs_restart
conflict
external_entry
orphaned_projection
target_missing
target_not_managed
source_missing
```

Before #368, `disabled_by_config` can be returned only when already detectable by existing code; otherwise use `visibility_unknown`.

## Error Handling

Typed failures should cover:

1. missing skill;
2. unknown agent;
3. missing workspace for project scope;
4. missing or non-managed target;
5. target agent mismatch;
6. unsupported projection method;
7. symlink unsupported by filesystem;
8. unsafe deactivate for copy/materialize;
9. non-Loom or external target entry conflict;
10. rollback failure with recovery details.

Errors must not be downgraded to warnings when activation/deactivation would leave wrong active state.

## Test Plan

Focused tests:

1. `activate --dry-run` is read-only and returns target/binding/projection actions.
2. `activate` creates managed user-scope target/binding/rule/projection for a source skill.
3. repeated `activate` is noop or repair-only.
4. missing projection with existing rule is repaired.
5. observed/external target activation fails before filesystem mutation.
6. symlink activation probes before replacing an existing projection.
7. `deactivate --dry-run` is read-only.
8. symlink deactivate removes only the safe Loom-owned symlink and source remains.
9. copy/materialize deactivate fails closed.
10. active list reports healthy, missing projection, target missing, and source missing states.

Suggested commands:

```bash
git diff --check
cargo test --test skill_activation
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
```

For a spec-only PR, run:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom/specs/GH367
```

## Rollback

Rollback can remove:

1. activation CLI args and subcommands;
2. `skill_activation` command module;
3. focused tests and docs updates.

If activation has been used in a test registry, rollback state by running `skill deactivate` or removing the generated active rule/projection records in the test fixture only. Production rollback must preserve skill sources and non-Loom target entries.

## Risks

1. Accidentally deleting user-authored target content. Mitigation: symlink-only safe deletion unless backup/capture exists.
2. Duplicating `skill project` safety logic. Mitigation: extract and share low-level projection helpers.
3. Claiming visibility without config/reload proof. Mitigation: output unknown until #368/#373 provide evidence.
4. Creating unstable binding ids. Mitigation: deterministic activation binding ids from agent/scope/profile/workspace/target.
