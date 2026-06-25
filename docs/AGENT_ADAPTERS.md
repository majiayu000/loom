# Agent Adapter Protocol

Loom keeps the built-in `AgentKind` CLI values for stable write commands, and
loads external adapter records for discovery/read surfaces.

## Locations

External adapters are JSON files loaded from:

1. `<registry-root>/adapters/*.json`
2. `LOOM_ADAPTER_PATH`, using the platform path-list separator. Each entry may
   be a JSON file or a directory containing `*.json` files.

The schema is `docs/schemas/agent-adapter-v1.schema.json`.

## Record

```json
{
  "adapter_api": "1",
  "id": "fixture-agent",
  "supported_scopes": ["project"],
  "projection_methods": ["copy", "symlink"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {
    "automatic_discovery": true,
    "explicit_invocation": true,
    "reload_required": false
  },
  "default_skill_dirs": ["/opt/fixture-agent/skills"],
  "health_checks": ["directory_exists"]
}
```

For v1, Loom only accepts `SKILL.md` as the skill entrypoint. Invalid adapter
records return `ADAPTER_INVALID` with a structured `details.reason`, and
`workspace init --scan-existing` validates adapters before writing registry
state.

## Source Display

CLI and Panel target read models include `agent_source`:

- `built-in` for built-in adapters.
- `external` for configured external adapters.
- `unknown` when a stored target references an agent id with no loaded adapter.
