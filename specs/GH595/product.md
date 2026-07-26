# GH595 Product Spec - 统一 workspace binding 匹配语义

Issue: https://github.com/majiayu000/loom/issues/595
Route: `write_spec`
State: `implx auto`
Locale: `zh-CN`
Complexity: medium

## 1. Problem

Registry binding 的 `path_prefix`、`exact_path` 与 `name` 匹配在多个命令中重复实现。
这些实现对 symlink、相对路径、尾随分隔符和不存在的路径后缀处理不同，导致同一个
workspace 在 projection、recommend、inspect、activation、workflow 与 inventory surface
上可能得到不同 binding 集合。现有统一 helper 还会把 permission、symlink loop 等 I/O
错误静默降级为 raw path 比较，使“无法判断”伪装成“不匹配”或成功输出。

## 2. Goals

1. 所有真正消费 `RegistryWorkspaceMatcher` 的 path binding 路径使用同一权威匹配契约。
2. symlink、相对路径、尾随分隔符与不存在后缀在所有 consumer 上得到一致结果。
3. 只有 `NotFound` 允许 deepest-existing-ancestor fallback；其他 I/O 错误显式失败。
4. 保留各 scope-aware surface 的 user-scope `name` marker 与 project-scope final-component
   name 的既有语义，不把两类 scope 静默合并。

## 3. Non-Goals

1. 不改变 Registry JSON schema、matcher kind 或 binding 持久化格式。
2. 不改变 binding/rule/projection 的创建、激活或健康状态。
3. 不把任意 I/O 错误转成“不匹配”、warning 或 raw-path success。
4. 不修改与 Registry binding 无关的同名/路径筛选数据模型。

## 4. Behavior Invariants

1. **B-001** `path_prefix` 与 `exact_path` 必须由一个权威 matcher API 比较；workspace 和
   matcher value 都使用同一 normalization，不得在 command 内复制 raw path 比较。
2. **B-002** 对存在路径，normalization 必须解析 symlink 和相对组件；等价的 symlink、
   relative 与 trailing-slash 表达必须产生相同 match 结果。
3. **B-003** 当且仅当 canonicalize 返回 `NotFound` 时，系统可逐级寻找 deepest existing
   ancestor，并在 canonical ancestor 后按原顺序附回不存在 suffix；不存在 suffix 不得导致
   raw-string fallback。
4. **B-004** canonicalize 的 `PermissionDenied`、symlink loop 和其他非 `NotFound` I/O
   错误必须作为 typed error 传播到命令边界；不得返回 `false`、panic、warning+success 或
   partial success。
5. **B-005** project-scope 的 `name` matcher 按 workspace final component 比较；缺少或非
   UTF-8 final component 时是明确的 non-match，不触发 path I/O。
6. **B-006** scope-aware user binding 不得被误解释为 project workspace basename：
   activation/workflow 继续使用 `kind=name,value=user` marker；visibility 为兼容存量 snapshot，
   继续把任意 `Name` matcher（例如 `value=default` profile marker）视为 user-scoped，并匹配
   任意 project workspace。
7. **B-007** serialized skill inventory 的 `workspace_matchers` 来自 Registry binding，参与
   workspace boost 时必须反序列化为同一 matcher 类型并复用同一 path/name 契约；malformed
   matcher 必须显式失败，不得用空字符串继续评分。
8. **B-008** 没有 workspace selector 的 surface 保持原行为：不执行 path I/O，并且不因统一
   matcher 引入新的筛选。
9. **B-009** 所有真正 Registry binding consumer 的调用点清单必须被实现与验证覆盖；新增
   consumer 不得绕过权威 API。

## 5. Acceptance Criteria

1. 权威 matcher 单测覆盖 symlink、relative、trailing slash、missing suffix、name 与
   non-matching prefix。
2. 负例覆盖 permission denied 与 symlink loop，并证明返回 typed error。
3. 至少一个此前遗漏的命令 consumer 回归测试证明统一语义生效。
4. 搜索确认 Registry binding consumer 不再保留 path matcher raw comparison。
5. focused tests、`cargo fmt --all -- --check`、`cargo check --locked` 与相关命令测试通过。

## 6. Edge Cases

1. matcher 和 workspace 都不存在但共享一个 existing ancestor。
2. matcher 存在、workspace 只缺少最后若干级；以及相反方向。
3. 相对路径依赖当前工作目录，normalization 后必须成为同一绝对 canonical path。
4. dangling symlink 的目标不存在时按 `NotFound` suffix 契约处理；symlink cycle 则显式错误。
5. user-scope marker（`name=user` 或 visibility 存量 `name=<profile>`）与 project basename
   matcher 适用上下文不同。

## 7. Boundary Checklist

| Boundary | Verdict |
| --- | --- |
| Empty / missing input | covered: B-003, B-005, B-007 |
| Error and failure paths | covered: B-004, B-007 |
| Authorization / permission | covered: B-004（filesystem permission） |
| Concurrency / race / ordering | N/A：matcher 是一次只读计算，不持有跨调用状态 |
| Retry / repetition / idempotency | covered: B-001, B-002 |
| Illegal state transitions | N/A：不修改 Registry 状态 |
| Compatibility / migration | covered: B-005, B-006, B-008 |
| Degradation / fallback | covered: B-003, B-004 |
| Evidence and audit integrity | covered: B-009 |
| Cancellation / interruption / partial completion | N/A：单次本地路径判定无可提交的部分状态 |
