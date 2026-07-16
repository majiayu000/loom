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
    <a href="CHANGELOG.md">Changelog</a> ·
    <a href="#features">Features</a> ·
    <a href="#how-it-works">How It Works</a> ·
    <a href="#comparison">Comparison</a> ·
    <a href="#command-surface">CLI</a>
  </p>
</div>

---

## Why Loom?

AI coding agents (Claude Code, Codex, Cursor, Windsurf, …) all read skills from **different directories**. Keeping them in sync is either:

- **Manual**: `cp -R` or `ln -s` between `~/.claude/skills`, `~/.agents/skills`, legacy `~/.codex/skills`, repo-local `.agents/skills`, … — easy to drift, hard to roll back, impossible to audit.
- **One-way sync apps**: install skills from a central store, but no binding logic, no per-project matching, no version history, no replay when things go wrong.

**Loom treats skills like infrastructure**: a Git-backed registry (add → commit → release --anchor / release <version> → rollback → diff), projected onto one or many agent directories through explicit bindings (agent + profile + matcher + policy), with sync, replay, and audit trail. CLI-first for automation, Panel-assisted for visibility.

Loom can import from local directories, Git URLs, and GitHub locators, but it is not a marketplace or a wrapper around `gh skill install`. Provider boundaries are documented in [Skill Provider Boundary](docs/SKILL_PROVIDER_BOUNDARY.md): upstream tools find, preview, or publish skills; Loom owns local lockfile, policy, projection, audit, rollback, and eval.

## Quick Start

```bash
# 1. Install a prebuilt release archive (recommended)
# Pick one target: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu
VERSION="0.1.5" # replace with the latest release version
TARGET="aarch64-apple-darwin"
BASE_URL="https://github.com/majiayu000/loom/releases/download/v${VERSION}"
curl -LO "${BASE_URL}/skillloom-${VERSION}-${TARGET}.tar.gz"
curl -LO "${BASE_URL}/SHA256SUMS"
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf "skillloom-${VERSION}-${TARGET}.tar.gz"
sudo install "skillloom-${VERSION}-${TARGET}/loom" /usr/local/bin/loom

# or install from the Homebrew tap after its formula PR is merged
brew install majiayu000/tap/loom

# or build from source
git clone https://github.com/majiayu000/loom.git
cd loom && cargo install --path .

# Optional: install the first-party Agent Skill for the agent(s) you use.
# For an extracted release archive:
SKILL_SOURCE="$PWD/skillloom-${VERSION}-${TARGET}/skills/loom-registry"
# For Homebrew, use this source instead:
# SKILL_SOURCE="$(brew --prefix loom)/share/loom/skills/loom-registry"
# For a source checkout after `cd loom`, use this source instead:
# SKILL_SOURCE="$PWD/skills/loom-registry"

install_loom_registry_skill() {
  source_dir="$1"
  target_dir="$2"
  if [ -e "$target_dir" ] || [ -L "$target_dir" ]; then
    printf '%s\n' "Refusing to overwrite existing Skill: $target_dir" >&2
    return 1
  fi
  test -f "$source_dir/SKILL.md"
  mkdir -p "$(dirname "$target_dir")"
  cp -R "$source_dir" "$target_dir"
}

# Run one or both lines, depending on the agents you use.
install_loom_registry_skill "$SKILL_SOURCE" "$HOME/.claude/skills/loom-registry"
install_loom_registry_skill "$SKILL_SOURCE" "$HOME/.agents/skills/loom-registry"

# 2. Initialize the default registry and auto-register existing agent skill dirs
loom init

# 3. Manage one skill through the single-skill lifecycle
loom skill author new fixflow --template coding-workflow
loom skill lint fixflow --portable
loom skill lint fixflow --quality
loom skill scan fixflow
loom skill activate fixflow --agent codex --scope user --dry-run
loom skill activate fixflow --agent codex --scope user
loom skill visibility fixflow --agent codex
```

The Agent Skill is named `loom-registry` to avoid colliding with Loom.com video Skills. The copy commands fail closed when a same-name target already exists; inspect and resolve that target manually instead of overwriting it. Start a new Claude Code or Codex session after copying so the agent can discover the Skill. A source build can use `skills/loom-registry` from the checkout; `cargo install` installs only the CLI binary and does not install the Agent Skill.

Loom defaults to `~/.loom-registry`. Pass `--root <dir>` only when you want a different registry.

