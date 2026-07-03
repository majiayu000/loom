# GH479 Product Spec - Remove silent degradation in safety and recovery

Issue: https://github.com/majiayu000/loom/issues/479
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

多个安全、provenance、patch recovery 路径会把错误转成空列表、默认 digest 或忽略恢复失败。这会让用户误以为系统安全或已恢复。

## 2. Goals

1. 影响安全、provenance、rollback/recovery 的错误必须 fail closed 或显式进入 envelope。
2. quarantine cleanup 不得在 registry snapshot 损坏时返回空 cleanup。
3. patch preimage restore 不得吞掉恢复失败。
4. provenance 关键文件写入必须符合原子写语义。
5. JSON digest 计算失败不得退化为空字节 digest。

## 3. Non-Goals

1. 不重写所有错误处理框架。
2. 不修改安全扫描规则本身。
3. 不改变 provider locator 的 pinning 策略。

## 4. Behavior Invariants

1. “没有 cleanup 项”必须与“无法读取 cleanup 依据”可区分。
2. recovery restore 失败必须告诉调用方哪些 path 未恢复。
3. provenance 文件写入失败必须保留旧文件或返回失败，不能留下半写文件作为成功结果。
4. digest 结果必须来自真实序列化输入。
5. 所有新增 warning/error 必须可由 JSON 调用方解析。

## 5. Acceptance Criteria

1. corrupt registry snapshot 下，quarantine cleanup 返回结构化失败或 warning。
2. patch rollback 中任一 preimage restore 失败会进入 command failure details。
3. `state/registry/sources.json` 与 `loom.lock` 写入使用 atomic write 路径。
4. JSON serialization failure 不会产生 `sha256` of empty bytes。
5. 每个旧 silent degradation 点都有回归测试。

## 6. Edge Cases

1. 目标 path 父目录不存在。
2. preimage 是非 UTF-8 文件。
3. atomic rename 跨设备失败。
4. snapshot 文件存在但 schema version 不支持。

## 7. Open Questions

1. quarantine cleanup 的 snapshot 读取失败应阻断整个 command，还是允许非 cleanup 部分继续并带 warning？
2. provenance 写入是否需要统一走 batch atomic writer？
