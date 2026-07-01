# Codex Skill Visibility

Codex visibility is path-based and session-scoped. Loom can project a skill, but
Codex will only use it when the skill is in a scanned active-view root, no config
rule disables its canonical `SKILL.md` path, and the current Codex session has
loaded the updated view.

Upstream and local references:

- Agent Skills specification: <https://agentskills.io/specification>
- Agent Skills client implementation guide: <https://agentskills.io/client-implementation/adding-skills-support>
- Claude Code skills docs: <https://code.claude.com/docs/en/skills>
- Local Codex active-view plan: [plan/codex-active-view-projection-spec.md](plan/codex-active-view-projection-spec.md)

## Active-View Roots

Codex roots Loom models today:

- Preferred user active view: `~/.agents/skills`.
- Legacy user root: `${CODEX_HOME:-~/.codex}/skills`.
- Project active view: `.agents/skills` discovered from the current working
  directory up through the repository root.

Codex may also have admin/system roots such as `/etc/codex/skills` and bundled
system skills. Loom treats those as outside managed activation unless adapter
metadata or a visibility command reports them. They can still explain a
collision or an unexpected visible skill.

## Why A Symlink Is Not Enough

A Loom projection proves that files exist at a target path. Codex visibility
requires more:

1. The target path is inside a Codex active-view root.
2. The entry contains a valid `SKILL.md`.
3. The symlink target or copied file resolves to the canonical skill identity
   Codex checks.
4. `skills.config` does not disable that canonical `SKILL.md` path.
5. The Codex session has started or reloaded after the file/config change.

If any step fails, the skill can be installed but not visible, visible but not
enabled, or enabled only after a restart.

## Canonical Identity

For symlink projections, Codex visibility is based on the canonical `SKILL.md`
path behind the symlink. For copy or materialize projections, the runtime
`SKILL.md` path is the identity.

Skill names are useful for display and collision diagnostics, but Codex config
disable rules should be interpreted by path first. A matching name does not
prove that the active canonical path is enabled.

## Config Disables

`skills.config` can disable a skill by canonical `SKILL.md` path. Loom-managed
config repairs must be narrow:

- Loom may repair or report Loom-generated config entries that block an active
  Loom projection.
- User-authored disables are preserved and reported for manual review.
- A command must not delete broad user config just because a projection exists.

Use dry-run first:

```bash
loom --json skill visibility fixflow --agent codex --workspace "$PWD"
loom --json codex reconcile --dry-run
```

Apply only after reviewing the JSON plan and confirming it touches the intended
Loom-owned entries.

## New Session Guidance

Codex skills are session-scoped. After activation, deactivation, config repair,
or symlink repair, start a new Codex session when `skill visibility` or
`workspace doctor` reports `new-session-recommended` or `restart-required`.

Do not treat a successful filesystem write as proof that an already-running
Codex process has reloaded the skill.

## Diagnosis Commands

Read-only checks:

```bash
loom --json workspace status
loom --json workspace doctor
loom --json skill inspect fixflow --agent codex --workspace "$PWD"
loom --json skill diagnose fixflow --agent codex
loom --json skill visibility fixflow --agent codex --workspace "$PWD"
loom --json skill active list --agent codex --scope user
```

Write planning:

```bash
loom --json skill activate fixflow --agent codex --scope user --dry-run
loom --json codex reconcile --dry-run
```

Only apply after the dry-run output is reviewed:

```bash
loom --json skill activate fixflow --agent codex --scope user
loom --json codex reconcile --apply --fix-config
```

The JSON output is the automation contract. Branch on `ok`, `error.code`, and
specific check statuses instead of parsing human prose.

## Common States

| State | Meaning | Next action |
|---|---|---|
| Installed only | Files exist in source or a target, but no active rule wants them. | Activate or remove the stale files. |
| Active but missing projection | Loom desired state wants the skill, but target files are absent. | Run `skill activate --dry-run`, then apply. |
| Projected but disabled-by-config | Files exist, but `skills.config` disables the canonical path. | Review `codex reconcile --dry-run`; preserve user-authored rules. |
| Enabled but restart-required | Files/config changed after the current Codex session started. | Start a new Codex session. |
| External collision | A system, bundled, or manually installed skill has the same name. | Inspect paths; decide whether Loom or the external source owns the active skill. |

Keep the desired active set small. Full registry mirrors are migration input,
not the recommended Codex active-view model.
