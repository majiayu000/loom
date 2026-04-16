<div align="center">
  <img src="./assets/loom-icon.svg" alt="Loom" width="120" />

  <h1>Loom</h1>

  <p><strong>The skill registry and projection control plane for AI coding agents.</strong></p>

  <p>
    <a href="https://github.com/majiayu000/loom/actions/workflows/ci.yml"><img src="https://github.com/majiayu000/loom/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust" />
    <a href="https://github.com/majiayu000/loom/stargazers"><img src="https://img.shields.io/github/stars/majiayu000/loom?style=flat" alt="Stars" /></a>
    <a href="docs/LOOM_COMPLETE_GUIDE_ZH.md"><img src="https://img.shields.io/badge/docs-中文-red.svg" alt="中文" /></a>
  </p>

  <p>
    <a href="#quick-start">Quick Start</a> ·
    <a href="#features">Features</a> ·
    <a href="#how-it-works">How It Works</a> ·
    <a href="#comparison">Comparison</a> ·
    <a href="#command-surface">CLI</a>
  </p>
</div>

---

## Why Loom?

AI coding agents (Claude Code, Codex, Cursor, Windsurf, …) all read skills from **different directories**. Keeping them in sync is either:

- **Manual**: `cp -R` or `ln -s` between `~/.claude/skills`, `~/.codex/skills`, repo-local `.claude/skills`, … — easy to drift, hard to roll back, impossible to audit.
- **One-way sync apps**: install skills from a central store, but no binding logic, no per-project matching, no version history, no replay when things go wrong.

**Loom treats skills like infrastructure**: a versioned registry (add → capture → save → snapshot → release → rollback → diff), projected onto one or many agent directories through explicit bindings (agent + profile + matcher + policy), with git-backed sync, replay, and audit trail. CLI-first for automation, Panel-assisted for visibility.

## Quick Start

```bash
# 1. Install from source
git clone https://github.com/majiayu000/loom.git
cd loom && cargo install --path .

# 2. Set up a registry directory (must be separate from the Loom tool repo)
mkdir -p ~/.loom-registry && cd ~/.loom-registry && git init

# 3. Register your Claude Code skills directory as a target
loom --root ~/.loom-registry target add \
  --agent claude --path "$HOME/.claude/skills" --ownership observed

# 4. Open the visual control panel
loom --root ~/.loom-registry panel        # → http://localhost:43117
```

Prefer the guided walkthrough? Run `./scripts/e2e-agent-flow.sh` for four real scenarios end-to-end.

## Features

- **🎯 Projection with three modes** — `symlink` / `copy` / `materialize`, per binding
- **🎚️ Ownership tiers** — `managed` (Loom writes) / `observed` (read-only) / `external` (hands-off)
- **🔗 Binding matchers** — route a skill to a target by `path-prefix`, `exact-path`, or `name`
- **📦 Profiles** — multiple config sets per agent (e.g. work/home Claude profiles)
- **🧬 Versioned lifecycle** — `add → capture → save → snapshot → release → rollback → diff`
- **🔁 Git-backed sync** — `sync push / pull / replay` between a team's registries
- **🛠️ Ops with audit** — `ops list / retry / purge` and `ops history diagnose / repair`
- **🛡️ Hard write guard** — refuses to write when `--root` points at the Loom tool repo itself
- **🖥️ CLI + Panel** — script anything from the CLI; diff and inspect from the React Panel
- **📤 JSON envelope** — every command speaks `--json` for machine consumption

## How It Works

```
┌───────────────────┐         ┌────────────────────┐
│   Skill Registry  │         │    Target Dirs     │
│  (your Git repo)  │         │                    │
│                   │         │  ~/.claude/skills  │
│   skills/*        │         │  ~/.codex/skills   │
│   versions/*      │ ──────▶ │  /repo/.claude/... │
│   bindings.json   │         │  …                 │
└─────────▲─────────┘         └──────────▲─────────┘
          │                              │
          │   capture / save / snapshot  │ projection
          │   (versioned lifecycle)      │ (symlink / copy / materialize)
          │                              │
┌─────────┴────────┐         ┌──────────┴──────────┐
│   `loom` CLI     │◀───────▶│   Loom Panel (Web)  │
│   (automation)   │         │  :43117 · React     │
└──────────────────┘         └─────────────────────┘
```

Four core concepts:

| Concept | What it is | Example |
|---------|-----------|---------|
| **Target** | An agent skills directory Loom knows about | `~/.claude/skills` (agent = `claude`, ownership = `observed`) |
| **Skill** | A versioned unit in the registry | `my-team-skill` with a chain of captures/releases |
| **Binding** | The rule mapping a skill to a target | agent=`claude`, profile=`work`, matcher `path-prefix:/Users/me/work` |
| **Projection** | The act of realizing a skill into a target | `loom skill project my-skill --binding <id> --method symlink` |

## Comparison

