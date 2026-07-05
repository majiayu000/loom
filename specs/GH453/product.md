# GH453 Product Spec: Core Layering And State Convergence

Issue: https://github.com/majiayu000/loom/issues/453
Blocks: #458, #459, #460, #461
Status: Implemented by #458, #459, #460, and #461
Locale: zh-CN

## Problem

Epics #363 and #376 landed roughly 59k lines in one week. The architecture
absorbed them, but every feature still lands as `fn cmd_*` inside `impl App`,
mixing domain rules, IO, validation, and JSON shaping in one layer. The cost
now compounds per feature:

1. 237 `cmd_*` functions across 30 files; 45 files in `src/commands/` exceed
   500 lines, several parked at 795-799 lines, directly under the 800-line
   ceiling.
2. The same `Command` tree is matched three times (dispatch, audit
   classification, durable-audit classification). Every new leaf edits three
   parallel matches; missing one silently mis-classifies audit behavior.
3. Before #459, two operation-log authorities were both live (legacy pending
   queue vs registry journal), a split `LOOM_ARCHITECTURE_DECISIONS.md`
   section 1 froze "for phase 1" and kept outliving phases.
4. Persisted vocabularies (`agent`, `ownership`, `method`, `health`) are raw
   strings; the type system cannot reject invalid states the ADR says writers
   must never emit.
5. Adapter v2 metadata is real only for codex; the other nine built-ins share
   one hardcoded capability literal, and external `health_checks` are parsed
   then dropped.

## Goal

Converge the foundation before the next feature epic: one domain service
layer both CLI and Panel call, one command metadata table, one operation-log
authority, typed vocabularies, and adapter metadata that is real for every
built-in agent.

## Scope

- #458 extract `src/core/` domain services; declarative per-command metadata
  replaces the triple match; panel calls services directly.
- #459 registry operations journal is the single write authority; legacy
  `pending_ops.*` runtime writers and files are deleted.
- #460 shared serde enums for `agent`, `ownership`, `method`, `health`,
  matcher `kind`; `state_model` stores enums, CLI re-exports them.
- #461 real per-agent v2 metadata (claude first), `health_checks` either
  wired into diagnostics or deleted from the schema.

## Non-Goals

1. No user-facing feature changes; CLI contract stays identical except where
   GH452 explicitly changes it.
2. No rewrite of gitops or the panel frontend.
3. No new persistence format; on-disk strings keep their current spellings
   (enums serialize to the existing values).

## Success Criteria

1. Adding a command touches one dispatch registration plus one metadata row.
2. `rg pending_ops src/` returns nothing.
3. A typoed `health` value fails load with a typed schema error instead of
   round-tripping.
4. Claude has discovery/visibility/reload metadata backed by tests
   equivalent to the codex coverage.

Closeout evidence:

- #458, #459, #460, and #461 are closed.
- PRs #467, #468, #469, and #472 provide the implementation evidence for this
  umbrella.
- Current verification for the closeout PR reruns Rust full checks,
  `scripts/e2e-agent-flow.sh`, and Panel typecheck/test.

## Ordering

#460 and #461 are independent and can land first. #458 should land before
the next feature epic begins. #459 is closed by the registry-journal authority
migration and must not regain legacy pending-queue writers.
