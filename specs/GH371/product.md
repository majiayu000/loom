# GH371 Product Spec: Runtime Dependency And MCP Readiness

Issue: https://github.com/majiayu000/loom/issues/371
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

让用户在激活或运行 skill 前知道 runtime dependency 是否就绪：

```bash
loom skill deps <skill> [--agent <agent>] [--workspace <path>] [--json]
```

The readiness report should be reused by future `skill inspect`, `skill diagnose`, `skill lint --quality`, activation gates, and Panel detail pages.

## Users

1. Skill user: wants to know why a visible skill will still fail at runtime.
2. Skill author: wants to declare required tools, env vars, network expectations, and MCP servers.
3. Agent: needs read-only structured next actions before trying to run or activate a skill.

## Declaration Sources

Read dependencies in precedence order:

1. `loom.skill.toml`:

```toml
requires_tools = ["git", "jq", "uv"]
requires_mcp = ["github", "filesystem"]
requires_env = ["GITHUB_TOKEN"]
network = "optional"
```

2. Portable `SKILL.md` frontmatter:

```yaml
compatibility: Requires git, jq, Python 3.12+, and access to GitHub MCP.
metadata:
  loom.requires_tools: git,jq,python
  loom.requires_mcp: github
  loom.network: optional
```

3. Script shebangs and script content heuristics.
4. `agents/openai.yaml` or other agent-specific metadata when present.

## Non-Goals

1. No network calls in the first implementation.
2. No MCP server installation or config mutation; #386 owns provisioning.
3. No secret value printing.
4. No shell interpolation for version checks.
5. No hard failure for unsupported agent MCP introspection; return `unknown`.

## Output Contract

```json
{
  "skill": "fixflow",
  "dependencies": {
    "tools": [
      {"name": "git", "required": true, "found": true, "version": "2.45.0"},
      {"name": "jq", "required": true, "found": false, "install_hint": "brew install jq"}
    ],
    "mcp": [
      {"name": "github", "required": true, "configured": false, "enabled": false}
    ],
    "env": [
      {"name": "GITHUB_TOKEN", "required": false, "present": false, "redacted": true}
    ],
    "network": {"required": "optional", "allowed_by_policy": false}
  },
  "ready": false,
  "next_actions": [
    "install jq",
    "configure github MCP server",
    "review network policy"
  ]
}
```

## Behavior Invariants

1. `skill deps` is read-only.
2. Required missing tools set `ready=false`.
3. Required missing MCP config sets `ready=false`; unsupported MCP detection returns `unknown` and a next action.
4. Env vars report presence only; values are never printed.
5. Tool version checks use argv arrays, a timeout, and no shell.
6. Network expectation is inferred but not tested by making network calls.
7. Missing declarations return empty dependency lists with `ready=true` and maybe quality warning, not fabricated dependencies.

## Checks

Tools:

1. Find executables on `PATH`.
2. Optionally run `<tool> --version` with timeout.
3. Return install hints for common tools when known.

MCP:

1. Detect configured MCP servers for the requested agent when possible.
2. Distinguish `configured`, `enabled`, `reachable`, and `authenticated` only when evidence exists.
3. Report `unknown` for unsupported agents.

Environment:

1. Required env var presence only.
2. Optional vs required support.
3. Redacted always true for env entries.

Network:

1. Infer from metadata and scripts.
2. Cross-check policy profile where available.

## Acceptance Criteria

1. `loom skill deps <skill>` reports tools, MCP servers, env vars, and network expectations.
2. Missing required tools cause `ready=false` with actionable hints.
3. Missing required MCP config causes `ready=false`.
4. Unsupported MCP detection returns `unknown`, not a false pass.
5. Env var values are never printed.
6. Checks are reusable by `skill inspect`, `skill diagnose`, and `skill lint --quality`.
7. Tests cover declared tools, inferred tools from scripts, missing tools, env redaction, MCP missing/configured states, unsupported agents, network inference, and no-dependency skills.
