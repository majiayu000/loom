# GH373 Tech Spec: Adapter Visibility Metadata

Issue: https://github.com/majiayu000/loom/issues/373
Product spec: `specs/GH373/product.md`
Status: Draft for implementation

## Current State

`src/agent_adapters.rs` currently has one adapter API constant set to `"1"`.
`AgentAdapter` stores identity, source, supported scopes, projection methods,
`SKILL.md`, capabilities, `default_skill_dirs`, and an optional config path.
External adapters deserialize into a v1 record and fail unless
`adapter_api == "1"`.

`resolve_agent_skill_dirs()` in `src/state/mod.rs` owns the current built-in
default directory table. `workspace status` and `workspace doctor` expose those
paths through adapter defaults, but they do not model preferred roots,
workspace-scope roots, symlink identity, config disable rules, or reload
semantics.

## Data Model

Add versioned adapter metadata while preserving v1 output compatibility.

```rust
pub(crate) const ADAPTER_API_V1: &str = "1";
pub(crate) const ADAPTER_API_V2: &str = "2";

pub(crate) struct AgentAdapter {
    pub adapter_api: String,
    pub id: String,
    pub source: String,
    pub supported_scopes: Vec<String>,
    pub projection_methods: Vec<String>,
    pub skill_entrypoint: String,
    pub capabilities: AdapterCapabilities,
    pub default_skill_dirs: Vec<PathBuf>,
    pub discovery_roots: Vec<AdapterDiscoveryRoot>,
    pub visibility: AdapterVisibility,
    pub reload: AdapterReload,
    pub config_path: Option<PathBuf>,
}
```

Recommended helper structs:

- `AdapterDiscoveryRoot`: `scope`, `path_template`, `role`, optional
  `source_env_var`, optional `priority`, optional unavailable reason.
- `ResolvedDiscoveryRoot`: resolved `path` plus the source root metadata used
  for diagnostics.
- `AdapterVisibility`: `follows_symlink_dirs`, `identity`, optional
  `config_file`, and `disable_rules`.
- `AdapterReload`: `strategy`, `hot_reload`, optional `notes`.

Use typed enums internally for values that have a closed set, but keep JSON
output as stable strings.

## External Adapter Loading

Deserialize the raw JSON first to read `adapter_api`, then route to v1 or v2
record parsing.

- v1: keep the existing strict schema and validation. Convert
  `default_skill_dirs` into discovery roots with role `legacy-default` and
  scope inferred from `supported_scopes` only when unambiguous. If a v1 adapter
  supports multiple scopes, duplicate each default into each supported scope
  with `legacy-default` role and mark the scope inference in diagnostics so
  target resolution does not pretend the adapter declared a preferred root. Map
  `capabilities.reload_required=true` to `reload.strategy=restart-required`
  and `hot_reload=false`; map `false` to `reload.strategy=no-reload-required`
  unless a v2 adapter overrides it. Keep `default_skill_dirs` in output.
- v2: require `discovery_roots` when `automatic_discovery` is true. Expand
  `~/` and `${CODEX_HOME:-...}`-style defaults only when a command resolves a
  root. Keep `<workspace>` placeholders templated until request-time selection
  so project-scope roots can use the command workspace instead of the registry
  root. Reject empty paths, unsupported roles, unsupported scopes, unsupported
  visibility identities, and unsupported reload strategies.
- unsupported API: preserve the existing error envelope before partial adapter
  registration: top-level `ADAPTER_INVALID` with
  `error.details.reason=ADAPTER_API_UNSUPPORTED`.

Unknown fields:

- v1 remains strict, matching `docs/schemas/agent-adapter-v1.schema.json`.
  Add explicit unknown-field rejection in the v1 loader, because serde currently
  ignores unknown keys unless the record type denies them.
- v2 may allow unknown fields only when they are ignored by serde into a
  top-level `extensions` object or are explicitly documented by schema.

## Built-In Adapters

Represent built-ins through the same v2 data path used by external v2 records.

Codex should include:

- `scope=user`, `path=~/.agents/skills`, role
  `preferred-cross-client`.
- `scope=user`, `path=$CODEX_SKILLS_DIR`, role `env-override`, source env var
  `CODEX_SKILLS_DIR`.
- `scope=user`, `path=${CODEX_HOME:-~/.codex}/skills`, role `legacy`.
- `scope=project`, `path=<workspace>/.agents/skills`, role
  `project-cross-client`.
- visibility identity is projection-method aware: symlink projections may use
  `canonical-skill-md-path`, while copy/materialize projections use the runtime
  target `SKILL.md` path.
- config file `${CODEX_HOME:-~/.codex}/config.toml`.
- disable rule `skills.config.path`; skill names are display/collision
  diagnostics only for Codex.
- reload strategy `new-session-recommended`, `hot_reload=false`.

