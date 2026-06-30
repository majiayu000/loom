# GH377 Tasks: Skillsets And Bundles

Issue: https://github.com/majiayu000/loom/issues/377
Product spec: `specs/GH377/product.md`
Tech spec: `specs/GH377/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement only the first non-blocked slice:

```text
skillset create/add/remove/show/lint
```

Do not implement:

```text
skillset activate/deactivate/eval/release/rollback
```

Those remain blocked by #367, #369, and #370.

## Tasks

### SP377-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom skillset create/add/remove/show/lint` parse successfully.
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
- no implementation claims cover deferred activation/eval/release/rollback behavior.

Verify:

```bash
git diff --check
cargo test --test skillset_cli
cargo check --workspace --all-targets --all-features
```

## Handoff Notes

- Use `Refs #377` for a partial first-slice PR unless the PR intentionally satisfies every acceptance criterion in #377.
- Do not use `Fixes #377` until activation, eval aggregation, release, rollback, and partial activation recovery are implemented or explicitly split into follow-up issues.
- The first PR should say that activation/eval remain blocked by #367/#369/#370.
