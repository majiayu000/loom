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

- `AdapterDiscoveryRoot`: `scope`, expanded `path`, `role`, optional
  `source_env_var`, optional `priority`.
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
  scope inferred from `supported_scopes` only when unambiguous. Keep
  `default_skill_dirs` in output.
- v2: require `discovery_roots` when `automatic_discovery` is true. Expand
  `~/`, `${CODEX_HOME:-...}`-style defaults, and `<workspace>` placeholders
  through one shared path-expansion helper. Reject empty paths, empty roles,
  unsupported scopes, unsupported visibility identities, and unsupported reload
  strategies.
- unsupported API: return `ADAPTER_API_UNSUPPORTED` before partial adapter
  registration.

Unknown fields:

- v1 remains strict, matching `docs/schemas/agent-adapter-v1.schema.json`.
- v2 may allow unknown fields only when they are ignored by serde into a
  top-level `extensions` object or are explicitly documented by schema.

## Built-In Adapters

Represent built-ins through the same v2 data path used by external v2 records.

Codex should include:

- `scope=user`, `path=~/.agents/skills`, role
  `preferred-cross-client`.
- `scope=user`, `path=${CODEX_HOME:-~/.codex}/skills`, role `legacy`.
- `scope=project`, `path=<workspace>/.agents/skills`, role
  `project-cross-client`.
- visibility identity `canonical-skill-md-path`.
- config file `${CODEX_HOME:-~/.codex}/config.toml`.
- disable rules `skills.config.path` and `skills.config.name`.
- reload strategy `new-session-recommended`, `hot_reload=false`.

Claude metadata may start with the existing default roots plus reload and scope
metadata that Loom can verify locally. Do not invent plugin or enterprise path
facts that the implementation does not inspect.

## Adapter-Driven Target Resolution

Add one shared helper, owned by the adapter or visibility module:

```rust
pub(crate) fn preferred_discovery_root(
    adapter: &AgentAdapter,
    scope: &str,
    workspace: &Path,
) -> Result<&AdapterDiscoveryRoot, CommandFailure>
```

The helper should:

1. Filter discovery roots by requested scope.
2. Prefer roles in a deterministic order, starting with
   `preferred-cross-client` for user scope and `project-cross-client` for
   project scope.
3. Fall back to explicit priority or declaration order.
4. Return a structured adapter error when no root matches.

Commands that currently ask `resolve_agent_skill_dirs()` for Codex or Claude
paths should call this helper unless they need legacy status output only.

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
    "disable_rules": ["skills.config.path", "skills.config.name"]
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
4. Unsupported `adapter_api` fails with `ADAPTER_API_UNSUPPORTED`.
5. Duplicate adapter id fails before mixed output is returned.
6. Adapter-driven target resolution chooses preferred user and project roots.
7. Commands consuming target roots no longer hard-code Codex path constants
   outside the adapter or visibility module.

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