Before importing a third-party or local catalog skill, use `loom catalog preview <locator>` and `loom skill install <locator> --name <skill> --dry-run` to inspect scripts, lint, safety, provenance, lockfile, and trust defaults without writing registry state.

For existing agent directories, import observed skills separately:

```bash
loom monitor --once
loom monitor --interval-seconds 30
```

Read the workflow guides:

- [Single-Skill Lifecycle](docs/SINGLE_SKILL_LIFECYCLE.md)
- [Codex Skill Visibility](docs/CODEX_SKILL_VISIBILITY.md)
- [Migrating To An Active View](docs/MIGRATING_TO_ACTIVE_VIEW.md)

For agent automation, keep the mutable registry separate from the Loom source checkout and always use an explicit root:

```bash
REGISTRY_ROOT="$HOME/.loom-registry"

loom --json --root "$REGISTRY_ROOT" workspace init --scan-existing
loom --json --root "$REGISTRY_ROOT" skill monitor-observed --once
loom --json --root "$REGISTRY_ROOT" workspace status
loom --json --root "$REGISTRY_ROOT" sync status
```

`--root` is the Git-backed skill registry directory, not the Loom tool repository.

For advanced managed projection flows:

```bash
# Import a skill into the registry
loom skill add "$HOME/.claude/skills/my-skill" --name my-skill
# Or pin a Git/GitHub source ref and subdirectory.
loom skill add github:owner/repo//skills/my-skill --name my-skill --ref v1.2.3

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

Release archives are the preferred install path because their binaries are built with the Panel frontend already bundled and smoke-tested. To verify a downloaded archive, use the release `SHA256SUMS` file as shown above; if you use GitHub CLI, you can also verify provenance with `gh attestation verify skillloom-${VERSION}-${TARGET}.tar.gz --repo majiayu000/loom`.

Release notes live in [CHANGELOG.md](CHANGELOG.md) and the [GitHub Releases](https://github.com/majiayu000/loom/releases) page.

Prefer a guided walkthrough? Run `./scripts/demo.sh` for a scripted end-to-end tour (init → target add → status → panel hint) against a throwaway registry. `./scripts/e2e-agent-flow.sh` runs the four real integration scenarios used in CI.

## Panel

<!-- TODO: replace with a real screenshot at ./assets/panel-screenshot.png once captured locally. -->
<!-- Capture steps:                                                                                   -->
<!--   1. ./scripts/demo.sh /tmp/loom-panel-demo                                                      -->
<!--   2. target/debug/loom --root /tmp/loom-panel-demo panel                                         -->
<!--   3. Screenshot http://localhost:43117 (overview + skills views), save as PNG into assets/.      -->

> Visual control panel for the registry. Launches on `http://localhost:43117`
> via `loom panel`; diff projections, inspect bindings, and replay queued operations
> in a single-page React app served by the same Rust binary.

## Features