Claude metadata may start with the existing default roots plus reload and scope
metadata that Loom can verify locally. Do not invent plugin or enterprise path
facts that the implementation does not inspect.
For project-scope `loom use --agents claude` defaults, preserve the existing
project target root contract such as `<registry-root>/targets/project/claude/skills`
until a richer Claude adapter project root is specified. Do not fall back to the
global home Claude directory for project-scope use.

Built-in adapters must remain loadable when `HOME` and `USERPROFILE` are
missing. In that case user roots are marked unavailable for diagnostics, but
read commands such as `workspace status` and `target list` must not fail during
adapter registry construction.

Every built-in adapter must preserve its existing default discovery paths in
the v2 `discovery_roots` list. Legacy `default_skill_dirs` output is derived
from resolved discovery roots that are eligible for automatic scanning, rather
than maintained as a separate source of truth. Project-scoped roots such as
`<workspace>/.agents/skills` are not fed into `workspace init --scan-existing`
unless the consumer explicitly asks for project-scope scanning.

## Adapter-Driven Target Resolution

Add one shared helper, owned by the adapter or visibility module:

```rust
pub(crate) fn preferred_discovery_root(
    adapter: &AgentAdapter,
    scope: &str,
    workspace: &Path,
) -> Result<ResolvedDiscoveryRoot, CommandFailure>
```

The helper should:

1. Filter discovery roots by requested scope.
2. Prefer roles in a deterministic order, starting with
   `preferred-cross-client` for user scope and `project-cross-client` for
   project scope.
3. Fall back to explicit priority or declaration order.
4. Resolve `path_template` with the command workspace only after selecting the
   root.
5. Return a structured adapter error when no root matches or the selected root
   is unavailable.

Supported root roles in v2 are:

```text
preferred-cross-client
project-cross-client
legacy
legacy-default
env-override
manual
```

External v2 adapters must reject unsupported role values instead of silently
falling back to priority or declaration order.

For Codex user-scope target selection, `preferred-cross-client`
(`~/.agents/skills`) remains the default write target. `CODEX_SKILLS_DIR` is a
preserved discovery override for status/doctor compatibility and can be selected
only by explicit target configuration or future policy; it must not silently
displace the documented preferred active-view root.

Supported v2 visibility identities:

```text
canonical-skill-md-path
directory-path
adapter-defined
```

Supported v2 reload strategies:

```text
no-reload-required
new-session-recommended
restart-required
unknown
```

Commands that currently ask `resolve_agent_skill_dirs()` for Codex or Claude
paths should call this helper unless they need legacy status output only.
`target add` and `loom use --target-root` are excluded from inferred root
selection because they register or use explicit caller-supplied paths.
Adapter-driven selection applies only to default-target helpers, diagnostics,
future activation flows, and `loom use` calls without an explicit target root.
Durable `plan use` must store the resolved target root and guards before review
so `apply` does not re-resolve a different root after adapter config or
environment changes.

Supported visibility disable rules:

```text
skills.config.path
adapter-defined
```

External v2 adapters must reject unsupported disable rule values unless they are
namespaced as adapter-defined extensions with documented semantics.

## JSON Output

`adapters_json()` should keep existing fields and add:

```json
{
  "adapter_api": "2",
  "discovery_roots": [
    {"scope": "user", "path": "/Users/example/.agents/skills", "role": "preferred-cross-client"}
  ],
  "visibility": {
    "follows_symlink_dirs": true,
    "identity": "canonical-skill-md-path",
    "config_file": "/Users/example/.codex/config.toml",
	    "disable_rules": ["skills.config.path"]
  },
  "reload": {
    "strategy": "new-session-recommended",
    "hot_reload": false
  }
}
```

## Schema And Docs

Add `docs/schemas/agent-adapter-v2.schema.json` and update
`docs/AGENT_ADAPTERS.md` with:

- v1 compatibility notes.
- v2 record example.
- supported placeholders.
- supported root roles.
- visibility identity and disable rule meanings.
- reload strategy meanings.

## Tests

Add focused tests covering:

1. Existing v1 fixture still loads.
2. Built-in Codex output includes all required roots and visibility metadata.
3. External v2 fixture loads and expands roots deterministically.
4. Unsupported `adapter_api` preserves top-level `ADAPTER_INVALID` and reports
   `ADAPTER_API_UNSUPPORTED` in `error.details.reason`.
5. Duplicate adapter id fails before mixed output is returned.
6. Adapter-driven default-target resolution chooses preferred user and project
   roots without changing explicit `target add --path` behavior.
7. Commands consuming inferred target roots no longer hard-code Codex path
   constants outside the adapter or visibility module.
8. V1 adapter files with unknown fields fail to load.
9. `workspace init --scan-existing` uses only scan-eligible legacy defaults and
   does not import project roots by accident.

## Verification

```bash
git diff --check
cargo test --test workspace_init
cargo test --test cli_surface
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #373` for the first implementation PR unless it satisfies every
acceptance criterion in the issue. If the first PR only adds internal metadata
and schema without command integration, explicitly call out the remaining
command integration tasks.
