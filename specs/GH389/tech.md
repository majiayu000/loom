# GH389 Tech Spec: Instruction Surface Inventory

Issue: https://github.com/majiayu000/loom/issues/389
Product spec: `specs/GH389/product.md`
Status: Draft for implementation

## Design Summary

Add an instruction-surface read model separate from Loom registry binding rules
and portable skills. The feature discovers known agent instruction files,
classifies them, diagnoses overlap with skills, and produces dry-run migration
plans. It does not mutate instruction files or register them as skills.

## Dependencies And Blocks

| Issue | Required capability |
|---|---|
| #366 | single-skill status and inspect model |
| #373 | adapter metadata for discovery roots and visibility |
| #374 | docs and migration guide conventions |
| #365 | portable skill lint rejection of non-`SKILL.md` inputs |

If adapter metadata is missing, commands should return unsupported or unknown
surface records rather than hard-coded guesses.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs` or split instruction args module |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| instruction model | new `src/commands/instruction.rs` or module directory |
| adapter metadata | agent adapter discovery metadata after #373 |
| lint boundary | existing skill lint tests |
| tests | new `tests/instruction_surfaces.rs`, lint tests |
| docs/specs | `specs/GH389/*`, CLI/migration docs |

## Data Model

Instruction records are read models, not registry-owned source truth:

```rust
struct InstructionSurface {
    instruction_id: String,
    agent: AgentKind,
    kind: InstructionKind,
    scope: InstructionScope,
    path: PathBuf,
    applies_to: Option<PathBuf>,
    precedence: InstructionPrecedence,
    always_on: bool,
    contains_skill_like_workflow: bool,
    suggested_action: InstructionSuggestedAction,
    warnings: Vec<InstructionFinding>,
}
```

Kinds:

```text
agents_md
claude_md
cursor_rule
windsurf_rule
windsurf_memory
windsurf_workflow
copilot_instruction
vscode_instruction
unknown
```

Suggested actions:

```text
keep-instruction
extract-skill
move-to-reference
review-conflict
unsupported
```

## Discovery

Discovery should:

1. start from workspace root unless a workspace path is provided;
2. use adapter metadata for known instruction roots and precedence notes;
3. find root and nested `AGENTS.md`;
4. find `CLAUDE.md` where relevant;
5. find `.cursor/rules/*.mdc` when Cursor metadata supports it;
6. find Windsurf/Cascade paths only when documented or configured;
7. find VS Code/Copilot instruction paths when discoverable;
8. classify project and user skill roots as skills, not instructions.

Discovery must be read-only and should bound traversal to avoid scanning large
unrelated directories.

## Classification

Classification should use conservative structured and textual signals:

1. file kind and adapter;
2. scope and applies-to path;
3. always-on versus triggered behavior;
4. trigger phrase patterns;
5. long procedural workflow blocks;
6. script/tool references;
7. repeated skill-like tasks;
8. safety or policy instructions;
9. unknown or unsupported fields.

Classifiers may produce suggestions, but they must not create hard migration
decisions without explicit user action.

## Doctor Behavior

`instruction doctor` should:

1. load relevant instruction surfaces;
2. load the skill read model when `--skill` is provided;
3. compare trigger phrases, task descriptions, safety instructions, tool
   requirements, and always-on rules;
4. report duplicates, conflicts, shadowing risks, prompt-budget risks, and
   missing adapter visibility;
5. recommend keep/extract/reference/review actions.

It must report missing data explicitly and avoid native precedence claims when
adapter metadata is unknown.

## Migration Planning

`instruction migrate-plan --dry-run` should:

1. resolve the instruction record;
2. generate a reviewable patch plan;
3. for `extract-skill`, propose a draft skill name, `SKILL.md`, and optional
   `references/` file split;
4. for `move-to-reference`, propose a target skill reference file and source
   edits;
5. for `keep-instruction`, explain why no file changes are needed;
6. write no files.

Apply is deferred. If later implemented, apply must use idempotency keys and
protect high-context files such as `AGENTS.md` from silent modification.

## Lint Boundary

Portable skill lint remains strict:

1. valid skills require `SKILL.md`;
2. `AGENTS.md`, `CLAUDE.md`, `.mdc`, and custom instruction files are not
   accepted as skills;
3. migration plans may propose draft skills, but lint still validates the
   generated draft separately.

## Test Plan

Focused tests:

1. discover root `AGENTS.md`;
2. discover nested `AGENTS.md` with nested scope;
3. discover Cursor rule when adapter metadata supports it;
4. unsupported agent surface is reported explicitly;
5. classify skill-like workflow without registering a skill;
6. doctor reports duplicate guidance between skill and instruction;
7. migrate-plan writes no files;
8. skill lint rejects non-`SKILL.md` instruction files;
9. scan does not store private memory or transcript content.

Suggested commands:

```bash
git diff --check
cargo test --test instruction_surfaces
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
```

Run SpecRail workflow validation for this packet when available.

## Rollback

The first slice should be isolated to read-only instruction scan/classify/doctor
and migration-plan commands, tests, docs, and adapter metadata reads. Rollback
removes the instruction command group without changing skill source, registry
state, native instruction files, or active projections.

## Risks

1. Silent mutation of high-context instruction files. Mitigation: dry-run only
   planning and explicit apply deferral.
2. Treating always-on instructions as skills. Mitigation: separate read model
   and lint boundary tests.
3. False precedence claims. Mitigation: adapter metadata owns precedence notes
   and unknown values remain unknown.
4. Private memory leakage. Mitigation: do not persist raw memory/transcript
   content in registry state.
