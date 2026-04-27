# Loom v3 Review

Updated: 2026-04-09
Status: Superseded by accepted phase-1 decisions

The remaining open questions in this review have been resolved for phase 1 in
[LOOM_V3_ARCHITECTURE_DECISIONS.md](LOOM_V3_ARCHITECTURE_DECISIONS.md). Keep
this file as historical review context; use the architecture decisions document
as the current contract.

## 1. Purpose

This document reviews the current v3 document set as a whole.

Reviewed documents:

1. [LOOM_V3_SPEC.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_SPEC.md)
2. [LOOM_V3_MIGRATION.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_MIGRATION.md)
3. [LOOM_V3_TEST_PLAN.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_TEST_PLAN.md)
4. [LOOM_V3_CLI_CONTRACT.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_CLI_CONTRACT.md)
5. [LOOM_V3_STATE_FIXTURES.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_STATE_FIXTURES.md)
6. [LOOM_V3_API_CONTRACT.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_API_CONTRACT.md)

The goal is to identify:

1. what is now aligned
2. what is still underspecified
3. what must be fixed before logic implementation begins

## 2. Review Summary

The v3 design is now coherent enough to start implementation planning.

The document set is aligned on the main architectural correction:

1. source is canonical
2. bindings are explicit
3. targets are separate from bindings
4. projections are derived state
5. capture is explicit
6. panel and API are secondary read models

This is a major improvement over v2.

However, there are still a few design questions that should be frozen before business logic is implemented.

## 3. Aligned Decisions

These points are consistent across all v3 documents.

### 3.1 Canonical Truth

Aligned:

1. `SkillSource` is the only canonical source of truth.
2. Live directories are projections, not truth.
3. `capture` is the explicit bridge from projection drift back into source.

Impact:

1. This removes the old temptation to treat a live Claude directory as the canonical store.

### 3.2 Multi-Workspace Model

Aligned:

1. `binding_id` is a first-class identity.
2. `target_id` is a first-class identity.
3. Absolute paths are data, not identity.

Impact:

1. Claude multi-workdir is now representable without default path guessing.

### 3.3 Projection Boundary

Aligned:

1. `target add` registers a location only.
2. `workspace binding add` registers a workspace-to-target preference only.
3. `skill project` is the actual materialization step.

Impact:

1. `init/import/link` are no longer fused into one unsafe super-command.

### 3.4 Observation and Capture

Aligned:

1. `observe_only` exists.
2. `auto_capture` is intentionally rejected.
3. drift is tracked per projection instance.

Impact:

1. Loom can acknowledge live edits without redefining truth automatically.

### 3.5 CLI and API Split

Aligned:

1. CLI is the authoritative machine control plane.
2. API is a read model over v3 state.
3. Panel must not invent state that CLI cannot express.

Impact:

1. UI work is safely decoupled from core logic.

## 4. Remaining Open Questions

These are not contradictions, but they are still underdefined enough to matter during implementation.

### 4.1 Cardinality of `BindingRule`

Current state:

1. examples imply one `skill_id + binding_id -> one target_id`
2. fixtures and CLI examples both follow that assumption

Open question:

1. should one binding be allowed to project the same skill to multiple targets intentionally

Recommendation:

1. freeze v3 phase 1 to `one skill + one binding -> one active target`
2. treat multi-target fan-out inside one binding as future extension

Reason:

1. this keeps projection conflict logic simpler

### 4.2 `policy_profile` Vocabulary

Current state:

1. `policy_profile` appears in bindings
2. examples use `safe-capture`
3. no fixed vocabulary exists yet

Recommendation:

1. define a minimal enum before logic implementation

Suggested initial values:

1. `safe-capture`
2. `read-only`
3. `manual-review`

### 4.3 `watch_policy` Vocabulary

Current state:

1. spec defines `off`, `observe_only`, `observe_and_warn`
2. fixtures currently use `observe_only`

Recommendation:

1. freeze these three values and reject unknown values in v3 schema parsing

### 4.4 Projection Repair Surface

Current state:

1. CLI contract mentions projection instance inspection and repair concepts
2. no explicit `repair` command is defined yet

Recommendation:

1. keep repair out of phase 1 implementation
2. do not add parser or API semantics that imply repair already exists

### 4.5 API Envelope vs CLI Envelope

Current state:

1. CLI envelope includes `cmd` and may include `op_id`
2. API envelope is read-oriented and intentionally simpler

Recommendation:

1. keep the distinction
2. document it as intentional, not accidental

Reason:

1. read APIs are resource views, not command execution responses

### 4.6 Sync Scope

Current state:

1. v3 says Git owns source revisions
2. sync acts on source and operation history
3. projection replay semantics are not yet fully defined

Recommendation:

1. phase 1 implementation should avoid v3 sync logic
2. freeze sync after local source/binding/projection semantics are implemented

## 5. Ready For Implementation

These areas are ready to be implemented now:

1. v3 state path layout
2. v3 read-only schema parsing
3. v3 Rust types for schema objects
4. selector validation helpers for `binding_id`, `target_id`, `instance_id`
5. read-only workspace summary assembly from v3 files

## 6. Not Ready For Implementation

These areas should not be implemented yet:

1. destructive projection writes
2. capture conflict resolution
3. migration apply logic
4. sync replay logic for v3
5. panel write actions

## 7. Recommended Implementation Order

1. add `src/v3.rs` types and parsers
2. add v3 fixture-based parser tests
3. add read-only `workspace status` view assembly for v3
4. add command surface skeleton for `workspace binding`, `target`, and `skill project/capture`
5. only then begin write-path logic

## 8. Release Gate Before Logic Work

Before full logic implementation, freeze these decisions:

1. one active target per `skill_id + binding_id`
2. accepted `policy_profile` enum values
3. accepted `watch_policy` enum values
4. whether migration plan output needs stable machine-readable issue codes

## 9. Conclusion

The v3 document set is strong enough to start code skeleton work now.

It is not yet strong enough to safely implement full projection, capture, and migration logic without first freezing a few enum and cardinality details.
