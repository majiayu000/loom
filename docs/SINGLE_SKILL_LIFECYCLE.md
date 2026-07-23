# Single-Skill Lifecycle

Loom is easiest to use when you think about one skill at a time: create or
import it, validate it, activate it for one agent, verify that the agent can see
it, collect eval evidence, then release or roll back.

Upstream context:

- Agent Skills specification: <https://agentskills.io/specification>
- Agent Skills client implementation guide: <https://agentskills.io/client-implementation/adding-skills-support>
- Claude Code skills docs: <https://code.claude.com/docs/en/skills>
- Local Codex active-view plan: [plan/codex-active-view-projection-spec.md](plan/codex-active-view-projection-spec.md)

## Terms

- Source: the canonical registry-owned skill files under `skills/<skill>`.
- Target: an agent skill directory known to Loom.
- Active view: the small runtime directory set an agent scans for active skills.
- Active: Loom has desired state saying the skill should be in a target active
  view.
- Installed: skill files exist in a source or target location.
- Visible: the target agent can discover the skill in its active view.
- Enabled: agent config does not disable the visible skill by canonical
  `SKILL.md` path.
- Disabled-by-config: files exist, but agent configuration suppresses the skill.
- Restart-required: files or config changed and the agent may need a new session
  before it sees the change.

Projection is not the same as visibility. A symlink or copied directory proves
that files exist in a target path; visibility also depends on the agent scan
roots, config disables, collisions, and session reload behavior.

## Fast Path

Human-facing flow:

```bash
loom init
loom skill author new fixflow --template coding-workflow
loom skill lint fixflow --portable
loom skill lint fixflow --quality
loom skill scan fixflow
loom skill activate fixflow --agent codex --scope user --dry-run
loom skill activate fixflow --agent codex --scope user
loom skill visibility fixflow --agent codex
loom skill eval run fixflow --agent codex --baseline no-skill --runner mock
loom skill release fixflow v1.0.0
```

Automation should keep the registry root explicit and parse only JSON envelopes:

```bash
ROOT="$HOME/.loom-registry"
loom --json --root "$ROOT" workspace init --scan-existing
loom --json --root "$ROOT" skill author new fixflow --template coding-workflow
loom --json --root "$ROOT" skill lint fixflow --portable
loom --json --root "$ROOT" skill lint fixflow --quality
PLAN_JSON=$(loom --json --root "$ROOT" plan converge fixflow --from-source --agent codex --require-runtime)
# Review plan_id, plan_digest, effects, risks, conflicts, and approvals.
loom --json --root "$ROOT" apply "$PLAN_ID" --plan-digest "$PLAN_DIGEST" --idempotency-key "$REQUEST_ID"
loom --json --root "$ROOT" skill visibility fixflow --agent codex
```

Treat only `ok=true` as success. On `ok=false`, branch on `error.code` and keep
the `request_id` in logs.

## Create Or Import

Use `loom skill author new <name>` when Loom should create a registry-owned skill
source. Templates are lint-clean starting points:

```bash
loom skill author new fixflow --template coding-workflow
```

Use `loom skill add` when the source already exists locally or in Git:

```bash
loom skill add /path/to/fixflow --name fixflow
loom skill add github:owner/repo//skills/fixflow --name fixflow --ref v1.2.3
```

After editing `skills/<skill>` inside the registry source, use `plan converge
<skill> --from-source` and review its immutable plan before digest-confirmed
`apply`. The plan-only phase writes no source commit, projection, live path, or
remote effect. Use low-level `skill commit <skill> --from-source` only for explicit
recovery. Use `--from-projection --instance <id>` only when a live projection is
the reviewed input side.

Apply retries reuse the same plan id, plan digest, and idempotency key. Remote
transport runs after local commit/projection/visibility evidence. A remote
pending or restart-required outcome is partial, not failure and not proof of
current-session visibility; follow every returned blocker and next action.

## Validate

Run portable lint before activation:

```bash
loom skill lint fixflow --portable
loom skill lint fixflow --quality
loom skill deps fixflow
loom skill scan fixflow
```

`skill lint` checks Agent Skills frontmatter and structure. `skill deps` reports
tool, env var, MCP, and network readiness without printing secrets. `skill scan`
reports trust and safety risks before a skill reaches an active view.

## Activate And Diagnose

`skill activate` creates or repairs the managed target, binding, rule, and
projection chosen by the agent, scope, workspace, profile, and method:

```bash
loom skill activate fixflow --agent codex --scope user --dry-run
loom skill activate fixflow --agent codex --scope user
```

Use `--dry-run` first when a command will write target files or registry state.
For project scope, pass the workspace explicitly in automation:

```bash
loom --json skill activate fixflow --agent codex --scope project --workspace "$PWD" --dry-run
```

Read-only checks:

```bash
loom skill inspect fixflow --agent codex --workspace "$PWD"
loom skill diagnose fixflow --agent codex
loom skill visibility fixflow --agent codex --workspace "$PWD"
loom skill active list --agent codex --scope user
```

`skill inspect` gives a compact lifecycle card. `skill diagnose` joins source,
bindings, targets, projections, drift, and recent operation failures.
`skill visibility` explains active-view visibility for Codex, including
projection health, config disables when Loom can read them, external entries,
and restart guidance.

## Evaluate

`skill eval` is quality evidence, not a safety proof.

```bash
loom skill eval offline fixflow --agent codex
loom skill eval run fixflow --agent codex --baseline no-skill --runner mock
loom skill eval compare fixflow --from v1.0.0 --to working-tree --agent codex --runner mock
```

The mock runner is the safe default. Real agent runners are explicit opt-in and
may require additional environment authorization.

## Release And Roll Back

Use release anchors before risky work and semantic releases for stable versions:

```bash
loom skill release fixflow --anchor
loom skill release fixflow v1.0.0 --preflight --baseline main
loom skill diff fixflow v1.0.0 working-tree
```

Rollbacks are source-history operations. Dry-run first when automation is
deciding whether to apply:

```bash
loom skill rollback fixflow --to v1.0.0 --dry-run
loom skill rollback fixflow --to v1.0.0
```

After rollback, re-run lint, visibility, and any eval gate that protected the
release.

## Current Command Status

Current implemented commands include `plan converge`, digest-confirmed `apply`,
`skill author new`, `skill add`, `skill commit`,
`skill lint`, `skill deps`, `skill scan`, `skill activate`,
`skill deactivate`, `skill active list`, `skill inspect`, `skill diagnose`,
`skill visibility`, `skill eval`, `skill release`, `skill rollback`, `skill diff`,
and `codex reconcile`.

The Panel exposes the same two-stage convergence mutation only when backend
health reports apply capability. Do not assume a filesystem projection or a
remote `SYNCED` state is visible to an agent unless visibility or diagnose
evidence proves the active-view chain.
