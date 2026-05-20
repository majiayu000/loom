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

**Loom treats skills like infrastructure**: a Git-backed registry (add → capture → save → snapshot → release → rollback → diff), projected onto one or many agent directories through explicit bindings (agent + profile + matcher + policy), with sync, replay, and audit trail. CLI-first for automation, Panel-assisted for visibility.

## Quick Start

```bash
# 1. Install a prebuilt binary with bundled Panel assets
cargo binstall skillloom

# or install from the Homebrew tap after its formula PR is merged
brew install majiayu000/tap/loom

# or install from source when you have Rust and Bun available
git clone https://github.com/majiayu000/loom.git
cd loom && cargo install --path .

# 2. Initialize the default registry and auto-register existing agent skill dirs
loom init

# 3. Import/update observed skills once, or keep watching in the foreground
loom monitor --once
loom monitor --interval-seconds 30
```

Prebuilt GitHub Release archives are the recommended install path. They are built
with Bun in CI, smoke-tested with `loom panel --port 0`, and include the Panel
frontend inside the `loom` binary. Source installs are useful for development,
but they need Bun when `panel/dist` is missing or stale.

To verify a downloaded release archive:

```bash
VERSION=X.Y.Z
TARGET=aarch64-apple-darwin
ARCHIVE="skillloom-${VERSION}-${TARGET}.tar.gz"

curl -LO "https://github.com/majiayu000/loom/releases/download/v${VERSION}/${ARCHIVE}"
curl -LO "https://github.com/majiayu000/loom/releases/download/v${VERSION}/SHA256SUMS"
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify "${ARCHIVE}" --repo majiayu000/loom
tar -xzf "${ARCHIVE}"
install -m 755 "skillloom-${VERSION}-${TARGET}/loom" "$HOME/.local/bin/loom"
```

Loom defaults to `~/.loom-registry`. Pass `--root <dir>` only when you want a different registry.

For agent automation, keep the mutable registry separate from the Loom source checkout and always use an explicit root:

```bash
REGISTRY_ROOT="$HOME/.loom-registry"

loom --json --root "$REGISTRY_ROOT" workspace init --scan-existing
loom --json --root "$REGISTRY_ROOT" skill monitor-observed --once
loom --json --root "$REGISTRY_ROOT" workspace status
loom --json --root "$REGISTRY_ROOT" sync status
```

`--root` is the Git-backed skill registry directory, not the Loom tool repository.

For managed projection flows:

```bash
# Import a skill into the registry
loom skill add "$HOME/.claude/skills/my-skill" --name my-skill

# Register a managed Claude Code target
mkdir -p "$HOME/.loom-targets/claude/skills"
TARGET_ID="$(
  loom --json target add \
    --agent claude --path "$HOME/.loom-targets/claude/skills" --ownership managed \
    | jq -r '.data.target.target_id'
)"

# Bind this project/workspace to that target
loom workspace binding add \
  --agent claude --profile home \
  --matcher-kind path-prefix --matcher-value "$PWD" \
  --target "$TARGET_ID"

# Project the skill, then open the control panel
BINDING_ID="$(loom --json workspace binding list | jq -r '.data.bindings[0].binding_id')"
loom skill project my-skill --binding "$BINDING_ID" --method symlink
loom panel        # -> http://localhost:43117
```

`loom panel` now serves a frontend bundled into the Rust binary at build time, so it works even when `--root` points at a separate registry directory. If panel assets are unavailable in your build, reinstall from a checkout with `bun` available so Loom can package the frontend during compile.

Prefer a guided walkthrough? Run `./scripts/demo.sh` for a scripted end-to-end tour (init → target add → status → panel hint) against a throwaway registry. `./scripts/e2e-agent-flow.sh` runs the four real integration scenarios used in CI.

## Panel

<!-- TODO: replace with a real screenshot at ./assets/panel-screenshot.png once captured locally. -->
<!-- Capture steps:                                                                                   -->
<!--   1. ./scripts/demo.sh /tmp/loom-panel-demo                                                      -->
<!--   2. target/debug/loom --root /tmp/loom-panel-demo panel                                         -->
<!--   3. Screenshot http://localhost:43117 (overview + skills views), save as PNG into assets/.      -->

