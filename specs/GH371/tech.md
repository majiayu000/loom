# GH371 Tech Spec: Runtime Dependency And MCP Readiness

Issue: https://github.com/majiayu000/loom/issues/371
Product spec: `specs/GH371/product.md`
Status: Draft for implementation

## Design Summary

Add a read-only dependency readiness module:

1. Parse dependency declarations from `loom.skill.toml`, `SKILL.md`, scripts, and agent metadata.
2. Probe local tools with safe argv + timeout.
3. Inspect local MCP config when possible without network.
4. Return one structured readiness report.
5. Expose the report through `skill deps` and reusable helpers.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs`, new `src/cli/deps.rs` |
| command dispatch | `src/commands/mod.rs` |
| readiness module | new `src/commands/skill_deps.rs` |
| manifest parsing | reuse/extend `src/commands/skill_new.rs` manifest parser or extract shared parser |
| lint/inspect/diagnose integration | `src/commands/skill_lint/sections.rs`, future #366 inspect, #368 diagnose |
| tests | new `tests/skill_deps.rs`, maybe extend `tests/skill_new_cli.rs` |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH371/*` |

## Declaration Parser

Create a shared dependency declaration model:

```rust
struct SkillDependencyDeclarations {
    tools: Vec<DependencyRequirement>,
    mcp: Vec<DependencyRequirement>,
    env: Vec<DependencyRequirement>,
    network: NetworkRequirement,
    sources: Vec<DeclarationSource>,
}
```

Sources:

1. `loom.skill.toml`: exact fields win.
2. `SKILL.md` frontmatter metadata keys with `loom.` prefix.
3. compatibility text heuristic.
4. script shebang/content heuristic.
5. agent metadata files.

Malformed declaration files should produce findings, not panic or silently ignore.

## Tool Probe

Use:

1. PATH lookup for executable.
2. Optional version command with argv array.
3. Timeout.
4. Captured stdout/stderr bounded and redacted.

Do not use shell strings.

## MCP Probe

Initial detection can be local config only:

1. For Codex, inspect known config location when implemented by #368/#373.
2. For Claude or other agents, inspect only documented local config paths when safe.
3. If config shape is unsupported, set `configured="unknown"`.
4. Do not connect to MCP server or network in v1.

`#386` owns install/config apply.

## Env Probe

For each env requirement:

```json
{"name": "GITHUB_TOKEN", "required": true, "present": true, "redacted": true}
```

No value, length, prefix, or hash should be printed by default.

## Readiness Decision

`ready=false` when:

1. required tool missing;
2. required MCP explicitly missing/unconfigured;
3. required env var missing;
4. network required but selected policy disallows network.

`ready=null` or `unknown` when evidence is insufficient, especially unsupported MCP detection.

## Test Plan

Focused tests:

1. declared tools from `loom.skill.toml`.
2. declared MCP and env from manifest.
3. frontmatter metadata declarations.
4. script shebang infers python/node/bash.
5. missing required tool returns ready false.
6. env present/missing with values redacted.
7. MCP configured/missing/unknown states.
8. network inference from script and metadata.
9. no-dependency skill returns ready true.
10. version probe timeout does not hang.

Suggested commands:

```bash
git diff --check
cargo test --test skill_deps
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
```

For a spec-only PR:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH371-dependency-readiness-spec/specs/GH371
```

## Rollback

Rollback can remove:

1. deps CLI args and command module;
2. shared dependency parser/prober;
3. lint/inspect/diagnose integration;
4. tests and docs updates.

No registry migration should be required because the first implementation is read-only.

## Risks

1. Secret leakage through env probing. Mitigation: presence only.
2. Command injection through version probes. Mitigation: argv arrays and no shell.
3. Hanging version commands. Mitigation: timeout.
4. False readiness for MCP. Mitigation: unknown when unsupported; no fabricated pass.
