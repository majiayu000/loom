# Changelog

All notable public release changes are tracked here. Loom also publishes release
archives, checksums, and provenance details on GitHub Releases.

## Unreleased

### Changed

- Registry operations journal is now the single operation-log authority for
  sync replay, ops retry/purge, history repair, and Panel operation backlog
  reads; legacy `state/pending_ops.*` runtime state is no longer used.

### Fixed

- Local-only registries no longer report successful journal rows and audit
  history as pending operations. CLI, doctor, Panel API, and Panel now expose
  the same four non-overlapping actionable, local-journal, unpushed-history,
  and local-only-history counters while preserving compatibility aliases.

## [0.1.5] - 2026-06-27

### Added

- Configurable agent adapters with documented provider boundaries, so operators
  can keep discovery, preview, local lockfiles, policy checks, projection, and
  audit responsibilities separate.
- Offline skill eval matrix support for repeatable skill quality checks.
- Codex active view projection spec for moving Codex from a full registry mirror
  to an explicit active runtime view.

### Fixed

- Skill eval now fails when cases fail, making quality gates actionable instead
  of silently passing incomplete runs.
- Workspace status and target flows now account for configurable adapter
  behavior.

## [0.1.4] - 2026-06-26

### Added

- Portable skill lint checks and inventory discovery commands for stricter skill
  validation before projection.
- Source provenance lockfiles and skill policy checks to make local skill
  changes auditable before they reach agent runtimes.
- Durable `loom plan use` / `loom apply` workflows for retry-safe skill setup
  plans.

### Changed

- Refactored large skill command surfaces into focused modules without changing
  the public lifecycle contract.
- Trimmed release binary size and kept the release performance budget current.

### Fixed

- Source import now skips Git metadata instead of copying nested repository
  internals into managed skill sources.
- Durable plan apply no longer echoes idempotency keys and handles replay
  failures more safely.

## [0.1.3] - 2026-06-04

### Added

- `loom skill diagnose <skill>` for read-only skill health reports covering
  source files, Git drift, bindings, targets, projections, pending queue state,
  and related registry operations.
- Panel Diagnose tab on skill detail pages, including loading/error handling and
  refresh after lifecycle mutations.

### Fixed

- Diagnose reports now surface pending queue read problems and Git/source drift
  read failures instead of silently treating them as healthy.
- Projection-only/orphaned skills now report missing targets when no rule covers
  the projection target.

## [0.1.2] - 2026-06-01

### Added

- Skill trash, history timeline, realtime save watch mode, and registry backup
  export/restore workflows.
- Panel support for pending review follow-ups and the split handler module
  layout used by the V1 read/write surface.
- Launch readiness metadata: changelog, repository topics, issue templates, PR
  template, and README release-note links.

### Changed

- Refreshed Rust and Panel dependencies, including `uuid`, `serde_json`,
  TypeScript, Vite, Vitest, jsdom, and coverage tooling.
- Kept release archives aligned with the bundled Panel build and current
  dependency lockfiles.

### Fixed

- Addressed post-review follow-ups across the skill lifecycle, registry
  operations, and Panel routes merged after `v0.1.1`.

## [0.1.1] - 2026-05-31

### Added

- Rollback preview and impact analysis for safer registry recovery planning.
- `loom skill verify` with documentation for the skill source threat model.
- Agent preflight dry-run planning for high-risk automation flows.
- Panel pages and APIs for projections, doctor checks, orphan cleanup, lifecycle
  actions, activity, and operations history.
- V1 registry contracts for health envelopes, command audit, snapshot audit, and
  union skill read models.

### Changed

- Panel release builds now bundle the frontend into the Rust binary.
- Release trust guidance now covers archive checksums and GitHub attestation
  verification.
- CI now uses cargo-nextest for the Rust test suite.
- Dependency refreshes for Rust, GitHub Actions, and the Panel toolchain.

### Fixed

- Hardened audit-critical registry flows, rollback failure reporting, command
  audit recording, target path canonicalization, and secret redaction in command
  events.
- Improved CLI agent ergonomics, `--version` wiring, Panel failure visibility,
  and skill lifecycle documentation.

## [0.1.0] - 2026-04-30

### Added

- Initial public release archives for Loom.

[0.1.5]: https://github.com/majiayu000/loom/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/majiayu000/loom/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/majiayu000/loom/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/majiayu000/loom/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/majiayu000/loom/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/majiayu000/loom/releases/tag/v0.1.0
