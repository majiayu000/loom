# Loom

Rust-based skill registry and projection control plane (v3-only).

## 说明

- 不提供向后兼容命令。
- 已移除：`workspace init`、`skill import/link/use`、`migrate v2-to-v3`。
- 多目录通过 `target add` 显式注册，不再靠隐式目录推断。
- 写操作硬保护：当 `--root` 指向 loom 工具仓库本身时会拒绝执行，请使用独立 skill registry 仓库。

## 命令面（当前实现）

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

## 多目录示例（Claude）

```bash
loom target add --agent claude --path "$HOME/.claude/skills" --ownership observed
loom target add --agent claude --path "$HOME/.claude-work/skills" --ownership observed

loom target list
```

## Agent E2E（推荐）

一键运行四个真实场景（`.claude/skills`、`.claude-work/skills`、多目录选择、`.codex/skills` + 失败反馈）：

```bash
./scripts/e2e-agent-flow.sh
```

可指定输出根目录：

```bash
./scripts/e2e-agent-flow.sh /tmp/my-loom-e2e
```

## 本地验证入口

```bash
make fmt-check
make lint
make test
make panel-build
make e2e
make ci
```

## JSON Envelope

`--json` 输出固定 envelope：

- `ok`
- `cmd`
- `request_id`
- `version`
- `data`
- `error`
- `meta`

## 状态文件

- `state/locks/`
- `state/pending_ops.jsonl`
- `state/pending_ops_snapshot.json`
- `state/pending_ops_history/`
- `state/v3/schema.json`
- `state/v3/targets.json`
- `state/v3/bindings.json`
- `state/v3/rules.json`
- `state/v3/projections.json`
- `state/v3/ops/`