> Visual control panel for the registry. Launches on `http://localhost:43117`
> via `loom panel`; diff projections, inspect bindings, and replay pending ops
> in a single-page React app served by the same Rust binary.

## Features

- **🎯 Projection with three modes** — `symlink` / `copy` / `materialize`, per binding
- **🎚️ Ownership tiers** — `managed` (Loom writes) / `observed` (read-only) / `external` (hands-off)
- **🔗 Binding matchers** — route a skill to a target by `path-prefix`, `exact-path`, or `name`
- **📦 Profiles** — multiple config sets per agent (e.g. work/home Claude profiles)
- **🧬 Git-backed lifecycle** — `add → capture → save → snapshot → release → rollback → diff` ([when to use which](#skill-lifecycle-verbs))
- **🔁 Git-backed sync** — `sync push / pull / replay` between a team's registries
- **🛠️ Ops with audit** — `ops list / retry / purge` and `ops history diagnose / repair`
- **🛡️ Hard write guard** — refuses to write when `--root` points at the Loom tool repo itself
- **🖥️ CLI + Panel** — script anything from the CLI; diff and inspect from the React Panel
- **📤 JSON envelope** — machine-facing commands speak compact `--json` for machine consumption (`--pretty` is available for human debugging)

## How It Works

```
┌───────────────────┐         ┌────────────────────┐
│   Skill Registry  │         │    Target Dirs     │
│  (your Git repo)  │         │                    │
│                   │         │  ~/.claude/skills  │
│   skills/*        │         │  ~/.codex/skills   │
│   state/registry  │ ──────▶ │  /repo/.claude/... │
│   Git history     │         │  …                 │
└─────────▲─────────┘         └──────────▲─────────┘
          │                              │
          │   capture / save / snapshot  │ projection
          │   (Git-backed lifecycle)     │ (symlink / copy / materialize)
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
| **Skill** | A tracked unit in the registry | `my-team-skill` with a chain of captures/releases |
| **Binding** | The rule mapping a skill to a target | agent=`claude`, profile=`work`, matcher `path-prefix:/Users/me/work` |
| **Projection** | The act of realizing a skill into a target | `loom skill project my-skill --binding <id> --method symlink` |

### Skill lifecycle verbs

The chain `add → capture → save → snapshot → release → rollback` is the most common point of confusion because several verbs all touch source history. Each one answers a different question:

| Verb | What it does | When to reach for it | Acts on |
|------|--------------|----------------------|---------|
| `loom skill add` | Import a skill source into the registry | First-time onboarding of a skill from a local path or Git URL | Source (initial import) |
| `loom skill project` | Realize a registry skill into an agent directory | Make the skill visible to the agent (Claude/Codex/…) | Target (live directory) |
| `loom skill capture` | Pull live edits from a projection back into the source | The user edited the skill **inside the agent directory** and you want those edits tracked | Projection → source |
| `loom skill save` | Commit edits made directly to the registry source | You edited `skills/<name>/…` **inside the registry repo** itself | Source (in place) |
| `loom skill snapshot` | Mark an unnamed checkpoint on source history | You want a labelable anchor before risky work, but no semver yet | Source (anchor) |
| `loom skill release` | Tag the skill at a semantic version | You're publishing a stable revision teammates can pull (`v1.2.0`) | Source (semver tag) |
| `loom skill rollback` | Reset the source to an earlier revision (with `recovery_ref`) | A capture or save introduced bad state — undo it without losing the recovery point | Source (history) |
| `loom skill diff` | Compare two revisions of a skill source | Inspect what changed between any two refs (commit, snapshot, release tag) | Source (read-only) |

Quick decision: **edits from the agent side → `capture`; edits inside the registry repo → `save`; anchor → `snapshot`; public version → `release`; undo → `rollback`.**

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
| Breadth of agents supported | 44 | 5 | 18 | 10 (Claude, Codex, Cursor, Windsurf, Cline, Copilot, Aider, OpenCode, Gemini CLI, Goose) |
| Desktop app (dmg/msi) | ✅ | ✅ | ❌ | — |

**Pick Loom when** you want fine-grained control (multi-project routing, Git-backed lifecycle, git-tracked audit trail) and are comfortable on the CLI. **Pick skills-hub or cc-switch** when you want a one-click GUI with broad agent coverage and don't need projection/binding semantics.

## Notes

- Multi-directory behavior is explicit via `target add`; no implicit directory inference.
- Agent automation should use explicit `--root`, `--json`, selectors such as `binding_id` / `target_id`, and branch on `ok` + `error.code`.
- `--json` wraps both command execution errors and argument parsing failures in the same envelope. `loom panel` is the local HTTP UI server and does not return a command envelope.
- Read commands such as `workspace status`, `workspace doctor`, `target list`, and `sync status` do not mutate registry state, Git refs, the Git index, live target directories, or the pending queue. They do write durable command audit events under `state/events/commands.jsonl`.
- Registry metadata lives under `state/registry`; Loom does not use release-style labels for internal state names.
- State-changing registry commands commit `state/registry` to Git, and `sync push` has a safety commit before pushing.
- Hard write guard: if `--root` points to the Loom tool repo itself, write operations are rejected. Use an independent skill registry repo for mutable operations.
- English is the primary documentation language. [中文完整指南](docs/LOOM_COMPLETE_GUIDE_ZH.md).
- V1 planning lives in [Loom V1 Core Spec](docs/LOOM_V1_CORE_SPEC.md).

## Command Surface

<details>
<summary><strong>Full CLI reference</strong> (click to expand)</summary>

Supported `--agent` values are `claude`, `codex`, `cursor`, `windsurf`, `cline`, `copilot`, `aider`, `opencode`, `gemini-cli`, and `goose`.

```bash
loom init
loom monitor [--target <target-id>] [--once] [--interval-seconds <seconds>]

loom workspace status
loom workspace doctor
loom workspace init [--scan-existing]
loom workspace binding add --agent <agent> --profile <id> --matcher-kind <path-prefix|exact-path|name> --matcher-value <value> --target <target-id> [--policy-profile <id>]
loom workspace binding list
loom workspace binding show <binding-id>
loom workspace binding remove <binding-id>
loom workspace remote set <git-url>
loom workspace remote status

loom target add --agent <agent> --path <abs-path> [--ownership <managed|observed|external>]
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
loom skill import-observed [--target <target-id>]
loom skill monitor-observed [--target <target-id>] [--once] [--interval-seconds <seconds>]

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

Most commands support compact `--json` for machine-readable output; add `--pretty` when you want formatted JSON for inspection. Commands default to `~/.loom-registry`; use `--root <dir>` to override that registry.

`target add` defaults to `--ownership observed`; pass `--ownership managed` only for directories Loom is allowed to write.

</details>

### Multi-Directory Example (Claude)

```bash
loom target add --agent claude --path "$HOME/.claude/skills" --ownership observed
loom target add --agent claude --path "$HOME/.claude-work/skills" --ownership observed
loom target list
```

### Observed Skill Monitoring

Use this when the real source of truth is still an agent skill directory such as `~/.claude/skills` or `~/.codex/skills`.

```bash
loom monitor --once
loom monitor --interval-seconds 30
```

`loom monitor` is a short alias for `loom skill monitor-observed`. It imports new observed skills and updates existing registry copies when file content changes. It does not delete registry skills when an observed directory disappears; deletion stays an explicit cleanup action.

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

- Per-agent default path conventions & env overrides (beyond `CLAUDE_SKILLS_DIR` / `CODEX_SKILLS_DIR`) for the 8 newly added agents — paths are currently supplied explicitly via `target add --path`
- Extend `loom workspace init --scan-existing` auto-import to the 8 newly added agents once their skill-directory conventions stabilize (currently scans Claude and Codex only)
- Desktop packaging (Tauri) for users who prefer a GUI
- Skill marketplace integration (upstream catalogs such as `agent-skills`)

## Community

- Issues: https://github.com/majiayu000/loom/issues
- Discussions: https://github.com/majiayu000/loom/discussions
