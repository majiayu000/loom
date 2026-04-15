# Loom

Rust-based skill registry and projection control plane.

## Language

- English is the primary documentation language in this repository.
- Chinese guide: [中文说明](docs/LOOM_COMPLETE_GUIDE_ZH.md)

## Notes

- Multi-directory behavior is explicit via `target add`; no implicit directory inference.
- Hard write guard: if `--root` points to the Loom tool repo itself, write operations are rejected. Use an independent skill registry repo for mutable operations.

## Command Surface

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

## Multi-Directory Example (Claude)

```bash
loom target add --agent claude --path "$HOME/.claude/skills" --ownership observed
loom target add --agent claude --path "$HOME/.claude-work/skills" --ownership observed

loom target list
```

## Agent E2E (Recommended)

Run four real scenarios in one command (`.claude/skills`, `.claude-work/skills`, multi-directory selection, `.codex/skills` + failure feedback):

```bash
./scripts/e2e-agent-flow.sh
```

Optional output root:

```bash
./scripts/e2e-agent-flow.sh /tmp/my-loom-e2e
```

## Local Verification Entrypoints

```bash
make fmt-check
make lint
make test
make panel-build
make e2e
make ci
```

## Pre-Commit Hook (Recommended)

Bind `cargo fmt` to every `git commit` so CI never flags format drift:

```bash
make install-hooks
```

The hook runs `cargo fmt --all -- --check` only when `.rs` files are staged,
and fails the commit if rustfmt would make changes. Disable with
`git config --unset core.hooksPath`.

## JSON Envelope

Most commands support `--json` for machine-readable output.
