# GH373 Product Spec: Adapter Visibility Metadata

Issue: https://github.com/majiayu000/loom/issues/373
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

Make agent adapters describe where skills are discovered, how visibility is
decided, and when agent reload is needed, so Loom can answer whether one skill
is visible to one target agent without relying on scattered hard-coded paths.

The user-visible outcome is a stable read model for commands such as
`workspace status`, `workspace doctor`, current `skill show`/`skill diagnose`,
future `skill inspect`, and future activation flows:

1. Which roots the agent scans.
2. Which root Loom should prefer for a requested scope.
3. Which config file or disable rules may hide an otherwise projected skill.
4. Whether a new agent session or reload is needed before the agent sees the
   skill.

## Scope For First PR

Implement the smallest adapter v2 slice:

- Keep external adapter v1 records loading without changes.
- Add an internal v2 representation for built-in adapters.
- Add optional v2 JSON adapter loading for external adapters.
- Expose `discovery_roots`, `visibility`, and `reload` metadata in adapter
  output.
- Route target directory selection through adapter metadata instead of
  duplicate hard-coded Codex and Claude path assumptions.
- Add schema and docs for adapter v2.

## Non-Goals

1. Do not repair Codex config disables; that belongs to #368.
2. Do not implement skill activation or rollback behavior; that belongs to
   #367 and the activation follow-up queue.
3. Do not implement eval harness behavior; that belongs to #369.
4. Do not add new marketplace, provider, or remote adapter behavior.
5. Do not remove v1 adapter support.
6. Do not claim Claude enterprise or plugin discovery is fully implemented
   unless it is backed by a documented adapter metadata field and tests.

## Behavior Invariants

1. Existing external v1 adapter files continue to load.
2. Unsupported `adapter_api` values preserve the existing adapter error
   envelope: top-level `ADAPTER_INVALID` with `details.reason` set to
   `ADAPTER_API_UNSUPPORTED`.
3. Duplicate adapter ids still fail before any command returns mixed metadata.
4. Unknown fields are accepted only for versions whose schema allows safe
   extension; v1 remains strict.
5. Adapter output preserves v1 fields while adding v2 metadata, so existing
   consumers do not lose `default_skill_dirs`, `capabilities`, or
   `config_path`.
6. Built-in Codex metadata includes user, legacy, and project discovery roots.
7. Built-in visibility metadata models Codex config disables by canonical
   `SKILL.md` path; skill names remain display/collision diagnostics only.
8. Activation-facing default-target helpers and `loom use` flows use the
   adapter's preferred discovery root only when the caller did not provide an
   explicit target root. Existing `target add --path <absolute-path>` and
   `loom use --target-root <path>` continue to use the caller-provided concrete
   directory and must not infer or replace it from adapter roots.
9. Durable `plan use` records the resolved adapter-selected target root and guard
   digests before review; apply must replay the reviewed root instead of
   re-resolving adapter/env defaults.
10. Reload metadata is descriptive only in this slice; commands may report it
   but must not silently mutate agent runtime state.

## Acceptance Criteria

1. `workspace status` adapter output includes `discovery_roots`,
   `visibility`, and `reload` metadata.
   `workspace doctor` also surfaces adapter discovery, visibility, and reload
   metadata or regression tests prove doctor consumes the same read model.
2. Built-in Codex adapter metadata includes `~/.agents/skills`,
   `${CODEX_HOME:-~/.codex}/skills`, and `<workspace>/.agents/skills` with
   stable roles.
3. `skill diagnose --agent codex`, current `skill show`, future inspect flows,
   and activation-facing default target resolution do not duplicate Codex skill
   path constants outside the adapter or visibility module.
4. External v1 adapters still load and return the same effective defaults.
5. External v2 adapters can define discovery roots, visibility, and reload
   metadata.
6. Unsupported adapter API values fail with a structured adapter error.
7. Tests cover v1 compatibility, v2 built-in Codex roots, unsupported adapter
   API, duplicate adapter id, and adapter-driven target resolution.
