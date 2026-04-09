# Loom Agent 使用 Runbook

本文档定义 agent 如何稳定调用 Loom，目标是可复现、可审计、可回滚。

推荐先触发 `loom` 技能（`SKILL.md`），再执行本文档中的非交互命令。

> v2 为 breaking 版本：旧顶层命令（`loom init/save/status/...`）已移除，Agent 必须使用 `workspace/skill/sync/ops` 命令组。

## 1. 双模式约定

- 人类操作者：优先 `loom workspace init --wizard`（交互式选择）。
- Agent：优先非交互模式，固定使用 `--json` + 明确参数。

## 2. Agent 基本调用契约

- 固定带 `--json`，只解析 JSON envelope。
- 固定带 `--root <absolute_path>`，避免 cwd 漂移。
- 只把 `ok=true` 视为成功。
- `ok=false` 时根据 `error.code` 分支处理。
- 记录 `request_id` 到日志，保证可追踪。

JSON envelope 关键字段：

- `ok`
- `cmd`
- `request_id`
- `version`
- `data`
- `error.code`
- `error.message`
- `meta.warnings`
- `meta.sync_state`

## 3. 首次接管（推荐流程）

Agent 首次接管本机 skills 时，执行：

```bash
loom --json --root <repo_root> workspace init --from-agent both --target both
```

该命令默认顺序为：

1. 备份 agent 原 skills 目录到 `<repo_root>/state/backups/<timestamp>/`
2. 导入到 `<repo_root>/skills/`
3. 建立 symlink（或 `--copy`）

产出中必须校验：

- `data.backup.destination` 存在
- `data.backup.manifest` 存在
- `data.summary.imported >= 0`
- `data.summary.linked >= 0`

## 4. 日常操作建议（Agent）

1. 读取状态：`loom --json --root <repo_root> workspace status`
2. 保存变更：`loom --json --root <repo_root> skill save <skill>`
3. 关键节点快照：`loom --json --root <repo_root> skill snapshot <skill>`
4. 发布版本：`loom --json --root <repo_root> skill release <skill> vX.Y.Z`
5. 差异检查：`loom --json --root <repo_root> skill diff <skill> <from> <to>`
6. 远端同步：`loom --json --root <repo_root> sync push` / `sync pull`

## 5. 安全护栏

- 未经明确授权，不要默认使用 `--skip-backup`。
- 未经明确授权，不要默认使用 `--force` 覆盖同名 skill。
- 优先 symlink 模式；只有环境不支持时再使用 `--copy`。
- `meta.warnings` 不为空时，视为“成功但有风险”，需写入运行日志。
- `sync_state=LOCAL_ONLY` 或 `PENDING_PUSH` 时，不应宣称“远端已同步”。

## 6. 常见失败码处理

- `ARG_INVALID`：参数或输入路径错误，修正参数后重试。
- `SKILL_NOT_FOUND`：先导入或确认 skill 名称。
- `LOCK_BUSY`：稍后重试，避免并发写同一 skill。
- `REMOTE_UNREACHABLE`：网络或远端不可达，转入本地排队模式。
- `REMOTE_DIVERGED`：先 `sync pull` 再处理冲突，再 `sync push`。
- `PUSH_REJECTED`：按分歧流程处理，不要强推覆盖。
- `REPLAY_CONFLICT`：进入人工或高阶冲突处理流程。

## 7. 最小自动化脚本模式

```bash
# 1) 初始化（首次）
loom --json --root "$ROOT" workspace init --from-agent both --target both

# 2) 日常保存
loom --json --root "$ROOT" skill save "$SKILL"

# 3) 同步
loom --json --root "$ROOT" sync push
```

## 8. 人类快速入口

```bash
loom workspace init --wizard
```

该入口用于“安装后首跑”或“不想记参数”的场景；Agent 不应依赖交互式输入。
