# GH452 Tech Spec: CLI Concept Convergence And Command-Surface Budget

Issue: https://github.com/majiayu000/loom/issues/452
Product spec: `specs/GH452/product.md`
Status: Draft for review

## Current State (evidence at 9b920c9)

- `Command` enum: 29 variants (`src/cli.rs:131-258`); `SkillCommand`: 58
  leaves (`src/cli.rs:314-419`).
- Shared scorer: `score_and_filter_skills` defined at
  `src/commands/skill_inventory.rs:626`, called by search (`:94`), resolve
  (`:133`), recommend (`src/commands/skill_recommend.rs:133`).
- Projection cores: `skill_activation/mod.rs:60,87`;
  `use_cmds.rs:79-106` chains `cmd_target` -> `cmd_workspace_binding` ->
  `cmd_project`.
- `scan` ⊂ `policy`: `skill_policy.rs:67-75` vs `skill_safety.rs:75-88`.
- `show` ⊂ `inspect`: `skill_inventory.rs:68-82` vs `skill_inspect.rs:96-195`.
- `use` scope gap: `UseScope` has only `Project`
  (`src/commands/use_cmds.rs:230-234`); non-codex built-ins declare only
  `scope: "user"` discovery roots (`src/agent_adapters/metadata.rs:90-99`),
  so `target_path_for` falls back to `<root>/targets/project/<agent>/skills`
  (`use_cmds.rs:216-217`); `--target-root` force-appends `<agent>/skills`
  (`use_cmds.rs:202-203`).
- Error envelope: `CommandFailure { code, message, details }`
  (`src/commands/mod.rs:90-104`); `ErrorBody` (`src/envelope.rs:6-10`);
  `next_actions` exists only in success payloads (`use_cmds.rs:66,153`,
  `skill_new.rs:181`, `skill_deps.rs:37`, `skill_recommend.rs:114`).

## Design

### 1. `skill commit` (#454)

```text
loom skill commit <skill> [--message <msg>] [--from-projection | --from-source]
```

Direction resolution, in order:

1. Exactly one side dirty -> that side (projection drift -> capture semantics;
   registry working tree dirty -> save semantics).
2. Both dirty -> typed `COMMIT_DIRECTION_AMBIGUOUS` error whose
   `next_actions` offer both explicit flags.
3. Neither dirty -> noop per V1 spec 4.5 (stable `noop: true`).

`release` gains `--anchor` (mutually exclusive with a version argument) and
reuses today's snapshot implementation. `diagnose` gains
`--check <drift|all>`; `--check drift` preserves `verify`'s exit-code
semantics. `capture`, `save`, `snapshot`, `verify` are deleted from the CLI,
panel mutation route table, and contract docs in the same PR.

### 2. Read-surface convergence (#455)

- `skill search <query> [--for-task] [--active] [--explain]`: `--for-task`
  is today's resolve weighting; `--explain` returns recommend's ranking
  inputs. `resolve` and `recommend` are deleted; the scorer keeps one public
  entry point.
- `skill inspect <skill> [--brief]`: `--brief` returns today's `show`
  payload; `show` is deleted.
- `policy` calls the scan module and embeds its findings under a `scan` key;
  `scan` remains as the safety-only standalone view (decision: keep `scan`,
  it is the only no-policy-context safety surface; document the composition).
- Top-level `doctor` is deleted; `workspace doctor` survives (it names the
  scope it checks).

### 3. `use --scope user` (#456)

- `UseScope` gains `User`. Resolution: adapter discovery roots for the
  requested scope via the #373 metadata; explicit `--target-root` means
  exactly that directory (no suffixing - breaking flag semantics change,
  documented).
- Writing into an existing directory not yet registered as a managed target
  requires `--adopt`: registers (or upgrades observed -> managed) the target,
  records the ownership change in the audit journal, then projects. Without
  `--adopt`, fail with `TARGET_NOT_MANAGED` + next_actions.

### 4. `error.next_actions[]` (#457)

```json
"error": {
  "code": "BINDING_NOT_FOUND",
  "message": "binding 'X' not found",
  "details": {"binding_id": "X"},
  "next_actions": [
    {"cmd": "loom workspace binding list --json",
     "reason": "list existing bindings to find a valid binding_id"}
  ]
}
```

- Additive and optional; `CommandFailure` gains
  `next_actions: Vec<NextAction>` defaulting to empty, serialized only when
  non-empty so existing consumers are untouched.
- A single helper module owns the suggestions per error code so wording stays
  consistent across command sites.
- Human rendering prints `hint: try <cmd>` lines after the error message.

## Compatibility And Sequencing

1. #457 first (additive, zero risk), then #456, then #455, then #454
   (largest breaking surface change last, right before a minor release).
2. Every deletion updates: `src/cli.rs` + arg modules, the three matches in
   `commands/mod.rs` (until #458 lands), panel route table
   (`docs/LOOM_ARCHITECTURE_DECISIONS.md` section 4.1), `tests/cli_surface.rs`,
   `docs/LOOM_CLI_CONTRACT.md`, README tables, and
   `docs/LOOM_COMPLETE_GUIDE_ZH.md`.
3. The surface-budget assertion lands with #455: `tests/cli_surface.rs`
   records the expected leaf count and fails on silent growth.
