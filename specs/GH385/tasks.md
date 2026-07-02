# GH385 Tasks: Telemetry And Analytics

Issue: https://github.com/majiayu000/loom/issues/385
Product spec: `specs/GH385/product.md`
Tech spec: `specs/GH385/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement only the telemetry foundation:

```text
local opt-in config + redacted event schema + report/export/purge read models
```

Do not implement in the first PR:

```text
hosted analytics, raw prompt/code collection, background daemon, recommendation
ranking changes, or Panel-only analytics state
```

## Tasks

- [ ] `SP385-T1` Owner: implementation | Done when: telemetry status/enable/disable/report/export/purge CLI parses and command ids classify read/write behavior correctly | Verify: `cargo test --test cli_surface`
- [ ] `SP385-T2` Owner: implementation | Done when: telemetry config and event models parse, serialize, redact, and reject malformed writes deterministically | Verify: `cargo test --test telemetry`
- [ ] `SP385-T3` Owner: implementation | Done when: enable/disable update local config, disabled mode prevents event appends, and known command/eval/safety paths call the telemetry writer only when enabled | Verify: `cargo test --test telemetry`
- [ ] `SP385-T4` Owner: implementation | Done when: report aggregates usage, eval, cost, drift, and risk while marking unavailable upstream data as missing | Verify: `cargo test --test telemetry`
- [ ] `SP385-T5` Owner: implementation | Done when: export writes redacted JSONL/CSV and purge dry-run/confirm operate only on telemetry events | Verify: `cargo test --test telemetry`
- [ ] `SP385-T6` Owner: implementation | Done when: inspect/API consume the telemetry read model and Panel renders a basic dashboard from the backend report API | Verify: `cargo test --test telemetry`

### SP385-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs` or a split telemetry args module
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom telemetry status [--json]` parses.
- `loom telemetry enable [--local-only] [--json]` parses.
- `loom telemetry disable [--json]` parses.
- `loom telemetry report [--skill <skill>] [--skillset <skillset>] [--agent <agent>] [--workspace <path>] [--since <date>] [--json]` parses.
- `loom telemetry export --format jsonl|csv --output <path> [--redacted]` parses.
- `loom telemetry purge [--before <date>] --dry-run [--json]` parses.
- `loom telemetry purge [--before <date>] --confirm <token>` parses.
- `loom skill inspect <skill> --include-telemetry` parses once inspect wiring is
  included.
- read/write command classification matches behavior.

Verify:

```bash
cargo test --test cli_surface
```

### SP385-T2: Add Telemetry State And Redaction

Owner: implementation
Depends on: SP385-T1

Files:

- new telemetry state module
- new `tests/telemetry.rs`

Done when:

- config and event schemas round-trip deterministically.
- absent config reports disabled state.
- redaction strips prompt, code, file content, env, secret, and transcript-like
  fields before persistence.
- redaction failure aborts event writes.
- malformed existing event lines are surfaced in report warnings.

Verify:

```bash
cargo test --test telemetry
```

### SP385-T3: Implement Enable, Disable, And Event Writes

Owner: implementation
Depends on: SP385-T2

Done when:

- enable writes local-only config.
- disable sets `enabled=false`.
- disabled mode prevents appending telemetry events.
- event writes are append-only JSONL and typed.
- existing command, eval, and safety event sources call the writer when
  telemetry is enabled.
- command-audit import stays report-only when telemetry is disabled.
- telemetry writes do not mutate skill source, active target projections, or
  unrelated registry state.

Verify:

```bash
cargo test --test telemetry
```

### SP385-T4: Implement Reports

Owner: implementation
Depends on: SP385-T2, SP385-T3

Done when:

- report aggregates activations, invocations, evals, token cost, command cost,
  failures, stale eval days, safety trend, dependency trend, and recommendation
  feedback where data exists.
- missing eval/safety/dependency inputs appear as `missing`, not zero.
- skill, agent, skillset, and workspace filters are supported where data exists.
- `skill inspect` can include a concise telemetry summary after #366 wiring.

Verify:

```bash
cargo test --test telemetry
```

### SP385-T5: Implement Export And Purge

Owner: implementation
Depends on: SP385-T2

Done when:

- redacted JSONL export works.
- redacted CSV export works.
- export rejects unsafe output paths.
- purge dry-run reports matching event count and bytes without mutation.
- confirmed purge uses an explicit token or idempotency key and atomically
  rewrites only telemetry event state.

Verify:

```bash
cargo test --test telemetry
```

### SP385-T6: Add API, Panel, Docs, And Final Checks

Owner: implementation
Depends on: SP385-T4, SP385-T5

Done when:

- CLI/API contracts document telemetry privacy and report semantics.
- Panel API returns the telemetry report read model.
- Panel renders a basic telemetry dashboard from the API read model.
- tests cover the first-slice acceptance criteria.
- repository checks pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

- Use `Refs #385` for a first-slice PR unless every CLI, report, export, purge,
  inspect, API, and Panel acceptance criterion is implemented.
- Use `Fixes #385` only when local telemetry, dashboard, inspect summary,
  export, purge, and tests are complete.
- Do not store raw prompts, source code, env values, secrets, or transcripts by
  default.
