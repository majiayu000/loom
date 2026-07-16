# Agent Adapter Protocol

Loom keeps the built-in `AgentKind` CLI values for stable write commands, and
loads external adapter records for discovery/read surfaces.

## Locations

External adapters are JSON files loaded from:

1. `<registry-root>/adapters/*.json`
2. `LOOM_ADAPTER_PATH`, using the platform path-list separator. Each entry may
   be a JSON file or a directory containing `*.json` files.

Schemas:

- `docs/schemas/agent-adapter-v1.schema.json`
- `docs/schemas/agent-adapter-v2.schema.json`

## Fidelity

Every adapter row emitted by `loom workspace status --json` includes required
output-only metadata `fidelity: "verified" | "generic"`.

- `verified` means the built-in discovery, visibility, and reload metadata is
  backed by agent-specific evidence and targeted tests.
- `generic` means the adapter uses conservative fallback metadata. A verified
  adapter must never contain a `legacy-default` discovery root.

External v1 and v2 input schemas do not accept a fidelity assertion. External
records always resolve to `generic` until Loom defines a separately validated
evidence mechanism, so existing external validation behavior is unchanged.

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
  "default_skill_dirs": ["/opt/fixture-agent/skills"]
}
```

For v1, Loom only accepts `SKILL.md` as the skill entrypoint. Invalid adapter
records return `ADAPTER_INVALID` with a structured `details.reason`, and
`workspace init --scan-existing` validates adapters before writing registry
state.

## v2 Metadata

Adapter v2 preserves the v1 identity and capability fields, then adds
discovery, visibility, and reload metadata:

```json
{
  "adapter_api": "2",
  "id": "fixture-agent",
  "supported_scopes": ["user", "project"],
  "projection_methods": ["copy", "symlink"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {
    "automatic_discovery": true,
    "explicit_invocation": true,
    "reload_required": true
  },
  "discovery_roots": [
    {
      "scope": "user",
      "path": "/opt/fixture-agent/skills",
      "role": "preferred-cross-client"
    },
    {
      "scope": "project",
      "path": "<workspace>/.fixture-agent/skills",
      "role": "project-cross-client",
      "scan_eligible": false
    }
  ],
  "visibility": {
    "follows_symlink_dirs": true,
    "identity_by_projection_method": {
      "symlink": "canonical-skill-md-path",
      "copy": "runtime-skill-md-path"
    },
    "disable_rules": ["adapter-defined"]
  },
  "reload": {
    "strategy": "restart-required",
    "hot_reload": false
  }
}
```

Supported discovery root roles are `preferred-cross-client`,
`project-cross-client`, `legacy`, `legacy-default`, `env-override`, and
`manual`. `workspace status` and `workspace doctor` report all roots with
availability. `workspace init --scan-existing` only scans roots marked
`scan_eligible`.

Supported visibility identities are `canonical-skill-md-path`,
`runtime-skill-md-path`, `directory-path`, and `adapter-defined`. Supported
disable rules are `skills.config.path` and `adapter-defined`.

External v2 reload strategies remain `no-reload-required`,
`new-session-recommended`, `restart-required`, and `unknown`. Built-in adapters
may additionally report an evidence-backed strategy; Gemini CLI uses
`in-session-command`.

Built-in Codex metadata declares `~/.agents/skills` as the preferred user root,
`${CODEX_HOME:-~/.codex}/skills` as a legacy user root, and
`<workspace>/.agents/skills` as the project root. Codex visibility is modeled
by canonical `SKILL.md` path for symlink projections and
`skills.config.path` disable rules. Reload is reported as
`new-session-recommended`; Loom does not claim an existing Codex session has
hot-reloaded changed skills.

Built-in Claude metadata declares `~/.claude/skills` as the preferred user
root, `${CLAUDE_HOME:-~/.claude}/skills` as a legacy user root, and
`<workspace>/.claude/skills` as the project root. Claude visibility is modeled
by canonical `SKILL.md` path for symlink projections and adapter-defined
disable rules. Reload is reported as `new-session-recommended`; Loom does not
claim an existing Claude session has hot-reloaded changed skills.

Built-in Gemini CLI metadata declares `~/.agents/skills` and
`~/.gemini/skills` as user roots, plus matching project roots. Gemini CLI loads
the `.agents` alias after `.gemini` within each tier, so Loom assigns the alias
the higher priority. Symlink identity follows the canonical `SKILL.md`; copy
and materialize identity use the runtime path. `~/.gemini/settings.json` and
adapter-defined `skills.disabled` behavior are surfaced without claiming Loom
can rewrite that setting. Reload is `in-session-command` with `/skills reload`.
Evidence: [official discovery docs](https://geminicli.com/docs/cli/creating-skills/),
[official command reference](https://geminicli.com/docs/reference/commands/),
[official settings reference](https://geminicli.com/docs/reference/configuration/),
and the official
[discovery implementation](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/skills/skillManager.ts).

## Source Display

CLI and Panel target read models include `agent_source`:

- `built-in` for built-in adapters.
- `external` for configured external adapters.
- `unknown` when a stored target references an agent id with no loaded adapter.
