# GH385 Tech Spec: Telemetry And Analytics

Issue: https://github.com/majiayu000/loom/issues/385
Product spec: `specs/GH385/product.md`
Status: Draft for implementation

## Design Summary

Add a local telemetry subsystem that is explicit, redacted before persistence,
and report-oriented. Telemetry should reuse existing command audit, eval,
safety, dependency, and activation status where possible while keeping a clear
privacy boundary.

The first slice should:

1. add CLI commands for status, enable, disable, report, export, and purge
   dry-run;
2. add a typed event model and redaction layer;
3. persist local events only when telemetry is enabled; existing command audit
   records are report-only unless the user explicitly enables telemetry before
   import;
4. aggregate report read models for CLI and Panel API consumers;
5. keep Panel UI deferred until report routes are stable.

## Dependencies And Blocks

| Issue | Required capability |
|---|---|
| #366 | `skill inspect` status model |
| #367 | activation/deactivation semantics |
| #369 | eval report inputs |
| #370 | safety/trust findings |
| #377 | skillset aggregation inputs |
| #378 | recommendation feedback inputs |

Missing upstream data should appear as `missing` or `not_available`, not zero.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs` or split telemetry args module |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| telemetry implementation | new `src/commands/telemetry.rs` or module directory |
| state model | new telemetry state/event module |
| Panel API | `docs/LOOM_API_CONTRACT.md`, panel API handlers after backend model |
| Panel UI | dashboard page after backend API stabilizes |
| tests | new `tests/telemetry.rs`, API/Panel tests when wired |
| docs/specs | `specs/GH385/*`, CLI/API contract docs |

## State Model

Suggested files:

```text
state/telemetry/config.json
state/telemetry/events.jsonl
```

Config shape:

```json
{
  "schema_version": 1,
  "enabled": true,
  "mode": "local-only",
  "redaction": "default",
  "retention_days": 90
}
```

Event shape:

```json
{
  "schema_version": 1,
  "event_id": "evt_...",
  "event_type": "skill.eval",
  "skill_id": "fixflow",
  "agent": "codex",
  "workspace_hash": "sha256:...",
  "session_id_hash": "sha256:...",
  "timestamp": "2026-07-01T00:00:00Z",
  "metrics": {},
  "privacy": {
    "raw_prompt_stored": false,
    "raw_code_stored": false,
    "redacted": true
  }
}
```

Rules:

1. Append events as JSONL.
2. Parse events with typed serde models.
3. Reject malformed new writes before append.
4. Quarantine malformed existing lines in report output instead of dropping them
   silently.
5. Use atomic rewrite for purge.
6. Never store raw private content in telemetry event fields by default.

## Redaction

Redaction must run before persistence:

1. hash workspace path with a local salt or stable one-way digest;
2. hash session id;
3. strip raw prompts, source snippets, file contents, env values, and secrets;
4. store path hashes or basename-only labels unless explicitly allowed by
   local-only profile;
5. mark the privacy block with redaction status.

If redaction fails, event persistence fails. It must not write unredacted data
and continue.

## Command Behavior

### status

Reads config and reports:

- enabled state;
- storage path;
- mode;
- retention;
- event count;
- malformed event count;
- privacy summary.

### enable

Creates or updates telemetry config. It may write config state and should use
the same durable write/audit semantics as other registry state changes when
available.

### disable

Sets `enabled=false` and prevents future telemetry event writes.

### report

Reads events and aggregates by skill, agent, skillset, and workspace where data
exists. Missing upstream sources return `missing`, not zero.

### export

Writes redacted JSONL or CSV to an explicit output path. Export must reject
paths that would overwrite registry state unless an existing explicit overwrite
pattern supports it.

### purge

Dry-run reports matching event counts and bytes. Confirmed purge should require
an explicit confirmation token or equivalent idempotency key and atomically
rewrite the telemetry event file.

## Panel API And UI

Panel must consume a backend report read model. Suggested route:

```text
GET /api/v1/telemetry/report
```

The API should return the same summary fields as `loom telemetry report --json`.
Panel UI work should wait until this read model is implemented and tested.

## Test Plan

Focused tests:

1. status when config is absent;
2. enable creates local-only config;
3. disable prevents event appends;
4. event writer redacts before persistence;
5. redaction failure prevents writes;
6. report aggregates usage/value/cost/risk metrics;
7. missing eval/safety inputs are marked missing;
8. export JSONL and CSV are redacted;
9. purge dry-run does not mutate;
10. confirmed purge atomically removes selected events;
11. API read model matches CLI report once API is wired.

Suggested commands:

```bash
git diff --check
cargo test --test telemetry
cargo check --workspace --all-targets --all-features
cd panel && bun run typecheck
cd panel && bun run test
```

Run Panel commands only when Panel files or API contracts are touched.

## Rollback

The first slice should be isolated to telemetry config, event storage, CLI
commands, report aggregation, tests, and docs. Rollback can remove telemetry
commands and ignore/delete `state/telemetry/*` without changing existing skill
source, projection, audit, or Panel state.

## Risks

1. Telemetry can accidentally capture private content. Mitigation: redaction
   before persistence and tests for prompt/code/env/secret fields.
2. Missing data can be misreported as zero. Mitigation: explicit `missing`
   status in reports.
3. Panel can invent analytics semantics. Mitigation: Panel consumes CLI/API
   read model only.
4. Purge can damage unrelated state. Mitigation: dedicated telemetry files and
   atomic rewrite tests.
