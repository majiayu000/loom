# GH385 Product Spec: Telemetry And Analytics

Issue: https://github.com/majiayu000/loom/issues/385
Parent: https://github.com/majiayu000/loom/issues/376
Status: Draft for implementation
Locale: en-US

## Goal

Add local-first, privacy-preserving telemetry for skill usage, value, cost,
drift, and risk. The feature must help users answer whether skills are being
used, whether they improve outcomes, whether they are stale, and whether risk
or operational cost is increasing.

## Users

1. Individual users who want to prune stale or noisy skills.
2. Teams that need evidence before promoting, trusting, or retiring skills.
3. Maintainers who need aggregate feedback for evals, safety scans,
   recommendations, skillsets, and Panel dashboards.

## Scope For First PR

The first mergeable slice should implement the telemetry foundation:

- explicit local enable/disable state;
- typed local event schema;
- event write path for already-known Loom command/eval/safety events;
- privacy redaction and hashed workspace/session identifiers;
- report aggregation from local event state;
- redacted export and purge with dry-run preview;
- backend read model that Panel can consume later.

Panel UI should be added only after the backend report API is stable.

## Non-Goals

1. No hosted analytics service in v1.
2. No raw prompt, source code, file content, environment value, secret, or
   transcript collection by default.
3. No background daemon requirement.
4. No tracking outside explicit Loom commands or explicit agent integrations.
5. No independent Panel mutation semantics.
6. No recommendation ranking changes until #378 consumes telemetry explicitly.

## Behavior Invariants

1. Telemetry is opt-in unless the event is already part of existing command
   audit state.
2. Disabled telemetry mode must not append telemetry events.
3. Workspace identity and session identity are hashed by default.
4. Raw prompts, code, file content, env values, secrets, and transcripts are not
   stored by default.
5. Redaction must happen before persistence and before export.
6. Exports are redacted by default.
7. Purge supports dry-run preview before deletion.
8. Reports must distinguish missing telemetry from zero usage.
9. Panel must render backend read models and must not invent analytics fields
   absent from CLI/API state.
10. Safety, trust, eval, and dependency trends must consume existing source
    reports; telemetry must not duplicate their authoritative state.

## User-Facing CLI

Required first-slice commands:

```bash
loom telemetry status [--json]
loom telemetry enable [--local-only] [--json]
loom telemetry disable [--json]
loom telemetry report [--skill <skill>] [--agent <agent>] [--since <date>] [--json]
loom telemetry export --format jsonl|csv --output <path> [--redacted]
loom telemetry purge [--before <date>] --dry-run [--json]
```

Deferred commands:

```bash
loom telemetry purge [--before <date>] --confirm <token>
loom skill inspect <skill> --include-telemetry
```

## Event Schema

Telemetry events should use a stable typed schema:

```json
{
  "schema_version": 1,
  "event_id": "evt_...",
  "event_type": "skill.activation",
  "skill_id": "fixflow",
  "agent": "codex",
  "workspace_hash": "sha256:...",
  "session_id_hash": "sha256:...",
  "timestamp": "2026-07-01T00:00:00Z",
  "metrics": {
    "tokens_in": 12000,
    "tokens_out": 3000,
    "commands": 8,
    "duration_ms": 90000,
    "success": true
  },
  "privacy": {
    "raw_prompt_stored": false,
    "raw_code_stored": false,
    "redacted": true
  }
}
```

Required event families:

- `skill.activation`
- `skill.deactivation`
- `skill.invocation`
- `skill.eval`
- `skill.safety`
- `skill.error`
- `recommendation.feedback`

## Report Metrics

Reports should aggregate at skill, agent, skillset, and workspace scope when
the relevant inputs exist:

1. activations and deactivations;
2. invocations or inferred invocations;
3. eval runs, pass rate, and baseline delta;
4. token and command overhead;
5. duration and failure categories;
6. stale days since last successful eval;
7. safety findings trend;
8. dependency readiness trend;
9. recommendation accepted, rejected, and ignored counts.

## Panel UX

Panel dashboard is deferred until backend read models are stable. When added,
it should consume the same report API and include:

1. skill health table;
2. usage vs eval-delta scatterplot;
3. stale skills needing re-eval;
4. high-risk active skills;
5. high token/command overhead skills;
6. recommendation feedback summary.

## Acceptance Criteria

1. `loom telemetry status --json` reports enabled state, storage path, retention
   policy, and privacy mode.
2. `loom telemetry enable --local-only` enables local telemetry without hosted
   service configuration.
3. `loom telemetry disable` prevents new telemetry event writes.
4. Event persistence stores hashed workspace/session ids and no raw private
   content by default.
5. Reports summarize usage, value, cost, drift, and risk from local events.
6. Export supports redacted JSONL and CSV output.
7. Purge dry-run previews event counts and byte impact before deletion.
8. Confirmed purge deletes only selected telemetry events and preserves
   unrelated registry state.
9. `skill inspect` can include last usage/eval telemetry summary when telemetry
   is enabled.
10. Panel dashboard reads backend telemetry reports once the API is available.
11. Tests cover enable, disable, event writing, disabled mode, redaction,
    aggregation, export, purge dry-run, confirmed purge, and API read model
    output.

## Open Questions

1. Whether telemetry should live beside command audit events or in a dedicated
   telemetry event store.
2. Whether command audit events should be imported into telemetry reports or
   only referenced.
3. Whether retention defaults should be global or workspace-specific.
