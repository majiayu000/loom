# Changelog

All notable public release changes are tracked here. Loom also publishes release
archives, checksums, and provenance details on GitHub Releases.

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

[0.1.1]: https://github.com/majiayu000/loom/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/majiayu000/loom/releases/tag/v0.1.0
