# Supported Agents

Loom defines a fixed list of agent kinds. Each kind has built-in adapter
metadata for default discovery roots that `loom workspace init --scan-existing`
and the `workspace doctor` agent inventory check probe under `$HOME`.

## Built-in agent kinds

| Agent kind (`AgentKind`) | CLI value (`--agent`) | Default skill directory |
|---|---|---|
| `Claude` | `claude` | `$HOME/.claude/skills` |
| `Codex` | `codex` | `$HOME/.agents/skills` preferred; `${CODEX_HOME:-$HOME/.codex}/skills` legacy |
| `Cursor` | `cursor` | `$HOME/.cursor/skills` |
| `Windsurf` | `windsurf` | `$HOME/.windsurf/skills` |
| `Cline` | `cline` | `$HOME/.cline/skills` |
| `Copilot` | `copilot` | `$HOME/.github/copilot/skills` |
| `Aider` | `aider` | `$HOME/.aider/skills` |
| `Opencode` | `opencode` | `$HOME/.opencode/skills` |
| `GeminiCli` | `gemini-cli` | `$HOME/.gemini/skills` |
| `Goose` | `goose` | `$HOME/.config/goose/skills` |

The built-in source of truth is `src/cli.rs` (the `AgentKind` enum) plus
`src/agent_adapters.rs`. External adapters use the protocol in
`docs/AGENT_ADAPTERS.md`.

Codex has active-view semantics beyond a single default path. See
`docs/CODEX_SKILL_VISIBILITY.md` for the preferred user root, legacy root,
project `.agents/skills` search chain, config disables, and restart guidance.

## How `--scan-existing` uses this list

`loom workspace init --scan-existing` iterates `DEFAULT_SCAN_AGENTS`, resolves
each default skill directory under the caller's `$HOME`, and registers any
directory that exists as an `observed` target. Missing directories are
reported under `skipped` with reason `does-not-exist`; non-directories are
reported with reason `not-a-directory`.

## How `workspace doctor` reports the inventory

`loom workspace doctor` adds an informational check
(`section=agents`, `id=agent_skill_inventory`, `severity=info`) that lists,
for every built-in agent kind, the resolved default path, whether the path
exists, and how many registered targets currently point at that path.

The check is informational only and does not affect the overall `healthy`
boolean. When `HOME` is unset or empty, the inventory is reported with
`home_set=false` and `total=0` instead of failing the command.

The same payload is also exposed under
`data.checks.agent_skill_dirs` for callers that read the legacy nested
`checks` object.

## Registering a path outside the default list

If your environment stores skills outside the canonical default (for example,
an XDG override, a custom team layout, or an agent kind that Loom does not
yet model), register the path explicitly instead of relying on
`--scan-existing`:

```bash
loom --json target add \
  --agent claude \
  --path /custom/path/to/skills \
  --ownership observed
```

Pick the `--agent` value that most closely matches the target tool. The
registered target's `path` is the absolute path on disk; the agent kind is a
label used for routing through bindings.

## Adding a new agent kind

Adding a new built-in agent kind is a coordinated change:

1. Add the variant to `AgentKind` in `src/cli.rs`, including the kebab-case
   serde rename so it round-trips through the JSON envelope.
2. Add the built-in adapter metadata in `src/agent_adapters.rs`.
3. Extend `agent_kind_as_str` in `src/commands/helpers.rs`.
4. Update this document and the `Quick Start` block of `README.md`.
5. Cover the new variant in `tests/cli.rs` (serde round-trip) and add a
   `workspace doctor` assertion in `tests/doctor.rs` if the new path needs an
   inventory entry assertion.
