# Loom legacy design State Schema

更新日期: 2026-04-07  
适用版本: Loom legacy design.x (Breaking)

## 1. 目录布局

```text
state/
  legacy/
    schema.json
    targets.json
    ops/
      operations.jsonl
      checkpoint.json
    locks/
      skill/<skill>.lock
      global/sync.lock
    backups/<backup_id>/
      manifest.json
      ...
    audit/
      events-YYYYMMDD.jsonl
```

## 2. `state/legacy/schema.json`

用途: 声明 legacy 状态目录版本。legacy 不兼容 legacy 状态文件布局。

示例:

```json
{
  "schema_version": 2,
  "created_at": "2026-04-07T12:00:00Z",
  "writer": "loom/2.0.0"
}
```

约束:

1. `schema_version` 必须等于 `2`。
2. 读到未知主版本必须返回 `SCHEMA_MISMATCH`。
3. 检测到 legacy 状态文件时不做自动迁移，直接失败并提示手工重建。

## 3. `state/legacy/targets.json`

用途: 记录 skill 到目标目录的激活投影关系。

示例:

```json
{
  "schema_version": 2,
  "skills": {
    "loom": {
      "method": "symlink",
      "claude_path": "/Users/foo/.claude/skills/loom",
      "codex_path": "/Users/foo/.codex/skills/loom",
      "updated_at": "2026-04-07T12:00:00Z"
    }
  }
}
```

约束:

1. `method` 仅允许 `symlink` 或 `copy`。
2. `skills` key 为唯一 skill 名。
3. 写入采用临时文件 + rename 原子替换。

## 4. `state/legacy/ops/operations.jsonl`

用途: 记录所有变更操作的 durable journal。  
格式: 每行一个 JSON 对象，append-only。

行示例:

```json
{
  "op_id": "op_01HTY9VBGV9J7M9YYW3Y4T5R5Q",
  "request_id": "3d7f3e7f-54bd-4d58-a3b8-4fd3a2b8b82e",
  "intent": "skill.save",
  "status": "succeeded",
  "ack": false,
  "retry_count": 0,
  "created_at": "2026-04-07T12:00:00Z",
  "updated_at": "2026-04-07T12:00:01Z",
  "payload": {
    "skill": "loom",
    "message": "save(loom): event"
  },
  "effects": {
    "commit": "abc123..."
  },
  "last_error": null
}
```

字段定义:

1. `op_id`: 全局唯一、不可复用。
2. `intent`: `workspace.* | skill.* | sync.*`。
3. `status`: `queued | running | succeeded | failed`。
4. `ack`: 是否已被 `sync push/replay` 确认送达远端。
5. `payload`: 原始意图参数，要求可重放。
6. `effects`: 执行副作用摘要(commit/tag/path...)。

约束:

1. 同一 `op_id` 只能状态前进，不能回退。
2. `succeeded` 且 `ack=false` 才是 replay 候选。
3. 任何追加失败必须让命令失败，不允许吞错。

## 5. `state/legacy/ops/checkpoint.json`

用途: 记录 replay 游标与确认进度。

示例:

```json
{
  "schema_version": 2,
  "last_scanned_op_id": "op_01HTY9VBGV9J7M9YYW3Y4T5R5Q",
  "last_acked_op_id": "op_01HTY9VBGV9J7M9YYW3Y4T5R5R",
  "updated_at": "2026-04-07T12:10:00Z"
}
```

约束:

1. checkpoint 仅表示进度，不替代 journal 真相。
2. checkpoint 损坏时可通过重扫 journal 重建。

## 6. `state/legacy/locks/*`

用途: 并发互斥。

1. `state/legacy/locks/skill/<name>.lock`: skill 粒度写锁。
2. `state/legacy/locks/global/sync.lock`: sync/replay 全局锁。

建议内容:

```json
{
  "holder": "pid:12345",
  "request_id": "3d7f3e7f-54bd-4d58-a3b8-4fd3a2b8b82e",
  "acquired_at": "2026-04-07T12:00:00Z",
  "ttl_sec": 300
}
```

约束:

1. 锁文件通过 `create_new` 创建。
2. 进程异常退出后允许 stale lock 检测与清理策略。

## 7. `state/legacy/backups/<backup_id>/manifest.json`

用途: 初始化与高风险操作前的恢复点元数据。

示例:

```json
{
  "backup_id": "bkp_20260407_120000",
  "created_at": "2026-04-07T12:00:00Z",
  "sources": [
    {
      "agent": "claude",
      "path": "/Users/foo/.claude/skills",
      "skill_dirs": 37
    },
    {
      "agent": "codex",
      "path": "/Users/foo/.codex/skills",
      "skill_dirs": 41
    }
  ],
  "total_skill_dirs": 78,
  "checksum": "sha256:..."
}
```

## 8. `state/legacy/audit/events-YYYYMMDD.jsonl`

用途: 审计日志，不参与状态判定。  
每行事件示例:

```json
{
  "event_id": "evt_01HTYA0AF0K4Q7F7D51KR1Q4YV",
  "op_id": "op_01HTY9VBGV9J7M9YYW3Y4T5R5Q",
  "kind": "operation.succeeded",
  "ts": "2026-04-07T12:00:01Z",
  "meta": {
    "cmd": "skill.save",
    "request_id": "3d7f3e7f-54bd-4d58-a3b8-4fd3a2b8b82e"
  }
}
```

## 9. 初始化与重置规则(legacy-only)

1. 首次启动 legacy 时仅创建 `state/legacy/*`。
2. 若发现 `state/targets.json` 或 `state/pending_ops.jsonl`，默认报错并退出。
3. 用户需显式执行 `loom workspace init --reset-state` 进行状态重建。
4. `--reset-state` 仅重建 legacy 状态目录，不读取 legacy 内容。

## 10. 失败策略

1. schema 版本不匹配: 直接失败，不做降级读写。
2. 发现损坏行(JSON parse error): 标记 `STATE_CORRUPT` 并停止写入。
3. 允许 `loom workspace doctor --repair-state` 在人工确认后修复。
