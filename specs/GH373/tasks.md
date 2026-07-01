# GH373 Tasks: Adapter Visibility Metadata

Issue: https://github.com/majiayu000/loom/issues/373
Product spec: `specs/GH373/product.md`
Tech spec: `specs/GH373/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement adapter v2 metadata and the first adapter-driven target resolution
slice:

```text
v1 compatibility + built-in v2 metadata + external v2 schema/load + preferred root helper
```

Do not implement:

```text
Codex config repair, activation rollback, eval harness behavior, marketplace/provider adapters
```

## Tasks

- [ ] `SP373-T001` Owner: adapter-model | Done when: `AgentAdapter` can represent v1-compatible defaults plus v2 `discovery_roots`, `visibility`, and `reload` metadata | Verify: `cargo test --test workspace_init`
- [ ] `SP373-T002` Owner: adapter-loader | Done when: external v1 adapters still load with unknown-field rejection, external v2 adapters parse through a versioned loader, and unsupported APIs preserve `ADAPTER_INVALID` with `ADAPTER_API_UNSUPPORTED` as the structured reason | Verify: `cargo test --test workspace_init`
- [ ] `SP373-T003` Owner: built-ins | Done when: built-in Codex metadata includes preferred user, legacy user, project roots, path-based config visibility, scan-eligible legacy defaults, and reload semantics | Verify: `cargo test --test workspace_init`
- [ ] `SP373-T004` Owner: target-resolution | Done when: `loom use`, diagnostics, and activation-facing default-target helpers choose preferred roots from adapter metadata while `target add --path` keeps the caller's explicit path | Verify: `cargo test --test cli_surface`
- [ ] `SP373-T005` Owner: docs-schema | Done when: `docs/schemas/agent-adapter-v2.schema.json` and `docs/AGENT_ADAPTERS.md` document v1 compatibility, v2 fields, roles, visibility, and reload semantics | Verify: `git diff --check`
- [ ] `SP373-T006` Owner: regression | Done when: tests cover v1 compatibility, v2 Codex roots, external v2 load, unsupported API, duplicate ids, and adapter-driven target resolution | Verify: `cargo test && cargo check --workspace --all-targets --all-features`

### SP373-T1: Extend Adapter Model

Owner: implementation

Files:

- `src/agent_adapters.rs`
- `src/state/mod.rs`

Done when:

- Adapter model stores discovery roots with scope, path, role, and deterministic
  order.
- Adapter model stores visibility metadata for symlink identity, config file,
  and disable rules.
- Adapter model stores reload metadata.
- Existing `default_skill_dirs` output remains available for current
  consumers.

Verify:

```bash
cargo test --test workspace_init
```

### SP373-T2: Version External Adapter Loading

Owner: implementation
Depends on: SP373-T1

Files:

- `src/agent_adapters.rs`
- `tests/workspace_init.rs`

Done when:

- Loader dispatches by `adapter_api` before deserializing the full record.
- Existing v1 fixture remains valid.
- V1 fixtures with unknown fields fail to load with a structured adapter error.
- v2 fixture accepts discovery roots, visibility, and reload metadata.
- Unsupported APIs preserve top-level `ADAPTER_INVALID` and report
  `ADAPTER_API_UNSUPPORTED` in details.
- Duplicate adapter ids still fail before returning mixed adapter output.

Verify:

```bash
cargo test --test workspace_init
```

### SP373-T3: Populate Built-In Adapter V2 Metadata

Owner: implementation
Depends on: SP373-T1

Files:

- `src/agent_adapters.rs`
- `src/state/mod.rs`
- `tests/workspace_init.rs`

Done when:

- Codex user root prefers `~/.agents/skills`.
- Codex legacy user root includes `${CODEX_HOME:-~/.codex}/skills`.
- Codex project root includes `<workspace>/.agents/skills`.
- Codex visibility includes canonical `SKILL.md` path identity and
  `skills.config.path` disable rules.
- Legacy defaults used by `workspace init --scan-existing` exclude project roots
  unless project-scope scanning is requested explicitly.
- Reload metadata reports `new-session-recommended` and no hot reload.

Verify:

```bash
cargo test --test workspace_init
```

### SP373-T4: Add Adapter-Driven Root Selection

Owner: implementation
Depends on: SP373-T2, SP373-T3

Files:

- `src/agent_adapters.rs`
- command modules that currently select agent target directories directly

Done when:

- A shared helper chooses a preferred discovery root by adapter and scope.
- User scope prefers role `preferred-cross-client`.
- Project scope prefers role `project-cross-client`.
- Missing scoped roots return a structured adapter error instead of falling
  back silently.
- `workspace doctor`, current `skill show`/`skill diagnose`, future inspect, and
  activation-facing target resolution stop duplicating Codex path constants.
- `target add --path` keeps registering the explicit path supplied by the user
  and does not infer a different adapter root.

Verify:

```bash
cargo test --test cli_surface
cargo test --test workspace_init
```

### SP373-T5: Add Schema And Docs

Owner: documentation
Depends on: SP373-T2

Files:

- `docs/schemas/agent-adapter-v2.schema.json`
- `docs/AGENT_ADAPTERS.md`
- `specs/GH373/*`

Done when:

- Adapter v2 schema documents required and optional fields.
- Docs explain v1 compatibility, placeholders, root roles, visibility rules,
  and reload strategies.
- Examples do not claim unsupported agent behavior.

Verify:

```bash
git diff --check
```

### SP373-T6: Add Regression Coverage

Owner: testing
Depends on: SP373-T1, SP373-T2, SP373-T3, SP373-T4, SP373-T5

Done when:

- Tests cover every GH373 acceptance criterion.
- Full check and test suites pass.
- No test assertion is weakened to fit the implementation.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #373` unless the implementation PR satisfies every acceptance
criterion. Do not use `Fixes #373` for a schema-only or metadata-only slice.
