# GH452 Product Spec: CLI Concept Convergence And Command-Surface Budget

Issue: https://github.com/majiayu000/loom/issues/452
Blocks: #454, #455, #456, #457
Status: Draft for review
Locale: zh-CN

## Problem

The CLI surface grew from 14 top-level / 63 leaf commands (v0.1.5) to 29
top-level / 144 leaf commands at `9b920c9`, with the `skill` group alone
holding 58 leaves. Several clusters expose one engine through multiple entry
points, so users and agents must choose between near-synonyms:

1. `search` / `resolve` / `recommend` share one scorer
   (`score_and_filter_skills`).
2. `activate` / `project` / top-level `use` end in the same projection core.
3. `scan` output is embedded inside `policy`.
4. `show` is a subset of `inspect`.
5. `capture` / `save` both produce one source commit; `snapshot` is an
   unversioned `release`; `verify` is a subset of `diagnose`.
6. Top-level `doctor` is a literal alias of `workspace doctor`.

README ships a mnemonic to keep the lifecycle verbs apart. A verb set that
needs a mnemonic is not converged. Meanwhile the most common single ask -
"make this skill visible to Claude Code in my user scope" - still requires the
manual six-command path because `loom use` cannot reach real user-scope
directories for non-codex agents.

## Goal

One verb per concept, a written budget for future surface growth, and a
two-command happy path onto real agent directories:

```bash
loom skill add ./my-skill --name my-skill
loom use my-skill --agents claude --scope user --adopt --apply
```

Error responses become as machine-guiding as success responses via
`error.next_actions[]`.

## Scope

- #454 lifecycle verb consolidation: `commit` replaces `capture`+`save`;
  `release --anchor` absorbs `snapshot`; `diagnose --check drift` absorbs
  `verify`.
- #455 read-surface convergence: `search` absorbs `resolve`/`recommend`;
  `inspect --brief` absorbs `show`; `policy` composes `scan`; one of
  `doctor` / `workspace doctor` survives.
- #456 `loom use --scope user` routed through adapter discovery roots, with
  explicit `--adopt` before writing into a previously observed directory.
- #457 additive `error.next_actions[]` in the envelope for the top not-found
  error classes.

## Non-Goals

1. No loss of `--json` machine capability; every merged verb's data remains
   reachable via flags on the surviving verb.
2. No compatibility aliases for removed verbs (V1 breaking-change policy).
3. No changes to registry state schemas; this epic is surface-level.
4. No new features hidden inside the consolidation PRs.

## Surface Budget Rule

After this epic, adding a new leaf command requires one of:

1. Merging or removing an existing leaf in the same PR; or
2. An ADR entry in `docs/LOOM_ARCHITECTURE_DECISIONS.md` stating why the
   concept cannot be expressed as a flag or mode of an existing verb.

CI enforces the count via `tests/cli_surface.rs`: the test records the
expected leaf total, and the PR that raises it must update the recorded
number plus one of the two justifications above.

## Success Criteria

1. `skill` group shrinks from 58 to at most 52 leaves; top level from 29 to at
   most 28.
2. The README lifecycle mnemonic becomes unnecessary and is deleted.
3. Two-command claude user-scope happy path passes in
   `scripts/e2e-agent-flow.sh`.
4. BINDING_NOT_FOUND, TARGET_NOT_FOUND, SKILL_NOT_FOUND,
   STATE_NOT_INITIALIZED, TARGET_NOT_MANAGED all return runnable
   `next_actions`.
