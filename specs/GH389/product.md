# GH389 Product Spec: Instruction Surface Inventory

Issue: https://github.com/majiayu000/loom/issues/389
Parent: https://github.com/majiayu000/loom/issues/376
Status: Draft for implementation
Locale: en-US

## Goal

Add read-only inventory, classification, diagnosis, and migration planning for
non-skill instruction surfaces while preserving a strict boundary between
portable Agent Skills and always-on or project-scoped instructions.

Loom should help users understand what an agent may see, how always-on
instructions can shadow or conflict with skills, and whether repeated
task-specific instructions should become a skill. Loom must not silently import,
rewrite, or project non-skill instruction files as skills.

## Users

1. Users who want to understand why a skill is not triggering as expected.
2. Maintainers auditing project instructions such as `AGENTS.md`, `CLAUDE.md`,
   Cursor rules, Windsurf/Cascade files, and Copilot instructions.
3. Teams deciding whether repeated instruction blocks should remain always-on,
   move into skill references, or become a new skill draft.

## Scope For First PR

The first mergeable slice should implement read-only discovery and planning:

- adapter-driven `instruction scan`;
- `instruction show` for one discovered surface;
- `instruction classify <path>` for one file;
- `instruction doctor` to explain instruction/skill overlap;
- `instruction migrate-plan` dry-run output with no writes;
- explicit proof that portable skill lint still rejects non-`SKILL.md`
  instruction files as skills.

## Non-Goals

1. No silent edits to `AGENTS.md`, `CLAUDE.md`, Cursor rules, Windsurf rules,
   memories, workflows, VS Code settings, or Copilot instruction files.
2. No automatic conversion of instructions into skills.
3. No claim that Loom owns native agent instruction precedence.
4. No storage of private memories or raw agent transcripts in registry state.
5. No weakening of portable `SKILL.md` validation by accepting arbitrary
   instruction files as skills.
6. No Panel-first instruction mutation workflow.

## Behavior Invariants

1. `instruction scan`, `show`, `classify`, `doctor`, and `migrate-plan
   --dry-run` are read-only.
2. Discovery paths and precedence notes come from adapter metadata where
   available.
3. Unknown surfaces are reported as unknown or unsupported when they affect
   diagnosis; they are not silently ignored.
4. Native scope and precedence are preserved as advisory metadata and are not
   invented when unknown.
5. Non-skill instructions are not registered as skills.
6. Portable skill lint continues to require `SKILL.md` and rejects arbitrary
   instruction files as skills.
7. Migration planning emits reviewable patch plans only.
8. Raw memories, transcripts, private instruction content, and secret-looking
   values are not stored in registry state.
9. Duplicate or conflicting guidance is reported with file paths, scope, and
   affected skill where known.

## User-Facing CLI

Required first-slice commands:

```bash
loom instruction scan [--agent codex|claude|cursor|windsurf|copilot] [--workspace <path>] [--json]
loom instruction show <instruction-id> [--json]
loom instruction classify <path> [--json]
loom instruction doctor [--agent <agent>] [--workspace <path>] [--skill <skill>] [--json]
loom instruction migrate-plan <instruction-id> --to skill|reference|keep-instruction [--name <skill>] --dry-run [--json]
```

Deferred command:

```bash
loom instruction migrate-apply <plan-id> --idempotency-key <key>
```

Apply is intentionally deferred until dry-run planning is proven.

## Surfaces To Inventory

Initial support should be adapter-driven and read-only:

1. root and nested `AGENTS.md`;
2. `CLAUDE.md` where relevant;
3. `.cursor/rules/*.mdc` and known Cursor rule locations;
4. Windsurf/Cascade rule, workflow, memory, and skill-adjacent paths where
   documented or configured;
5. VS Code or Copilot custom instruction files where discoverable;
6. project `.agents/skills` and user skill roots only as skills, not
   instructions.

## Instruction Model

`instruction scan` should return records like:

```json
{
  "instruction_id": "instr_...",
  "agent": "codex",
  "kind": "agents_md",
  "scope": "workspace",
  "path": "/repo/AGENTS.md",
  "applies_to": "/repo",
  "precedence": "agent-defined",
  "always_on": true,
  "contains_skill_like_workflow": true,
  "suggested_action": "keep-instruction",
  "warnings": []
}
```

## Migration Planning

`migrate-plan` must be dry-run and produce one of these reviewable actions:

1. `keep-instruction`: leave content as always-on instruction;
2. `move-to-reference`: move long background into a skill `references/` file;
3. `extract-skill`: create a new skill draft with trigger boundaries and
   references;
4. `review-conflict`: ask for human review when precedence or conflict cannot
   be resolved safely.

No files are written by `migrate-plan --dry-run`.

## Acceptance Criteria

1. `instruction scan` reports known `AGENTS.md` files without mutating files.
2. `instruction scan` reports at least one adapter-specific rule surface when
   present.
3. Unknown or unsupported surfaces are reported explicitly when they affect
   diagnosis.
4. `instruction doctor --skill <skill>` reports duplicate or conflicting
   guidance between a skill and always-on instructions.
5. `migrate-plan` emits a reviewable plan and performs no writes in dry-run
   mode.
6. Portable skill lint still rejects non-`SKILL.md` instruction files as
   skills.
7. Adapter metadata owns discovery paths and precedence notes.
8. Tests cover `AGENTS.md` discovery, nested scope, unsupported agent surface,
   duplicate skill/instruction guidance, migrate-plan no-write behavior, and
   non-skill rejection by lint.

## Open Questions

1. Whether instruction scan results should be ephemeral only or optionally
   cached as redacted observations.
2. Whether migration apply should create draft files directly or require a
   separate `skill new` command.
3. How much native precedence detail can be reliably represented per adapter.
