# GH366 Tech Spec: Single-Skill Inspect And Status Read Model

Issue: https://github.com/majiayu000/loom/issues/366
Product spec: `specs/GH366/product.md`
Status: Draft for implementation

## Design Summary

Implement a read-only `skill inspect` command by composing existing Loom state instead of inventing a second source of truth:

1. Add a small CLI args module for `SkillInspectArgs` so `src/cli.rs` stays below the 800-line ceiling.
2. Add a focused inspect command module that builds one `SkillStatusModel`.
3. Reuse existing inventory, lint, verify, registry snapshot, binding, target, and projection helpers.
4. Add a small runtime classifier that distinguishes source, active rule, projection, filesystem health, and unknown agent-specific visibility.
5. Return stable JSON and compact human output from the same model.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new `src/cli/skill_inspect_args.rs` |
| command dispatch | `src/commands/mod.rs` |
| inspect model | new `src/commands/skill_inspect.rs` or `src/commands/skill_status.rs` |
| shared helpers | reuse `src/commands/skill_inventory.rs`, `src/commands/skill_verify.rs`, `src/commands/skill_lint.rs`, `src/commands/skill_diagnose.rs` where practical |
| tests | new or extended `tests/skill_inspect.rs`, maybe `tests/cli_surface.rs` |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH366/*` |

## Data Sources

The inspect model should be assembled from read-only sources:

1. `AppContext::skill_path(skill)` for source path and entrypoint checks.
2. `build_skill_read_model` for registry/source inventory summary.
3. `RegistryStatePaths::maybe_load_snapshot` for bindings, rules, targets, projections, operations.
4. `lint_skill_source` for portable and agent compatibility status.
5. `head_tree_oid_for_path`, `last_commit_for_path`, and `last_saved_commit_for_skill` where available for source drift/provenance-style fields.
6. Existing projection drift logic from workspace doctor / skill diagnose where it can be shared without mutation.

No implementation should call write-command helpers, projection apply helpers, config patchers, or pending-op writers.

## CLI Types

Add:

```rust
#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillInspectArgs {
    pub skill: String,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
}
```

Notes:

1. Reuse the global JSON envelope switch instead of adding a second incompatible JSON flag if the app already has one.
2. Validate `skill` with existing `validate_skill_name`.
3. Validate `agent` only against known adapter ids when adapter registry is available; unknown agents should be a typed argument error or warning according to existing command conventions.
4. `workspace` and `profile` are selectors only.

## Status Model

Internal model can be Rust structs first, serialized to JSON at the boundary:

```rust
struct SkillStatusModel {
    skill: String,
    source: SourceStatus,
    spec: SpecStatus,
    provenance: ProvenanceStatus,
    runtime: BTreeMap<String, RuntimeStatus>,
    quality: QualityStatus,
    safety: SafetyStatus,
    next_actions: Vec<String>,
}
```

Runtime status fields:

```rust
struct RuntimeStatus {
    installed_in_registry: bool,
    active_rule_present: bool,
    projected_to_target: bool,
    materialized_path_exists: Option<bool>,
    visible_to_agent: VisibilityValue,
    enabled_by_agent_config: VisibilityValue,
    restart_required: VisibilityValue,
    target_id: Option<String>,
    binding_id: Option<String>,
    target_path: Option<String>,
    materialized_path: Option<String>,
    health: Option<String>,
    truth_level: String,
    findings: Vec<StatusFinding>,
}
```

Use explicit tri-state values for agent-specific fields:

```text
true | false | unknown | not_checked
```

## Runtime Classification

For each relevant agent:

1. Start with known adapters and registry targets.
2. Mark `installed_in_registry=true` when the source exists in the registry or the inventory read model includes the skill.
3. Mark `active_rule_present=true` when a registry rule references the skill for the selected agent/workspace/profile.
4. Mark `projected_to_target=true` when a projection instance references the skill for the selected target.
5. Check `materialized_path_exists` from the projection path.
6. Detect generic projection problems:
   - materialized path missing;
   - source path missing;
   - symlink target missing;
   - target id missing;
   - target agent mismatch;
   - binding target missing.
7. Leave `visible_to_agent`, `enabled_by_agent_config`, and `restart_required` as `unknown` when they require #368/#373 agent semantics.

Do not infer visibility from `projected_to_target=true`.

## Next Actions

Generate deterministic next actions from the model:

1. Missing source: `loom skill add ...` cannot be guessed, so say restore or import the skill.
2. Lint failure: `loom skill lint <skill> --portable`.
3. Missing projection or active rule: after #367, prefer `loom skill activate <skill> --agent <agent>`; before #367 exists, use current supported `target` / `workspace binding` / `skill project` path if the implementation cannot call an activate command yet.
4. Unknown Codex visibility: `loom skill doctor <skill> --agent codex` or the #368 command when present.
5. Missing eval evidence: `loom skill eval <skill> --agent <agent> --baseline no-skill`.
6. Unknown safety: `loom skill policy <skill>` until #370 adds scan/trust state.

## Human Output

Human output should be rendered from `SkillStatusModel`, not by re-running checks. Keep it short:

1. one title line with the skill id;
2. `Source`, `Spec`, `Runtime`, `Quality`, `Safety`, `Next`;
3. no raw JSON unless global JSON mode is enabled;
4. no hidden success claim for unknown visibility.

## Test Plan

Focused tests:

1. CLI help exposes `loom skill inspect`.
2. Missing skill returns `SKILL_NOT_FOUND`.
3. Existing source with no registry snapshot still returns source/spec status and registry warning.
4. Registry skill with no projections returns `installed_in_registry=true`, `active_rule_present=false`, `projected_to_target=false`.
5. Skill with rule and projection separates rule, projection, path existence, and target metadata.
6. Missing projection path produces an error finding and next action.
7. Broken symlink is detectable and does not report visible.
8. `--agent codex` filters runtime sections but preserves source/spec.
9. Command is read-only by comparing registry files, target files, and source files before/after.

Suggested commands:

```bash
git diff --check
cargo test --test skill_inspect
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
```

If only specs are changed, run:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom/specs/GH366
```

## Rollback

Rollback can remove:

1. the `SkillInspectArgs` CLI module and enum variant;
2. the inspect/status command module;
3. focused tests and docs updates.

No registry migration or cleanup is required because the command is read-only.

## Risks

1. Duplicating `skill_diagnose` and `workspace doctor` logic could create drift. Mitigation: share projection classification helpers or keep inspect as an aggregator over existing helpers.
2. Reporting `visible=true` too early would be silent degradation. Mitigation: use `unknown` until #368/#373 provide agent-specific proof.
3. `src/cli.rs` is already close to 800 lines. Mitigation: move new args into `src/cli/skill_inspect_args.rs` and avoid adding large inline structs.
4. Adding status logic to already-large files could violate file-size rules. Mitigation: create a focused module and keep helpers small.