- **🎯 Projection with three modes** — `symlink` / `copy` / `materialize`, per binding
- **🎚️ Ownership tiers** — `managed` (Loom writes) / `observed` (read-only) / `external` (hands-off)
- **🔗 Binding matchers** — route a skill to a target by `path-prefix`, `exact-path`, or `name`
- **📦 Profiles** — multiple config sets per agent (e.g. work/home Claude profiles)
- **🧬 Git-backed lifecycle** — `add → commit → release --anchor → release <version> → rollback → diff` ([when to use which](#skill-lifecycle-verbs))
- **🩺 Skill status** — `skill inspect` shows one read-only lifecycle card; `skill diagnose` drills into source, bindings, targets, projections, drift, and related operations
- **🛡️ Skill safety state** — `skill scan`, trust levels, quarantine metadata, and security diff gate risky skills before projection
- **🔎 Provider previews** — `provider` and `catalog` commands preview third-party/local skills safely before any install path can write registry state
- **🧰 Runtime readiness** — `skill deps` reports required tools, env vars, MCP config, and network expectations without printing secrets
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
│   skills/*        │         │  ~/.agents/skills  │
│   state/registry  │ ──────▶ │  legacy .codex     │
│   Git history     │         │  /repo/.agents/... │
│                   │         │  …                 │
└─────────▲─────────┘         └──────────▲─────────┘
          │                              │
          │ commit / release --anchor    │ projection
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
| **Skill** | A tracked unit in the registry | `my-team-skill` with a chain of commits/releases |
| **Binding** | The rule mapping a skill to a target | agent=`claude`, profile=`work`, matcher `path-prefix:/Users/me/work` |
| **Projection** | The act of realizing a skill into a target | `loom skill project my-skill --binding <id> --method symlink` |

### Skill lifecycle verbs

The current lifecycle keeps source history on a smaller verb set: import with `add`, record edits with `commit`, mark either unnamed anchors or semver releases with `release`, recover with `rollback`, and inspect changes with `diff`.

| Verb | What it does | When to reach for it | Acts on |
|------|--------------|----------------------|---------|
| `loom skill add` | Import a skill source into the registry and record source provenance in `loom.lock` | First-time onboarding of a skill from a local path, Git URL, local Git repo, or `github:owner/repo//subdir` | Source (initial import) |
| `loom skill list` | List registry, source, and observed skill inventory | See what skills exist before mutating registry state | Source + registry metadata (read-only) |
| `loom skill inspect --brief` | Show one skill from the shared inventory model | Inspect entrypoint, description, source status, projections, compatible targets, warnings, and next actions | Source + registry metadata (read-only) |
| `loom skill inspect` | Show one skill lifecycle status card | Check source, lint, projection/runtime, quality, safety, and next actions without mutation | Source + registry metadata (read-only) |
| `loom skill deps` | Check runtime dependency readiness | Report required tools, env vars, MCP servers, and network expectations before activation or use | Source + local environment (read-only) |
| `loom skill compile` | Plan and verify derived compiled runtime artifacts | Return read-only dry-run plans, list known artifacts, and verify manifests, sidecars, digests, and gates without replacing `SKILL.md` | Source + compiled artifact state (read-only) |
| `loom skill activate` | Activate one skill for an agent without manual binding IDs | Create or repair the managed target, binding, rule, and projection selected by agent/scope/profile | Source + target + registry metadata |
| `loom skill deactivate` | Deactivate one skill from an agent target | Remove the desired rule and only delete safe symlink projections; copy/materialize fail closed | Target + registry metadata |
| `loom skill active list` | List desired active skills and realized projections | See active rules, projection health, missing targets, and explicit `not_checked` visibility claims | Registry metadata + target filesystem (read-only) |
| `loom skill visibility` | Explain one skill's agent active-view visibility | For Codex, join source, active rules, projection symlink, config disables, runtime entries, external entries, and restart requirements | Source + registry metadata + target filesystem (read-only) |
| `loom skill search` | Search, resolve, and explain skill candidates with deterministic scoring | Find likely skills by metadata; use `--for-task` for task resolution and `--explain` for recommendation details | Source + registry metadata (read-only) |
| `loom skill author draft/extract/rewrite/tune-description/generate-evals` | Create guarded authoring patch artifacts with the deterministic mock provider | Review proposed source/eval diffs without mutating `skills/<skill>`; prompt material is redacted and size-bounded | Source + `state/patches` artifact output |
| `loom skill author apply-patch` | Apply a reviewed authoring patch through validation gates | Requires an idempotency key, revalidates source digest/ref, runs staging lint/safety/eval gates, commits only after validation, and supports idempotent replay | Patch artifact state + skill source |
| `loom skill commit` | Commit source changes from the registry or a live projection | Preserve edits after Loom detects source-only, projection-only, or ambiguous drift; use `--from-source` / `--from-projection` only when needed | Source history |
| `loom skill release --anchor` | Mark the current source revision without a semver tag | Create a named recovery point before risky work or review, without publishing a version | Source history |
| `loom skill release <version>` | Tag a semver release | Publish a stable revision teammates can pull and compare | Source history |
| `loom skill rollback` | Reset the source to an earlier revision with a recovery ref | Undo a bad commit or release without losing the pre-rollback state | Source history |
| `loom skill diff` | Compare two source revisions | Review raw source changes or security-relevant deltas before promotion | Source history (read-only) |
| `loom instruction scan/show/classify/doctor/migrate-plan` | Inspect native instruction surfaces without importing them as skills | Inventory `AGENTS.md`, `CLAUDE.md`, Cursor, Windsurf, and Copilot instruction files; diagnose overlap; emit dry-run migration plans only | Workspace files (read-only) |
| `loom skill author new` | Create a lint-clean local skill skeleton | Start a new registry-owned skill with `SKILL.md`, references, scripts, assets, eval stubs, and `loom.skill.toml` | Source (initial create) |
| `loom provider add/list/remove` | Manage local or GitHub catalog provider records | Configure provider ids for advisory search/preview without storing credentials | Registry provider state |
| `loom catalog search/show/preview` | Inspect provider locators without executing source code | See metadata, scripts, license/provenance hints, lint, safety, and install dry-run guidance | Provider source (read-only) |
| `loom package plan/build/verify` | Build deterministic outbound package artifacts from reviewed skills | Create a reviewed package plan, build a portable archive, and verify manifest/checksum/content integrity without claiming active install state | Skill source + package artifact |
| `loom mcp requirement/plan/apply/doctor/catalog` | Plan and apply guarded MCP server configuration | List declared requirements, inspect source policy, write reviewed Codex config through idempotent apply, and point to next actions without storing secrets | Skill source + local agent config |
| `loom provision plan/doctor/apply/export/import` | Plan devcontainer and remote active-view provisioning without target mutation | Preview generated devcontainer setup, active-view paths, dependency readiness, secret names, policy gates, reviewed shell/tar export artifacts, and import dry-runs before apply exists | Registry + target workspace (read-only for plan/doctor/import dry-run; explicit artifact output for shell/tar export) |
| `loom skill install --dry-run` | Plan a provider-backed import without writing registry state | Check pin policy, lint, safety, provenance, lockfile, and trust defaults before mutating install apply exists | Provider source + policy (read-only except command audit) |
| `loom policy org init/show/check` | Initialize and inspect Git-backed org policy | Review allow/deny/approval-required decisions before enforcement is wired across all mutations | Registry policy state |
| `loom approval request/list/approve/reject` | Manage append-only approval events | Create and decide auditable approval requests with redacted reasons/comments and role checks | Registry approval log |
| `loom roles list/grant/revoke` | Manage local org role grants | Bootstrap and review viewer/author/reviewer/maintainer/admin grants without hosted RBAC | Registry role state |
| `loom skill provenance inspect/verify/outdated/refresh` | Inspect, check, report stale provider pins, or refresh recorded source provenance and `loom.lock` | Confirm a skill still matches pinned metadata, list stale provider-backed installs, and emit review-only re-pin plans | Source metadata + `loom.lock` |
| `loom skill policy` | Report declared capabilities, content risks, provenance drift, policy decision, and embedded safety scan | Review a skill before projection or explain why a policy profile blocks it | Source metadata + source files (read-only) |
| `loom skill scan` | Return unified safety findings, trust state, and activation decision | Review prompt-injection, script, secret, network, provenance, and trust risks before activation | Source + trust metadata (read-only) |
| `loom skill trust/quarantine/unquarantine` | Persist registry-owned trust and quarantine metadata | Mark review state or block a skill without editing portable `SKILL.md` | Registry trust state |
| `loom skill eval` | Run offline fixtures or explicit eval harnesses | Compare offline quality, trigger behavior, and mock with-skill/no-skill baselines without network calls by default | Source + eval fixtures; reports under registry state |
| `loom skill improve` | Run a read-only single-skill preflight | Aggregate source drift, lint, safety, dependency, eval, and optional real-eval planning before saving edits | Source + local environment (read-only) |
| `loom skill regression` | Compare one skill against a baseline gate | Fail with typed regression details when lint, safety, dependency, eval, or size gates block the candidate | Source + local environment (read-only) |
| `loom skillset create/add/remove/show/lint/activate/deactivate/eval/release/rollback` | Group existing registry skills into a named set | Organize coherent skill bundles, activate members together, aggregate member evals, and version skillset definitions | Registry skillset state + target projections |
| `loom telemetry status/enable/disable/report/export/purge` | Manage local privacy-preserving telemetry | Opt in to redacted local event writes, aggregate usage/value/cost/drift/risk, export redacted events, and purge selected telemetry state with dry-run confirmation | `state/telemetry` |
| `loom skill used/feedback` | Record redacted local production usage and recommendation feedback | Hook agent wrappers and recommendation flows into local telemetry without storing raw prompts, output, errors, env values, or file contents | `state/telemetry` |
| `loom workflow create/show/plan/preflight` | Define and guard a multi-skill DAG workflow | Agents need an auditable plan before coordinating several skills; execution remains hidden/deferred until apply gates land | Registry workflow state + source metadata |
| `loom use` | Plan or apply target, binding, and projection setup in one flow | New users want to use a skill without copying target/binding IDs between commands | Source + target + registry metadata |
| `loom plan use` / `loom apply` | Persist a guarded use plan, then execute it with idempotency | Agents need a retry-safe plan/apply protocol for higher-risk flows | Command audit + source/target/registry metadata |
| `loom skill project` | Realize a registry skill into an agent directory | Make the skill visible to the agent (Claude/Codex/…) | Target (live directory) |
| `loom skill commit` | Commit edits from the registry source or a live projection | Loom detects source-only vs projection-only edits; use `--from-source` or `--from-projection` for conflicts | Source or projection |
| `loom skill release --anchor` | Mark an unnamed checkpoint on source history | You want a labelable anchor before risky work, but no semver yet | Source (anchor) |
| `loom skill release <version>` | Tag the skill at a semantic version | You're publishing a stable revision teammates can pull (`v1.2.0`) | Source (semver tag) |
| `loom skill rollback` | Reset the source to an earlier revision (with `recovery_ref`) | A commit introduced bad state; undo it without losing the recovery point | Source (history) |
| `loom skill diff` | Compare two revisions of a skill source | Inspect raw source changes or use `--security` for security-relevant findings only | Source (read-only) |
| `loom skill lint` | Check portable Agent Skills metadata compliance | Validate `SKILL.md`, YAML frontmatter, portable name, and description before projection | Source (read-only) |
| `loom skill diagnose --check drift` | Detect uncommitted drift in a skill source | Confirm `skills/<name>` matches the committed source tree; flag external edits that bypassed `commit` | Source (read-only) |
| `loom skill diagnose` | Run a read-only health report for one skill | Explain missing source, broken bindings/targets/projections, source drift, operation backlog issues, and recent failures | Source + registry metadata (read-only) |
| `loom codex reconcile` | Plan or repair Codex active-view visibility | Dry-run projection/config actions, repair safe Loom-owned symlinks, remove stale records, and optionally patch safe config disables | Target + registry metadata + Codex config |

Quick decision: **edits from either side → `commit` (add `--from-projection` or `--from-source` only for conflicts); anchor → `release --anchor`; public version → `release --preflight --baseline <ref>`; undo → `rollback`; drift audit → `diagnose --check drift`; health triage → `diagnose`; quality evidence → `eval`.**

## Comparison

| Capability | [skills-hub](https://github.com/qufei1993/skills-hub) | [cc-switch](https://github.com/farion1231/cc-switch) | [agent-skills](https://github.com/tech-leads-club/agent-skills) | **Loom** |
|-----------|:---:|:---:|:---:|:---:|
| Projection: symlink | ✅ | ✅ | ✅ | ✅ |
| Projection: copy | ✅ | ✅ | ✅ | ✅ |
| Projection: materialize | ❌ | ❌ | ❌ | **✅** |
| Ownership tiers (managed / observed / external) | ❌ | ❌ | ❌ | **✅** |
| Binding matcher (path-prefix / exact-path / name) | ❌ | ❌ | ❌ | **✅** |
| Profiles (multi-config per agent) | ❌ | ❌ | ❌ | **✅** |
| Skill release anchors / rollback / diff | ❌ | ❌ | lockfile only | **✅** |
| Ops history + diagnose + repair | ❌ | ❌ | `audit` logs | **✅** |
| Git-native sync + replay | ❌ | cloud sync | ❌ | **✅** |
| Hard write guard | ❌ | ❌ | ❌ | **✅** |
| CLI-first + Web panel | GUI only | GUI only | CLI only | **✅** |
| Breadth of agents supported | 44 | 5 | 18 | 10 ([list](docs/SUPPORTED_AGENTS.md)) |
| Desktop app (dmg/msi) | ✅ | ✅ | ❌ | — |

**Pick Loom when** you want fine-grained control (multi-project routing, Git-backed lifecycle, git-tracked audit trail) and are comfortable on the CLI. **Pick skills-hub or cc-switch** when you want a one-click GUI with broad agent coverage and don't need projection/binding semantics.

## Notes

- Multi-directory behavior is explicit via `target add`; no implicit directory inference.
- Agent automation should use explicit `--root`, `--json`, selectors such as `binding_id` / `target_id`, and branch on `ok` + `error.code`.
- Agents can call `loom skill search "<task>" --for-task --agent <agent> --workspace <path>` before choosing a workflow skill, then `loom agent preflight --agent <agent> --workspace <path> --skill <skill>` before writing. Add `--dry-run` to high-risk writes, or use `loom skill rollback --dry-run` to get a no-mutation rollback plan.
- `--json` wraps both command execution errors and argument parsing failures in the same envelope. `loom panel` is the local HTTP UI server and does not return a command envelope.
- Read commands such as `workspace status`, `workspace doctor`, `target list`, `skill list`, `skill inspect`, `skill inspect --brief`, `skill deps`, `skill search`, `skillset show`, `skillset lint`, and `sync status` do not mutate registry state, Git refs, the Git index, live target directories, or the operation backlog. Durable command audit events may be recorded under `state/events/commands.jsonl` for audited surfaces.
- Registry metadata lives under `state/registry`; Loom does not use release-style labels for internal state names.
- State-changing registry commands commit `state/registry` to Git, and `sync push` has a safety commit before pushing.
- Hard write guard: if `--root` points to the Loom tool repo itself, write operations are rejected. Use an independent skill registry repo for mutable operations.
- English is the primary documentation language. [中文完整指南](docs/LOOM_COMPLETE_GUIDE_ZH.md).
- Release notes and install trust guidance live in [CHANGELOG.md](CHANGELOG.md), [Releasing Loom](docs/RELEASING.md), and [Security Policy](SECURITY.md).
- V1 planning lives in [Loom V1 Core Spec](docs/LOOM_V1_CORE_SPEC.md).

## Command Surface

<details>
<summary><strong>Full CLI reference</strong> (click to expand)</summary>

Supported `--agent` values are `claude`, `codex`, `cursor`, `windsurf`, `cline`, `copilot`, `aider`, `opencode`, `gemini-cli`, and `goose`.

```bash
loom init
loom backup export [--output <path>] [--format tar] [--include-target-cache]
loom backup inspect <artifact>
loom backup restore <artifact> [--force-empty-root]
loom monitor [--target <target-id>] [--once] [--interval-seconds <seconds>]
loom use <skill> --agents <agent[,agent]> [--scope project] [--workspace <path>] [--profile <id>] [--method <symlink|copy|materialize>] [--target-root <path>] [--apply]
loom plan use <skill> --agents <agent[,agent]> [--scope project] [--workspace <path>] [--profile <id>] [--method <symlink|copy|materialize>] [--target-root <path>]
loom apply <plan-id> --idempotency-key <key> [--approve <token[,token]>]
loom workspace status
loom workspace doctor
loom workspace init [--scan-existing]
loom workspace binding add --agent <agent> --profile <id> --matcher-kind <path-prefix|exact-path|name> --matcher-value <value> --target <target-id> [--policy-profile <id>]
loom workspace binding list
loom workspace binding show <binding-id>
loom workspace binding remove <binding-id> [--orphan-projections]
loom workspace remote set <git-url>
loom workspace remote status

loom agent preflight --agent <agent> --workspace <path> [--skill <skill>] [--method <symlink|copy|materialize>]
loom codex reconcile [--dry-run | --apply] [--fix-config] [--binding <binding-id>] [--target <target-id>] [--allowlist <path>]

loom target add --agent <agent> --path <abs-path> [--ownership <managed|observed|external>]
loom target list
loom target show <target-id>
loom target remove <target-id>

loom skill list
loom skill inspect <skill> [--agent <agent>] [--workspace <path>] [--profile <profile>]
loom skill inspect <skill> --brief
loom skill inspect <skill> --include-telemetry
loom skill used <skill> [--agent <agent>] [--workspace <path>] [--session-id <id>] [--tokens-in <n>] [--tokens-out <n>] [--commands <n>] [--duration-ms <n>] [--success | --error] [--failure-category <category>]
loom skill feedback <skill> --feedback <accepted|rejected|ignored> [--agent <agent>] [--workspace <path>] [--session-id <id>] [--task <text>]
loom skill deps <skill> [--agent <agent>] [--workspace <path>]
loom skill compile <skill> --dry-run [--agent <agent>] [--profile <profile>]
loom skill compile --skill <skill> --dry-run [--agent <agent>] [--profile <profile>]
loom skill compile list <skill>
loom skill compile verify <skill> [--artifact <artifact-id>]
loom skill activate <skill> --agent <agent> [--scope <user|project>] [--workspace <path>] [--profile <profile>] [--target <target-id>] [--method <symlink|copy|materialize>] [--dry-run]
loom skill deactivate <skill> --agent <agent> [--scope <user|project>] [--workspace <path>] [--profile <profile>] [--target <target-id>] [--dry-run]
loom skill active list --agent <agent> [--scope <user|project>] [--workspace <path>] [--profile <profile>]
loom skill visibility <skill> --agent codex [--workspace <path>] [--profile <profile>]
loom skill search <query> [--agent <agent>] [--profile <profile>] [--status <status>] [--trust <trust>] [--workspace <path>] [--active] [--for-task] [--semantic] [--explain]
loom skill author draft <skill> --from-session <path|id> [--agent <agent>] [--provider mock] [--dry-run]
loom skill author extract <skill> --from-diff <path> [--provider mock] [--dry-run]
loom skill author rewrite <skill> --instruction <text> [--provider mock] [--dry-run]
loom skill author tune-description <skill> [--description <text>] [--provider mock] [--dry-run]
loom skill author generate-evals <skill> [--task <text>] [--provider mock] [--dry-run]
loom skill author apply-patch <patch-id> --idempotency-key <key>
loom skill author new <skill> [--template <basic|coding-workflow|scripted|reference-heavy>] [--description <text>] [--agent <agent>] [--dry-run]
loom skill add <path|git-url|github:owner/repo//subdir> --name <skill> [--ref <branch|tag|commit>] [--subdir <path>]
loom skill provenance inspect <skill>
loom skill provenance verify <skill>
loom skill provenance outdated [<skill>] [--plan]
loom skill provenance refresh <skill>
loom skill policy <skill> [--policy-profile <safe-capture|audit-only|deny-risky|strict|custom>]
loom skill scan <skill> [--mode <install|activate|release>] [--strict]
loom skill trust <skill> --level <local-draft|reviewed|team-approved|third-party-unreviewed|blocked|quarantined>
loom skill quarantine <skill> [--reason <text>]
loom skill unquarantine <skill>
loom skill eval <skill> [--agent <agent> | --matrix <agent,agent>] [--model <model>]
loom skill eval offline <skill> [--agent <agent> | --matrix <agent,agent>] [--model <model>]
loom skill eval run <skill> --agent <agent> --baseline no-skill [--cases <path>] [--runs <n>] [--runner mock|codex-cli] [--dry-run] [--output <path>]
loom skill eval trigger <skill> --agent <agent> [--cases <path>] [--runs <n>] [--runner mock|codex-cli] [--output <path>]
loom skill eval compare <skill> --from <ref> --to <ref|working-tree> --agent <agent> [--cases <path>] [--runner mock|codex-cli] [--output <path>]
loom skill improve <skill> [--agent <agent>] [--workspace <path>] [--baseline <ref>] [--real-eval] [--dry-run]
loom skill regression <skill> [--agent <agent>] [--from <ref>] [--to <ref|working-tree>]
loom skill project <skill> --binding <binding-id> [--target <target-id>] [--method <symlink|copy|materialize>] [--dry-run]
loom skill commit <skill> [--message <msg>] [--from-projection | --from-source] [--binding <binding-id>] [--instance <instance-id>] [--preflight]
loom skill release <skill> [<version> | --anchor] [--preflight --baseline <ref>]
loom skill rollback <skill> [--to <ref> | --steps <n>] [--dry-run]
loom skill diff [--security] <skill> <from> <to>
loom skill history <skill> [--limit <n>] [--from <rev>] [--to <rev>] [--include-diff-stat] [--include-ops]
loom telemetry status
loom telemetry enable [--local-only]
loom telemetry disable
loom telemetry report [--skill <skill>] [--skillset <skillset>] [--agent <agent>] [--workspace <path>] [--since <date>]
loom telemetry export --format jsonl|csv --output <path> [--redacted]
loom telemetry purge [--before <date>] --dry-run
loom telemetry purge [--before <date>] --confirm <token>
loom skill trash add <skill> [--dry-run]
loom skill trash list
loom skill trash restore <skill> [--trash-id <id>]
loom skill trash purge <trash-id> [--dry-run]
loom skill lint <skill> [--strict | --compat | --fix]
loom skill diagnose <skill> [--agent codex] [--check all|drift]
loom skill watch [<skill>] [--debounce-ms <ms>] [--max-batch <n>] [--dry-run] [--once]
loom skill import-observed [--target <target-id>]
loom skill monitor-observed [--target <target-id>] [--once] [--interval-seconds <seconds>]
loom skill orphan list
loom skill orphan clean [--delete-live-paths] [--dry-run]

loom instruction scan [--agent <agent>] [--workspace <path>]
loom instruction show <instruction-id> [--workspace <path>]
loom instruction classify <path>
loom instruction doctor [--agent <agent>] [--workspace <path>] [--skill <skill>]
loom instruction migrate-plan <instruction-id> [--workspace <path>] --to <skill|reference|keep-instruction> [--name <skill>] --dry-run

loom package plan <skill:<skill>|skillset:<skillset>> --format agent-skills-archive [--agent <agent>] [--output-plan <path>]
loom package build <plan-artifact> --output <path> --idempotency-key <key>
loom package verify <artifact> [--format agent-skills-archive]

loom mcp requirement list --skill <skill> [--agent <agent>]
loom mcp plan --skill <skill> --agent <agent> [--workspace <path>] [--output-plan <path>]
loom mcp apply <plan-id|plan-artifact> --idempotency-key <key> [--approve <approval-token>...]
loom mcp doctor --agent <agent> [--skill <skill>] [--workspace <path>]
loom mcp catalog search <query>
loom mcp catalog show <server>

loom provision plan --target devcontainer [--workspace <path>] [--agent codex] [--output-plan <path>]
loom provision doctor --target devcontainer|codespaces|remote [--workspace <path>] [--agent <agent>] [--plan <plan-id|plan-artifact>]
loom provision apply <plan-id|plan-artifact> --idempotency-key <key> [--approve <approval-token>...]
loom provision export <plan-id|plan-artifact> --format devcontainer|shell|tar --output <path>
loom provision import <artifact> --dry-run

loom skillset create <skillset-id> [--description <text>]
loom skillset add <skillset-id> <skill-id> [--role <role>] [--required|--optional]
loom skillset remove <skillset-id> <skill-id>
loom skillset show <skillset-id>
loom skillset lint <skillset-id>
loom skillset activate <skillset-id> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--dry-run]
loom skillset deactivate <skillset-id> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--dry-run]
loom skillset eval <skillset-id> --agent <agent> [--baseline no-skill|single-skills]
loom skillset release <skillset-id> <version>
loom skillset rollback <skillset-id> --to <version|ref>

loom workflow create <workflow-id> --file <workflow.json> [--dry-run]
loom workflow create <workflow-id> --from-skillset <skillset-id> --dry-run
loom workflow show <workflow-id>
loom workflow plan <workflow-id> --agent <agent> --workspace <path>
loom workflow preflight <plan-id>

loom sync status
loom sync push [--dry-run]
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

Use this when the real source of truth is still an agent skill directory such as
`~/.claude/skills`, `~/.agents/skills`, or legacy `~/.codex/skills`.

```bash
loom monitor --once
loom monitor --interval-seconds 30
```

`loom monitor` is a short alias for `loom skill monitor-observed`. It imports new observed skills and updates existing registry copies when file content changes. It does not delete registry skills when an observed directory disappears; deletion stays an explicit cleanup action.

## Agent E2E (Recommended)

Run four real scenarios in one command (`.claude/skills`, `.claude-work/skills`,
multi-directory selection, legacy `.codex/skills` + failure feedback):

```bash
./scripts/e2e-agent-flow.sh                  # default output root
./scripts/e2e-agent-flow.sh /tmp/my-loom-e2e # custom output root
```

## Local Verification

Panel development and validation use Bun directly:

```bash
cd panel && bun install --frozen-lockfile
cd panel && bun run dev
cd panel && bun run typecheck
cd panel && bun run test
cd panel && bun run build
```

Run repository-wide gates from the root. The root Make targets orchestrate
Rust, Panel, e2e, release, and performance checks.

```bash
make fmt-check
make lint
make test
make e2e
make ci       # repository-wide gate
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

- Per-agent environment overrides beyond the default paths listed in [Supported Agents](docs/SUPPORTED_AGENTS.md).
- Convert observed agent directories to managed projection targets from the Panel once users opt in.
- Desktop packaging (Tauri) for users who prefer a GUI
- Optional upstream provider integrations for discovery and preview, while keeping Loom as the local control plane

## Community

- Issues: https://github.com/majiayu000/loom/issues
- Discussions: https://github.com/majiayu000/loom/discussions