| Capability | [skills-hub](https://github.com/qufei1993/skills-hub) | [cc-switch](https://github.com/farion1231/cc-switch) | [agent-skills](https://github.com/tech-leads-club/agent-skills) | **Loom** |
|-----------|:---:|:---:|:---:|:---:|
| Projection: symlink | ✅ | ✅ | ✅ | ✅ |
| Projection: copy | ✅ | ✅ | ✅ | ✅ |
| Projection: materialize | ❌ | ❌ | ❌ | **✅** |
| Ownership tiers (managed / observed / external) | ❌ | ❌ | ❌ | **✅** |
| Binding matcher (path-prefix / exact-path / name) | ❌ | ❌ | ❌ | **✅** |
| Profiles (multi-config per agent) | ❌ | ❌ | ❌ | **✅** |
| Skill snapshot / rollback / diff | ❌ | ❌ | lockfile only | **✅** |
| Ops history + diagnose + repair | ❌ | ❌ | `audit` logs | **✅** |
| Git-native sync + replay | ❌ | cloud sync | ❌ | **✅** |
| Hard write guard | ❌ | ❌ | ❌ | **✅** |
| CLI-first + Web panel | GUI only | GUI only | CLI only | **✅** |
| Breadth of agents supported | 44 | 5 | 18 | 2 (Claude, Codex) |
| Desktop app (dmg/msi) | ✅ | ✅ | ❌ | — |

**Pick Loom when** you want fine-grained control (multi-project routing, versioned lifecycle, git-tracked audit trail) and are comfortable on the CLI. **Pick skills-hub or cc-switch** when you want a one-click GUI with broad agent coverage and don't need projection/binding semantics.

## Notes

- Multi-directory behavior is explicit via `target add`; no implicit directory inference.
- Hard write guard: if `--root` points to the Loom tool repo itself, write operations are rejected. Use an independent skill registry repo for mutable operations.
- English is the primary documentation language. [中文完整指南](docs/LOOM_COMPLETE_GUIDE_ZH.md).

## Command Surface

<details>
<summary><strong>Full CLI reference</strong> (click to expand)</summary>

```bash
loom workspace status
loom workspace doctor
loom workspace binding add --agent <claude|codex> --profile <id> --matcher-kind <path-prefix|exact-path|name> --matcher-value <value> --target <target-id> [--policy-profile <id>]
loom workspace binding list
loom workspace binding show <binding-id>
loom workspace binding remove <binding-id>
loom workspace remote set <git-url>
loom workspace remote status

loom target add --agent <claude|codex> --path <abs-path> [--ownership <managed|observed|external>]
loom target list
loom target show <target-id>
loom target remove <target-id>

loom skill add <path|git-url> --name <skill>
loom skill project <skill> --binding <binding-id> [--target <target-id>] [--method <symlink|copy|materialize>]
loom skill capture [<skill>] [--binding <binding-id>] [--instance <instance-id>] [--message <msg>]
loom skill save <skill> [--message <msg>]
loom skill snapshot <skill>
loom skill release <skill> <version>
loom skill rollback <skill> [--to <ref> | --steps <n>]
loom skill diff <skill> <from> <to>

loom sync status
loom sync push
loom sync pull
loom sync replay

loom ops list
loom ops retry
loom ops purge
loom ops history diagnose
loom ops history repair --strategy <local|remote>

loom panel [--port 43117]
```

Most commands support `--json` for machine-readable output.

</details>

### Multi-Directory Example (Claude)

```bash
loom target add --agent claude --path "$HOME/.claude/skills" --ownership observed
loom target add --agent claude --path "$HOME/.claude-work/skills" --ownership observed
loom target list
```

## Agent E2E (Recommended)

Run four real scenarios in one command (`.claude/skills`, `.claude-work/skills`, multi-directory selection, `.codex/skills` + failure feedback):

```bash
./scripts/e2e-agent-flow.sh                  # default output root
./scripts/e2e-agent-flow.sh /tmp/my-loom-e2e # custom output root
```

## Local Verification

```bash
make fmt-check
make lint
make test
make panel-build
make e2e
make ci       # all of the above
```

## Pre-Commit Hook (Recommended)

Bind `cargo fmt` to every `git commit` so CI never flags format drift:

```bash
make install-hooks
```

The hook runs `cargo fmt --all -- --check` only when `.rs` files are staged,
and fails the commit if rustfmt would make changes. Disable with
`git config --unset core.hooksPath`.

## Roadmap

- Broaden agent coverage beyond Claude & Codex (Cursor, Windsurf, Cline, Copilot, Aider, OpenCode, Gemini CLI, Goose)
- `loom workspace init` onboarding migration: auto-import existing `.claude/.codex/skills`
- TypeScript type generation for the Panel API (eliminate hand-written `panel/src/types.ts`)
- Desktop packaging (Tauri) for users who prefer a GUI
- Skill marketplace integration (upstream catalogs such as `agent-skills`)

## Community

- Issues: https://github.com/majiayu000/loom/issues
- Discussions: https://github.com/majiayu000/loom/discussions
