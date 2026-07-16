# Module Ceiling Signal Report

## Contract

`make module-ceiling` runs `scripts/module-ceiling.sh` locally and in the CI
`verify` job after Rust lint.

- Production Rust files above 800 lines fail.
- Production Rust files from 701 through 800 lines emit warnings.
- `tests/` path components, sibling `tests.rs` modules, `*_tests.rs`, and
  generated-file patterns are excluded.
- Inline `#[cfg(test)]` blocks inside production files still count toward the
  complete physical-file size.
- Allowlist entries use `path<TAB>baseline_lines<TAB>issue-ref`.
- An allowlisted file above 800 lines must exactly match its reviewed baseline;
  any increase or decrease fails until the entry is explicitly reviewed and
  updated. It also fails if it disappears, becomes excluded, or falls to 800
  lines or below without its stale entry being removed.
- Rust-file and source-directory symlinks are rejected instead of being silently
  skipped.

The guard implementation and negative fixtures are checked by:

```bash
make module-ceiling module-ceiling-test
```

## Current Hard-Ceiling Queue

Baseline: `bb9b738` (the implementation branch preserves the same three hard
violations).

| Path | Baseline | Split issue | Policy |
| --- | ---: | --- | --- |
| `src/commands/mcp/apply.rs` | 981 | #544 | split-on-touch |
| `src/commands/mcp.rs` | 878 | #545 | split-on-touch |
| `src/commands/skillset_activation.rs` | 856 | #546 | split-on-touch |

## Current Warning Band

The final production filter reports 18 files in the 701–800 warning band:

| Path | Lines |
| --- | ---: |
| `src/commands/codex_visibility.rs` | 800 |
| `src/commands/skill_deps.rs` | 799 |
| `src/commands/skill_diagnose.rs` | 798 |
| `src/commands/watch_cmds.rs` | 794 |
| `src/commands/backup_cmds.rs` | 787 |
| `src/commands/provider_cmds/locator.rs` | 785 |
| `src/cli.rs` | 774 |
| `src/commands/provision/apply.rs` | 770 |
| `src/commands/skill_eval.rs` | 745 |
| `src/commands/skill_authoring.rs` | 745 |
| `src/commands/telemetry/mod.rs` | 741 |
| `src/agent_adapters.rs` | 732 |
| `src/commands/skill_cmds/observed.rs` | 724 |
| `src/commands/provenance.rs` | 720 |
| `src/commands/skill_authoring_apply.rs` | 717 |
| `src/commands/projections.rs` | 713 |
| `src/commands/skill_inventory.rs` | 712 |
| `src/commands/skill_eval_harness/runner.rs` | 711 |

The allowlist may only shrink. A new entry requires a linked split issue and
explicit review; reducing a baseline while the file remains above 800 also
requires explicit review.
