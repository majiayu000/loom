# Loom Agent 使用 Runbook

本文档定义 agent 如何稳定调用 Loom，目标是可复现、可审计、可回滚。

推荐先触发 `loom` 技能（`SKILL.md`），再执行本文档中的非交互命令。

`loom init` 和 `loom monitor` 是保留给快速启动的顶层别名。Agent 自动化应优先使用显式的 `workspace/target/skill/sync/ops` 命令组，避免隐藏默认路径。

## 1. 双模式约定

- 人类操作者：可以使用 `loom init` 和 `loom monitor --once` 快速启动。
- Agent：优先非交互模式，固定使用 `--json` + 明确参数。

## 2. Agent 基本调用契约

- 固定带 `--json`，只解析 JSON envelope。
- 固定带 `--root <registry_root>`，避免 cwd 漂移；这里的 root 是可变 Git-backed skill registry，不是 Loom 源码仓库。
- 默认 `--json` 是紧凑单行输出；人类排查时可加 `--pretty`。
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

`meta.sync_state` 是 agent 做同步决策时应优先读取的顶层状态。部分命令也会在 `data.remote.sync_state` 中返回远端细节视图；两者同时存在时，先按 `meta.sync_state` 分支，再把 `data.remote.sync_state` 作为诊断信息记录。

## 3. 首次接管（推荐流程）

Agent 首次接管本机 skills 时，执行：

```bash
REGISTRY_ROOT="$HOME/.loom-registry"

loom --json --root "$REGISTRY_ROOT" workspace init --scan-existing
loom --json --root "$REGISTRY_ROOT" skill monitor-observed --once
```

该命令默认顺序为：

1. 初始化 `$REGISTRY_ROOT` 为 Git-backed Loom registry。
2. 将已存在的默认 agent skill 目录注册为 observed targets。
3. 扫描 observed targets，将包含 `SKILL.md` 的 skill 导入到 `$REGISTRY_ROOT/skills/`。

产出中必须校验：

- `workspace init` 返回 `data.initialized=true`
- `workspace init` 返回 `data.scanned=true`
- `skill monitor-observed --once` 返回 `ok=true`
- `meta.warnings` 为空，或被明确记录为“成功但有风险”

## 4. 日常操作建议（Agent）

1. 读取状态：`loom --json --root <registry_root> workspace status`
2. 保存变更：`loom --json --root <registry_root> skill save <skill>`
3. 关键节点快照：`loom --json --root <registry_root> skill snapshot <skill>`
4. 发布版本：`loom --json --root <registry_root> skill release <skill> vX.Y.Z`
5. 差异检查：`loom --json --root <registry_root> skill diff <skill> <from> <to>`
6. 远端同步：`loom --json --root <registry_root> sync push` / `sync pull`

## 5. 安全护栏

- 未经明确授权，不要默认使用 `--force` 覆盖同名 skill。
- 优先 symlink 模式；只有环境不支持时再使用 `--method copy`。
- `meta.warnings` 不为空时，视为“成功但有风险”，需写入运行日志。
- `sync_state=LOCAL_ONLY` 或 `PENDING_PUSH` 时，不应宣称“远端已同步”。
- 读命令（如 `workspace status`、`workspace doctor`、`target list`）不写 command event；不要把读命令当作审计记录来源。

## 6. 常见失败码处理

- `ARG_INVALID`：参数或输入路径错误，修正参数后重试。
- `SKILL_NOT_FOUND`：先导入或确认 skill 名称。
- `LOCK_BUSY`：稍后重试，避免并发写同一 skill。
- `REMOTE_UNREACHABLE`：网络或远端不可达，转入本地排队模式。
- `REMOTE_DIVERGED`：先 `sync pull` 再处理冲突，再 `sync push`。
- `PUSH_REJECTED`：按分歧流程处理，不要强推覆盖。
- `REPLAY_CONFLICT`：进入人工或高阶冲突处理流程。
- `QUEUE_BLOCKED`：远端不可写或依赖状态未解决，记录 pending op 并等待恢复。
- `GIT_ERROR` / `IO_ERROR`：底层 Git 或文件系统失败，保留原始 message 供排查。

## 7. 最小自动化脚本模式

```bash
# 1) 初始化（首次）
loom --json --root "$ROOT" workspace init --scan-existing
loom --json --root "$ROOT" skill monitor-observed --once

# 2) 日常保存
loom --json --root "$ROOT" skill save "$SKILL"

# 3) 同步
loom --json --root "$ROOT" sync push
```

## 8. 人类快速入口

```bash
loom init
loom monitor --once
```

该入口用于“安装后首跑”或“不想记参数”的场景；Agent 不应依赖交互式输入。
