# Loom legacy design PRD

更新日期: 2026-04-07  
状态: Draft legacy (Breaking)

## 1. 背景

Loom 当前已经覆盖基础能力: `init/import/link/save/snapshot/release/rollback/sync/panel`。  
核心问题不是“功能缺失”，而是“边界不清晰导致可靠性风险”:

1. 读命令和写初始化路径耦合，启动存在副作用。
2. `pending_ops` 语义偏粗，`replay` 不是逐操作确认。
3. 锁和 preflight 没有覆盖全部写命令。
4. 命令层、领域层、git/io/交互层混在一个大模块。
5. panel 形态重复(内嵌 HTML 与 React 两套)。

## 2. 目标与非目标

### 2.1 目标

1. 将 Loom 升级为“可恢复、可验证、可演进”的技能注册中心。
2. 采用破坏性 CLI 升级，不提供任何 legacy 兼容入口。
3. 建立 `state/legacy` schema 与强约束状态机。
4. 建立可审计操作日志与幂等重放能力。
5. 让 agent 调用契约更稳定，减少“成功但状态不一致”。

### 2.2 非目标

1. 本期不做云托管控制面。
2. 本期不引入数据库依赖(保持文件后端)。
3. 本期不改变“Git-native”主方向。
4. 本期不提供 alias、双命令面或向后兼容层。

## 3. 目标用户与关键场景

1. 个人开发者: 管理本地 Claude/Codex skills，同步到远端。
2. Agent 自动化脚本: 需要 `--json` 稳定协议、可重放、可追踪。
3. 团队维护者: 需要版本发布、回滚、审计与冲突恢复。

关键场景:

1. 首次接管: 备份 -> 导入 -> 投影 -> 校验。
2. 日常维护: save/snapshot/release + 自动同步。
3. 异常恢复: remote 不可达、分叉、replay 冲突、锁冲突。

## 4. 设计原则

1. Read path 绝不写磁盘。
2. 所有写操作先落 journal，再执行 side effect。
3. 所有 destructive 操作自动生成恢复点。
4. 状态文件必须有 schema version。
5. 破坏性升级优先，拒绝隐式兼容行为。

## 5. 信息架构与领域模型

### 5.1 分层

1. `domain`: 规则与状态机，不依赖 CLI 和文件系统。
2. `application services`: 用例编排、preflight、错误映射。
3. `infrastructure adapters`: git/fs/locks/clock/id generator。
4. `interfaces`: CLI(JSON envelope) + panel API。

### 5.2 核心实体

1. `Workspace`: 根目录、受管分支、remote 策略、schema 版本。
2. `Skill`: 名称、来源、路径、指纹(hash)。
3. `Projection`: target(claude/codex)、method(symlink/copy)、health。
4. `RevisionArtifact`: commit/tag/snapshot/release 元数据。
5. `Operation`: `op_id/intention/status/retry/last_error/ack`。
6. `BackupSet`: 备份来源、manifest、checksum。

## 6. 功能需求

### 6.1 Workspace

1. `workspace init` 支持导入来源、目标、备份策略。
2. `workspace status` 返回 git/remote/pending/projection 健康信息。
3. `workspace doctor` 返回结构化检查报告与修复建议。

### 6.2 Skill 生命周期

1. `skill import/add/link/use/save/snapshot/release/rollback/diff` 保持原语义。
2. `import --force`、`rollback` 必须自动创建恢复点。
3. `link/use` 更新 projection 状态时必须原子写 targets。

### 6.3 Sync 与 Ops

1. `sync push/pull/replay/status` 基于 ops journal。
2. `ops list/retry/purge` 支持按 `status` 过滤与重试。
3. `replay` 仅处理 `ack=false` 且可重放操作，逐条确认。

### 6.4 输出与错误

1. 保持 envelope: `ok/cmd/request_id/version/data/error/meta`。
2. 新增 `data.op_id` 和 `meta.recovery_ref`(如适用)。
3. 错误码允许扩展: `SCHEMA_MISMATCH/STATE_CORRUPT/UNSUPPORTED_V1_COMMAND`。

## 7. 非功能需求

1. 并发: skill 粒度锁 + sync 全局锁。
2. 原子性: 所有状态文件通过 temp file + rename 写入。
3. 可观测: 操作事件写入 audit 日志(JSONL)。
4. 可恢复: 中断后可根据 journal 与 checkpoint 继续。
5. 可测试: 集成测试覆盖 diverged/replay/conflict。

