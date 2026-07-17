# Supported Agents

Loom defines a fixed list of agent kinds. Each kind has built-in adapter
metadata for default discovery roots that `loom workspace init --scan-existing`
and the `workspace doctor` agent inventory check probe under `$HOME`.

## Built-in agent kinds

| Agent kind (`AgentKind`) | CLI value (`--agent`) | Fidelity | Default skill directory |
|---|---|---|---|
| `Claude` | `claude` | `verified` | `$HOME/.claude/skills` |
| `Codex` | `codex` | `verified` | `$HOME/.agents/skills` preferred; `${CODEX_HOME:-$HOME/.codex}/skills` legacy |
| `Cursor` | `cursor` | `generic` | `$HOME/.cursor/skills` |
| `Windsurf` | `windsurf` | `generic` | `$HOME/.windsurf/skills` |
| `Cline` | `cline` | `generic` | `$HOME/.cline/skills` |
| `Copilot` | `copilot` | `generic` | `$HOME/.github/copilot/skills` |
| `Aider` | `aider` | `generic` | `$HOME/.aider/skills` |
| `Opencode` | `opencode` | `generic` | `$HOME/.opencode/skills` |
| `GeminiCli` | `gemini-cli` | `verified` | `$GEMINI_CLI_HOME` (fallback `$HOME`) supplies `.agents/skills` then `.gemini/skills`; project aliases match; no custom skills-dir override |
| `Goose` | `goose` | `generic` | `$HOME/.config/goose/skills` |

The built-in source of truth is `src/cli.rs` (the `AgentKind` enum) plus
`src/agent_adapters.rs`. External adapters use the protocol in
`docs/AGENT_ADAPTERS.md`.

Codex has active-view semantics beyond a single default path. See
`docs/CODEX_SKILL_VISIBILITY.md` for the preferred user root, legacy root,
project `.agents/skills` search chain, config disables, and restart guidance.

Gemini CLI is verified against its official skill discovery and command
contracts. Its `.agents/skills` alias overrides the matching `.gemini/skills`
entry within the same user or project tier, disabled names are persisted under
`skills.disabled` with case-insensitive union semantics, and untrusted
workspaces cannot satisfy project-only visibility. Remote enterprise admin
policy is not represented by local settings, so Loom reports it as
unobservable instead of claiming visibility. `/skills reload` refreshes the
current session. `workspace init --scan-existing` and `workspace doctor`
inspect both user roots; Loom-managed `use` writes to the native `.gemini`
root to avoid sharing Codex's managed target. See the
[Gemini CLI skill docs](https://geminicli.com/docs/cli/creating-skills/),
[command reference](https://geminicli.com/docs/reference/commands/), and
[settings reference](https://geminicli.com/docs/reference/configuration/), plus
the [trusted-folders reference](https://geminicli.com/docs/cli/trusted-folders/).
The process environment fixes bootstrap user settings and trust locations;
only a workspace already trusted by that state may load a runtime dotenv
`GEMINI_CLI_HOME` for subsequent user-root discovery. Runtime dotenv loading
also honors effective `advanced.ignoreLocalEnv` and `advanced.excludedEnvVars`;
generic project dotenv redirects are rejected when `--ignore-env` could change
the selected file.

`generic` means Loom exposes a conservative fallback path without claiming
that discovery precedence, visibility disables, or reload behavior has been
verified against that agent. Generic adapters are not used as verified
visibility evidence.

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
