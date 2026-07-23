# Loom Agent 使用 Runbook

本文档定义 agent 如何稳定调用 Loom，目标是可复现、可审计、可回滚。

推荐先触发仓库随发行物提供的 [`loom-registry`](../skills/loom-registry/SKILL.md) Skill，再执行本文档中的非交互命令。该 Skill 只处理本地 Loom registry/CLI，不处理 Loom.com 视频请求。

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
- `meta.sync_state`（仅兼容 registry transport）
- `data.convergence.registry_transport`
- `data.convergence.projections`
- `data.convergence.visibility`

新 agent 必须优先读取 `data.convergence` 的三个独立状态轴。`meta.sync_state` 与 `data.remote.sync_state` 仅表示 registry Git transport/backlog，兼容保留到下一个 major version；它们不能证明 projection 已收敛，也不能证明当前 agent/session 已加载 Skill。

判定顺序：

1. `registry_transport` 判断 registry remote/backlog；
2. `projections` 判断实时文件、method、digest/链接证据；
3. `visibility` 判断 adapter 可见性与 `restart_required`；
4. 任一轴为 `unknown` / `error` / `stale=true`，或出现在 `incomplete_axes` 时，不得宣称完整收敛。
5. `complete=true` 只证明请求的状态轴证据已采集完成，不代表这些轴 healthy；仍必须逐轴判断，
   例如 projection 为 `missing` 时必须修复 projection。

合法示例：registry transport 为 `SYNCED`，同时 projection 为 `drifted`；这只表示远端同步，不是运行时完成。

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
2. canonical source 修改默认用 `loom plan converge <skill> --from-source`；只有在 `execution_enabled=true`、`safe_to_apply=true` 且审阅 exact effects/risks/conflicts 后，才用返回的 `plan_id` + `plan_digest` + caller-held idempotency key 执行 `loom apply`
3. 高风险写入预演：在 `skill project` / `skill rollback` / `skill trash add` / `skill trash purge` / `skill orphan clean` / `sync push` 后加 `--dry-run`；`skill rollback --preview` 仅作为兼容别名保留
4. `skill commit` / `skill project` / `skill visibility` / `sync` 保留为 typed conflict、partial outcome 和人工 recovery 的低层命令，不替代默认 convergence happy path
5. 创建恢复锚点：`loom --json --root <registry_root> skill release <skill> --anchor`
6. 发布版本：`loom --json --root <registry_root> skill release <skill> vX.Y.Z --preflight --baseline <ref>`
7. 差异检查：`loom --json --root <registry_root> skill diff <skill> <from> <to>`
8. 远端同步：`loom --json --root <registry_root> sync push` / `sync pull`

## 5. 安全护栏

- 未经明确授权，不要默认使用 `--force` 覆盖同名 skill。
- 优先 symlink 模式；只有环境不支持时再使用 `--method copy`。
- `meta.warnings` 不为空时，视为“成功但有风险”，需写入运行日志。
- `agent preflight` 和 `--dry-run` 返回 `ok=true` 不代表可以直接写入；必须同时检查 `data.safe_to_run=true`。`plan use` 返回 `ok=true` 只表示 plan 已持久化；`apply` 前仍要检查 `required_approvals`。
- `--dry-run` 只允许写 command audit，不应改变 registry ops、operation backlog、Git refs/index 或 live target 内容；`skill rollback --dry-run` 连 command audit 也不会追加。
- `registry_transport.state=LOCAL_ONLY` 或 `PENDING_PUSH` 时，不应宣称“远端已同步”。即使为 `SYNCED`，也必须独立检查 projection convergence 与 agent visibility。
- `plan converge` 是 plan-only boundary：除 immutable plan/command audit 外不得产生 domain mutation。apply 重试必须复用原 `plan_id`、`plan_digest` 和 idempotency key；remote 永远最后执行。
- `local_complete_remote_pending` 保留已验证的本地结果并要求同一 authority 重试 transport；`local_complete_restart_required` 要求 restart/new session 后重查。显式接受 restart 只影响 completion blocker，不会把 visibility 改写成 `visible`。
- 读命令（如 `workspace status`、`workspace doctor`、`target list`）不会修改 registry state、Git refs/index、live target 目录或 operation backlog；它们会写入 durable command event。registry 写操作审计以 `meta.op_id` / `/api/v1/ops` 为准。

## 6. 常见失败码处理

- `ARG_INVALID`：参数或输入路径错误，修正参数后重试。
- `STATE_NOT_INITIALIZED`：当前 root 还没有 registry state，先运行 `loom init` 或 `loom workspace init`。
- `TRASH_ENTRY_NOT_FOUND`：trash id 或对应 skill 的 trash entry 不存在，先用 `loom skill trash list` 刷新选择。
- `SKILL_NOT_FOUND`：先导入或确认 skill 名称。
- `LOCK_BUSY`：稍后重试，避免并发写同一 skill。
- `REMOTE_UNREACHABLE`：网络或远端不可达，转入本地排队模式。
- `REMOTE_DIVERGED`：先 `sync pull` 再处理冲突，再 `sync push`。
- `PUSH_REJECTED`：按分歧流程处理，不要强推覆盖。
- `REPLAY_CONFLICT`：进入人工或高阶冲突处理流程。
- `QUEUE_BLOCKED`：远端不可写或依赖状态未解决，保留 registry operation backlog 记录并等待恢复。
- `GIT_ERROR` / `IO_ERROR`：底层 Git 或文件系统失败，保留原始 message 供排查。

## 7. 最小自动化脚本模式

```bash
# 1) 初始化（首次）
loom --json --root "$ROOT" workspace init --scan-existing
loom --json --root "$ROOT" skill monitor-observed --once

# 2) 日常 source 收敛：先计划并保存完整 JSON 供审阅
PLAN_JSON=$(loom --json --root "$ROOT" plan converge "$SKILL" --from-source --agent codex --require-runtime)
PLAN_ID=$(printf '%s' "$PLAN_JSON" | jq -er 'select(.ok == true and .data.execution_enabled == true and .data.safe_to_apply == true) | .data.plan_id')
PLAN_DIGEST=$(printf '%s' "$PLAN_JSON" | jq -er '.data.plan_digest')

# 3) 人或上层 agent 审阅 effects/risks/conflicts/approvals 后执行
loom --json --root "$ROOT" apply "$PLAN_ID" --plan-digest "$PLAN_DIGEST" --idempotency-key "$REQUEST_ID"

# 4) 仅在 recovery 需要时使用低层状态/同步命令
loom --json --root "$ROOT" workspace status
```

## 8. 人类快速入口

```bash
loom init
loom monitor --once
```

该入口用于“安装后首跑”或“不想记参数”的场景；Agent 不应依赖交互式输入。
