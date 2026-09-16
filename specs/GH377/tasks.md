# GH377 Tasks: Skillsets And Bundles

Issue: https://github.com/majiayu000/loom/issues/377
Product spec: `specs/GH377/product.md`
Tech spec: `specs/GH377/tech.md`
Status: Implementation update for lifecycle completion PR

## Scope For This PR

Implemented:

```text
skillset create/add/remove/show/lint
skillset activate/deactivate
skillset eval member aggregation
skillset release/rollback definition versioning
partial activation failure rollback/recovery reporting
```

September 2026 follow-up: `skillset eval --runner mock|codex-cli` runs bundle fixtures through the existing eval harness and compares against no skill or each member independently. `--dry-run` previews the selected comparison. A real Codex bundle/no-skill smoke comparison passed; see [verification](../../docs/plan/codex-cli-verification.md). Broader model quality requires additional evidence; deterministic tests do not prove it.

## Tasks

- [x] `SP377-T001` Owner: cli-surface | Done when: `skillset create/add/remove/show/lint/activate/deactivate/eval/release/rollback` parse, command names are stable, and read/write/preview metadata is correct | Verify: `cargo test --test cli_surface`
- [x] `SP377-T002` Owner: state-model | Done when: `state/registry/skillsets.json` loads absent as empty, round-trips deterministically, rejects malformed state, and preserves sorted skillsets/members | Verify: `cargo test --test skillset_cli`
- [x] `SP377-T003` Owner: crud | Done when: create/add/remove reject duplicates and missing members, preserve skill source, and commit registry state | Verify: `cargo test --test skillset_cli`
- [x] `SP377-T004` Owner: read-lint | Done when: show includes current skill read-model summaries and lint reports empty/missing/duplicate member findings | Verify: `cargo test --test skillset_cli`
- [x] `SP377-T005` Owner: activation | Done when: dry-run activation returns per-member plans, apply reuses single-skill activation, required failures fail closed, and partial activation failure rolls back or reports recovery commands | Verify: `cargo test --test skillset_cli`
- [x] `SP377-T006` Owner: eval | Done when: member offline eval results aggregate case/pass/fail/skipped counts; an explicit runner executes bundle fixtures against no-skill or single-skill baselines, while fixtures without a selected runner report `not_run` | Verify: `cargo test --test skillset_cli --test skillset_end_to_end`
- [x] `SP377-T007` Owner: release-rollback | Done when: release creates `release/skillset/<name>/<version>` and rollback restores only the skillset definition from version/ref | Verify: `cargo test --test skillset_cli`
- [x] `SP377-T008` Owner: verification | Done when: focused tests, CLI surface tests, diff check, workspace cargo check, and SpecRail check pass | Verify: `git diff --check && cargo test --test skillset_cli && cargo test --test cli_surface && cargo check --workspace --all-targets --all-features`

### SP377-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom skillset create/add/remove/show/lint/activate/deactivate/eval/release/rollback` parse successfully.
- `command_name` returns stable command ids.
- write/read command classification is correct.

Verify:

```bash
cargo test --test cli_surface
```

### SP377-T2: Add Skillset State Module

Owner: implementation

Files:

- `src/commands/skillset_cmds.rs`

Done when:

- absent `state/registry/skillsets.json` loads as empty.
- valid file round-trips deterministically.
- malformed file fails without overwrite.
- skillsets and members are sorted before write.

Verify:

```bash
cargo test --test skillset_cli
```

### SP377-T3: Implement Write Commands

Owner: implementation
Depends on: SP377-T1, SP377-T2

Commands:

- `loom skillset create`
- `loom skillset add`
- `loom skillset remove`

Done when:

- create rejects duplicates.
- add rejects missing skills and duplicate members.
- remove rejects missing skillsets and missing members.
- writes preserve skill sources and projection state.
- write commands are committed/audited consistently with other registry writes.

Verify:

```bash
cargo test --test skillset_cli
```

### SP377-T4: Implement Read Commands

Owner: implementation
Depends on: SP377-T2

Commands:

- `loom skillset show`
- `loom skillset lint`

Done when:

- show includes member summaries from the current skill inventory read model.
- show marks manually drifted missing members.
- lint reports empty skillset warning.
- lint reports required missing member as invalid.

Verify:

```bash
cargo test --test skillset_cli
```

### SP377-T5: Update Tests And Verification

Owner: implementation
Depends on: SP377-T1, SP377-T2, SP377-T3, SP377-T4

Done when:

- focused tests cover every product acceptance criterion in the first slice.
- repository formatting and compile checks pass.
- tests cover activation/eval/release/rollback behavior implemented in this PR.
- implementation and specs distinguish optional runner execution from member-only aggregation.

Verify:

```bash
git diff --check
cargo test --test skillset_cli
cargo check --workspace --all-targets --all-features
```

### SP377-T6: Implement Skillset Activation And Deactivation

Owner: implementation
Depends on: SP377-T1, SP377-T2, SP377-T4

Done when:

- `skillset activate --dry-run` returns per-member plans without writes.
- `skillset activate` calls the single-skill activation path for each ready member.
- required member missing/blocked/quarantined/lint-invalid failures fail closed with typed errors.
- partial activation failure rolls back non-noop activated members or reports recovery commands.
- `skillset deactivate` calls the single-skill deactivation path.

Verify:

```bash
cargo test --test skillset_cli
```

### SP377-T7: Implement Skillset Eval Aggregation

Owner: implementation
Depends on: SP377-T2

Done when:

- `skillset eval <name> --agent <agent>` runs existing member offline evals.
- aggregate summary includes case/pass/fail/skipped counts and aggregate score.
- member failures return `EVAL_FAILED` with the aggregate report.
- detected `skillsets/<name>/evals/` fixtures are `not_run` without an explicit runner; selected runner results are reported separately.

Verify:

```bash
cargo test --test skillset_cli
```

### SP377-T8: Implement Skillset Release And Rollback

Owner: implementation
Depends on: SP377-T2, SP377-T4

Done when:

- `skillset release <name> <version>` validates lint and creates `release/skillset/<name>/<version>`.
- `skillset rollback <name> --to <version|ref>` restores only the skillset definition from the resolved ref.
- rollback does not check out member skill source files.

Verify:

```bash
cargo test --test skillset_cli
```

## Handoff Notes

- The end-to-end `skillsets/<name>/evals/` runner is implemented with deterministic regressions and one real Codex smoke comparison; broad model quality still requires acceptance evidence.
- Issue closure should distinguish implemented runner behavior from any remaining real-agent acceptance criteria.
