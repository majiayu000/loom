# Status Field Classification

Updated: 2026-04-27
Tracks: `src/state_model/mod.rs` — `V3Snapshot::status_view()` (lines 237–277)
Closes: #42

This document is the single source of truth for the tier classification of every field
returned by `status_view()`.  Update it whenever `status_view()` changes.

---

## Three-Tier Model

| Tier | Definition |
|------|------------|
| **Authoritative** | Sourced directly from v3 state files or computed exclusively from registered v3 entities.  Safe for decision logic. |
| **Advisory** | Computed or aggregated from a mix of registered state and heuristic comparisons.  Useful as UX hints; must not drive control-plane decisions. |
| **Compatibility-only** | Retained only for legacy callers; scheduled for removal in a future MAJOR version.  Must not be referenced in new code. |

---

## Field Table

### Top-level fields

| Field | Tier | Storage location | Source citation |
|-------|------|-----------------|-----------------|
| `schema_version` | Authoritative | `state/v3/schema.json` | [source: src/state_model/mod.rs:260] |
| `counts` | — | (object; see sub-fields below) | [source: src/state_model/mod.rs:261–269] |
| `targets[]` | Authoritative | `state/v3/targets.json` | [source: src/state_model/mod.rs:271] |
| `bindings[]` | Authoritative | `state/v3/bindings.json` | [source: src/state_model/mod.rs:272] |
| `rules[]` | Authoritative | `state/v3/rules.json` | [source: src/state_model/mod.rs:273] |
| `projections[]` | Authoritative (see sub-field exceptions below) | `state/v3/projections.json` | [source: src/state_model/mod.rs:274] |
| `checkpoint` | Authoritative | `state/v3/ops/checkpoint.json` | [source: src/state_model/mod.rs:275] |

### `counts` sub-fields

| Field | Tier | Derivation | Source citation |
|-------|------|-----------|-----------------|
| `counts.skills` | Authoritative | Unique `skill_id` values across registered rules and projections | [source: src/state_model/mod.rs:238–249, 261] |
| `counts.targets` | Authoritative | `len()` of registered targets array | [source: src/state_model/mod.rs:262] |
| `counts.bindings` | Authoritative | `len()` of registered bindings array | [source: src/state_model/mod.rs:263] |
| `counts.active_bindings` | Authoritative | Filtered count: `binding.active == true` | [source: src/state_model/mod.rs:252–257, 264] |
| `counts.rules` | Authoritative | `len()` of registered rules array | [source: src/state_model/mod.rs:265] |
| `counts.projections` | Authoritative | `len()` of registered projections array | [source: src/state_model/mod.rs:266] |
| `counts.drifted_projections` | **Advisory** | Computed: `observed_drift == true OR health != "healthy"` — depends on the optional `observed_drift` field | [source: src/state_model/mod.rs:244–250, 267] |
| `counts.operations` | Authoritative | `len()` of operation records from the v3 ops journal | [source: src/state_model/mod.rs:268] |

### `projections[]` sub-fields with classification nuance

All fields on `V3ProjectionInstance` are authoritative unless noted below.

| Sub-field | Tier | Notes | Source citation |
|-----------|------|-------|-----------------|
| `instance_id` | Authoritative | Primary key; stored | [source: src/state_model/mod.rs:142] |
| `skill_id` | Authoritative | Stored | [source: src/state_model/mod.rs:143] |
| `binding_id` | Authoritative | Stored | [source: src/state_model/mod.rs:144] |
| `target_id` | Authoritative | Stored | [source: src/state_model/mod.rs:145] |
| `materialized_path` | Authoritative | Stored | [source: src/state_model/mod.rs:146] |
| `method` | Authoritative | Stored | [source: src/state_model/mod.rs:147] |
| `health` | Authoritative | Stored enum: `healthy`, `drifted`, `missing`, `conflict` | [source: src/state_model/mod.rs:149] |
| `updated_at` | Authoritative | Stored timestamp; optional | [source: src/state_model/mod.rs:155] |
| `last_applied_rev` | **Advisory** (as of v3.x; promotion to authoritative requires a future issue) | Git rev recorded when projection was last applied; represents a historical comparison point, not a live git state | [source: src/state_model/mod.rs:148] |
| `observed_drift` | **Advisory** | Optional boolean; computed by comparing the stored `last_applied_rev` to the current source head at observation time — not set until a check is performed | [source: src/state_model/mod.rs:150–152] |

### Compatibility-only tier

No fields currently fall in this tier.  It is defined here so follow-up issues can
promote fields to it explicitly rather than silently deprecating them.

---

## Environment-Based Discovery

This section documents how env-based discovery interacts with status output.

### Scope boundary

Environment-based discovery affects **skill-source inventory only**.

It does not affect:
- binding records
- target records
- rule records
- projection records
- operation journal entries

All of those are always read from the `state/v3/` JSON files.

### Discovery priority

Resolution order for skill directories is:
1. Process environment variable (via `std::env::var()`)
2. `.env` file in the workspace root (loaded by `load_dotenv_map()`)
3. Hard default derived from `$HOME`

[source: src/state/mod.rs:86–88] — `env_or_dotenv()` implements priority 1 and 2.
[source: src/state/mod.rs:90–104] — `load_dotenv_map()` reads `<workspace_root>/.env`.

### Variables

| Variable | Hard default when unset | Source citation |
|----------|------------------------|-----------------|
| `CLAUDE_SKILLS_DIR` | `$HOME/.claude/skills` | [source: src/state/mod.rs:55–57] |
| `CODEX_SKILLS_DIR` | `$HOME/.codex/skills` | [source: src/state/mod.rs:59–61] |

`resolve_agent_skill_dirs()` returns the first directory from each variable
([source: src/state/mod.rs:51–64]).

`resolve_agent_skill_source_dirs()` expands both variables into a merged, deduplicated
list of source scan paths ([source: src/state/mod.rs:66–84]).

### Why this matters for status classification

`counts.skills` is derived from `skill_id` values inside registered rules and projections
— it is **not** a count of files found in skill directories.  The env-discovery paths
influence which skills are *available for registration*, but once a rule or projection
is registered its `skill_id` is authoritative registered state.

No `/api/v3/status` (or `/api/v3/workspace`) field is computed by scanning the
filesystem paths that `CLAUDE_SKILLS_DIR` or `CODEX_SKILLS_DIR` point to.  Advisory
skill directory hints appear only in diagnostic or migration endpoints, never in the
counts returned by `status_view()`.