## 8. CLI legacy 命令面

```bash
loom workspace init|status|doctor|remote
loom skill import|add|link|use|save|snapshot|release|rollback|diff
loom sync status|push|pull|replay
loom ops list|retry|purge
```

全局参数:

1. `--json`
2. `--root <abs-path>`
3. `--request_id <id>`
4. `--dry-run`
5. `--strict`
6. `--no-auto-sync`

## 9. 破坏性命令切换表(无兼容层)

| 旧命令(legacy) | 新命令(legacy) | legacy 行为 |
|---|---|---|
| `loom init ...` | `loom workspace init ...` | 新入口；旧命令直接报错并提示新命令 |
| `loom status` | `loom workspace status` | 新入口；读路径零副作用 |
| `loom doctor` | `loom workspace doctor` | 新入口；结构化诊断输出 |
| `loom import ...` | `loom skill import ...` | 新入口；`--force` 自动恢复点 |
| `loom add ...` | `loom skill add ...` | 新入口；统一锁与 preflight |
| `loom link ...` | `loom skill link ...` | 新入口；原子写 targets |
| `loom use ...` | `loom skill use ...` | 新入口；与 link 同实现路径 |
| `loom save ...` | `loom skill save ...` | 新入口；统一 journal |
| `loom snapshot ...` | `loom skill snapshot ...` | 新入口；写 artifact 元数据 |
| `loom release ...` | `loom skill release ...` | 新入口；记录发布 provenance |
| `loom rollback ...` | `loom skill rollback ...` | 新入口；返回 `recovery_ref` |
| `loom diff ...` | `loom skill diff ...` | 新入口；非破坏查询 |
| `loom remote status` | `loom sync status` | remote 视图收敛到 sync |
| `loom remote set <url>` | `loom workspace remote set <url>` | remote 管理归入 workspace |
| `loom sync push` | `loom sync push` | 保持同名；改为逐 op ack |
| `loom sync pull` | `loom sync pull` | 保持同名；pull 后基于 checkpoint replay |
| `loom sync replay` | `loom sync replay` | 保持同名；逐 op 幂等回放 |

发布规则:

1. legacy.0 起立即移除全部 legacy 命令实现。
2. 收到 legacy 命令时，返回 `UNSUPPORTED_V1_COMMAND`。
3. CLI `--help` 仅展示 legacy 命令面。

## 10. 实施里程碑

### M0 稳定性先行

1. 读命令零副作用。
2. 写命令统一 preflight 与锁。
3. 引入 `op_id` 到响应。

验收:

1. `workspace status/doctor` 不改动 git/index/state。
2. 并发写同 skill 必然返回 `LOCK_BUSY`。

### M1 Journal 化

1. 新增 `state/legacy/ops/operations.jsonl` 与 checkpoint。
2. `sync replay` 改为逐 op ack。
3. `queue append` 失败必须抛错，不再吞掉错误。

验收:

1. 人工构造 50 个 pending op，replay 后全部 ack。
2. 中途故障重启后可从 checkpoint 续跑。

### M2 命令面切换

1. 切换到 `workspace/skill/sync/ops` 命令组。
2. 删除 legacy 命令分支与解析路径。
3. runbook 全量改写到 legacy。

验收:

1. 输入任意 legacy 命令均返回 `UNSUPPORTED_V1_COMMAND`。
2. 所有自动化脚本通过 legacy 回归用例。

### M3 面板收敛

1. 选定单一面板栈并移除重复实现。
2. 增加 ops/recovery/sync-state 可视化。

验收:

1. panel API 与 CLI `workspace status` 一致。

## 11. 风险与对策

1. 破坏性切换会导致旧脚本失效。对策: 发布前统一改写官方 runbook 与 CI 脚本。
2. 文件后端在极端崩溃下可能损坏。对策: 原子写 + checksum + schema 校验。
3. 强切换窗口内故障放大。对策: 提供回滚到 legacy 二进制的运维手册(不是协议兼容)。

## 12. 验证与发布标准

1. 单测: domain 状态机与错误映射。
2. 集成测试: git remote/diverged/replay/conflict。
3. 并发测试: 锁冲突与重复 replay 幂等。
4. 回归: legacy runbook 命令全部通过。

## 13. 交付物

1. 本 PRD 文档。
2. `state/legacy` schema 文档: `docs/LOOM_LEGACY_STATE_SCHEMA.md`。
3. legacy-only runbook 与脚本清单。
