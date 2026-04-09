# Loom

Rust-based Skill 管理工具（Git 原生后端）。

## 已实现能力

- Skill 目录管理：`skills/<skill>/...`
- 支持批量导入：从本地目录或 `claude/codex` skills 目录导入
- Git 版本语义映射：
  - `save` -> commit（event）
  - `snapshot` -> `snapshot/<skill>/<ts>-<sha>` 标签
  - `release` -> `release/<skill>/vX.Y.Z` 注释标签
  - `rollback` -> 从目标 ref 恢复 skill 路径并提交 revert commit
- Claude/Codex 投影：`symlink-first`，支持 `--copy` fallback
- 远端同步：`workspace remote set/status` + `sync status/push/pull/replay`
- 离线暂存：`state/pending_ops.jsonl`
- 纯文件元数据：`state/locks/`、`state/targets.json`、`state/pending_ops.jsonl`
- `workspace doctor`：包含 `git fsck` 和队列/目标文件检查
- 本地 panel：`loom panel --port 43117`

## 命令

```bash
loom workspace init [--from-agent claude|codex|both] [--target claude|codex|both] [--copy] [--force] [--backup-dir <dir>] [--skip-backup]
loom workspace init --wizard
loom workspace status
loom workspace doctor
loom workspace remote set <git-url>
loom workspace remote status
loom skill add <path|git-url> --name <skill>
loom skill import --source <dir> [--skill <name>] [--link] [--target claude|codex|both] [--copy] [--force]
loom skill import --from-agent claude|codex|both [--skill <name>] [--link] [--target claude|codex|both] [--copy] [--force]
loom skill link <skill> --target claude|codex|both [--copy]
loom skill use <skill> --target claude|codex|both [--copy]
loom skill save <skill>
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
loom panel [--port 43117]
```

> 注意：v2 起已移除旧顶层命令（如 `loom init`、`loom save`、`loom status`）。必须使用 `workspace/skill/sync/ops` 命令组。

推荐导入到可被 agent 直接操作的模式：

```bash
# 首次安装推荐：自动备份 + 导入 + 重建 symlink
loom workspace init --from-agent both --target both

# 交互式终端选择模式（推荐手动使用）
loom workspace init --wizard

# 从现有 agent 目录导入，并立刻重建 symlink
loom skill import --from-agent both --link --target both
```

`--json` 可用于机读输出，envelope 固定为：

- `ok`
- `cmd`
- `request_id`
- `version`
- `data`
- `error`
- `meta`

## 状态文件

- `state/pending_ops.jsonl`: 离线/同步失败待回放操作
- `state/locks/<skill>.lock`: skill 粒度锁
- `state/targets.json`: `link/use` 目标映射

## 说明

- `sync push` 仅 fast-forward 语义（通过 fetch + behind 检查实现）。
- 无远端时，写操作会进入 `PENDING_PUSH` 并写入 pending 队列。
- 当前 panel 是可运行的本地监控面板（health/skills/remote/pending）。
- Agent 调用规范见 `docs/AGENT_USAGE.md`。
