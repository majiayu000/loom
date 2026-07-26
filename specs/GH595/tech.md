# GH595 Tech Spec - 统一 workspace binding 匹配语义

Issue: https://github.com/majiayu000/loom/issues/595
Product spec: `specs/GH595/product.md`
Route: `write_spec`
Status: `implx auto`

## 1. Current Behavior

- `src/state_model/mod.rs` 已有 PR 内 partial `matches_workspace -> bool`，但
  `canonicalize_workspace_path` 捕获全部错误并回退 raw path，静默吞掉 permission、ELOOP 等错误。
- PR 已迁移 agent preflight、convergence、plan、provision、inspect 与 recommend 的一部分调用点。
- `src/commands/workflow_cmds/mod.rs`、`src/commands/skill_activation/plan.rs`、
  `src/commands/codex_visibility/report_support.rs` 与 `src/commands/skill_inventory.rs` 仍保留
  raw path matcher。
- `name` 有两类上下文：普通 project selector 按 final component；user-scope marker 表示全
  用户域。activation/workflow 使用 `name=user`，visibility 还兼容 `name=<profile>` 存量
  snapshot，必须显式保留，不可直接套用 project basename 语义。

## 2. Call-site Inventory

| Consumer | Registry binding semantics | Planned handling |
| --- | --- | --- |
| `agent_cmds.rs` preflight | project path/name | authoritative API + `IO_ERROR` |
| `convergence_status.rs` projection selector | project path/name | authoritative API；错误成为 projection axis error |
| `plan_cmds/converge.rs` | project path/name | authoritative API + `IO_ERROR` |
| `convergence_transaction/guards.rs` | sealed project selector | authoritative API + `IO_ERROR` |
| `provision/planner.rs` active/safety views | project path/name | authoritative API + `IO_ERROR` |
| `skill_inspect.rs` | project path/name | authoritative API + `IO_ERROR` |
| `skill_recommend.rs` | project path/name | authoritative API + `IO_ERROR` |
| `skill_recommend_active.rs` | optional project selector | `None` 保持全选；`Some` 走 authoritative API |
| `workflow_cmds/mod.rs` | project path；`name=user` 是 user marker | path 走 authoritative API；user marker 显式分支 |
| `skill_activation/plan.rs` | Project path；User `name=user` | Project 走 authoritative API；User marker 显式分支 |
| `codex_visibility/report_support.rs` | path selector；任意 legacy `Name` user binding 全域 | path 走 authoritative API；legacy user marker 显式分支 |
| `skill_inventory.rs` serialized Registry matchers | project path/name | deserialize `RegistryWorkspaceMatcher` 后复用 API |

`workspace_cmds/binding.rs` 与 `skill_activation/resolve.rs` 只比较 matcher identity 以查找/复用
持久化对象，不判定 workspace 是否命中，因此不是 matching consumer，不迁移。

## 3. Proposed Design

1. `RegistryWorkspaceMatcher::matches_workspace(&Path)` 返回 `std::io::Result<bool>`。
   `Name` 不访问 filesystem；两类 path matcher 先分别 normalize 再比较。
2. normalization 先把相对输入锚定到 `current_dir`。直接 canonicalize 成功即返回；仅当
   `ErrorKind::NotFound` 时逐级 pop，并保存 suffix。遇到第一个 existing canonical ancestor
   后逆序附回 suffix。任一级出现其他错误立即返回原始 typed `io::Error`。
3. command surface 用现有 `map_io` 或 `CommandFailure(ErrorCode::IoError, ...)` 转换错误。
   convergence status 的非 `Result` 公共 API 将 matcher I/O 记录为显式 `Error` axis，而不是
   空 selection；不得继续生成 `NotApplicable` success。
4. iterator `filter`/`any` 路径改为可短路的普通 loop 或 `collect::<Result<...>>()`，保证首个
   I/O error 返回到拥有的 command/result boundary。
5. user-scope marker 留在 scope-aware wrapper 中：activation/workflow 检查 `name=user`；
   visibility 为兼容存量数据保留任意 `Name => user-scope`。wrapper 注释并测试它与 project
   basename matcher 是不同语义。serialized inventory matcher 直接 serde decode，schema
   错误显式返回。

## 4. Affected Areas

1. `src/state_model/mod.rs`：权威 matcher、NotFound-only normalization 与边界单测。
2. Registry command consumers：agent、convergence、plan/guard、provision、inspect、recommend、
   workflow、activation、visibility。
3. `src/commands/skill_inventory.rs`：serialized Registry matcher 复用与 error propagation。
4. `specs/GH595/`：产品、技术和任务契约。

## 5. Verification Plan

1. `cargo test --locked state_model::matches_workspace_tests -- --nocapture`
2. focused command tests：agent preflight、recommend/inspect、workflow、activation、visibility、
   inventory 与 convergence selector 的现有/新增测试。
3. `rg -n 'workspace_matcher|workspace_matchers' src/commands` 人工审计：所有 path consumer
   指向 authoritative API；identity-only 与 user marker 分支有注释。
4. `cargo fmt --all -- --check`
5. `cargo check --locked`
6. `git diff --check`

## 6. Rollback Plan

revert GH595 的单个实现提交即可恢复旧 matcher；不涉及 schema、migration 或持久化数据回滚。

## 7. Product Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | `state_model/mod.rs` + all project consumers | matcher call-site `rg` audit + focused command tests |
| B-002 | canonical path normalization | symlink/relative/trailing-slash unit tests |
| B-003 | NotFound-only ancestor reconstruction | missing-suffix unit tests |
| B-004 | `io::Result` + command adapters | permission/ELOOP unit tests + command error regression |
| B-005 | `MatcherKind::Name` project behavior | name matcher unit tests |
| B-006 | activation/workflow/visibility scope wrapper | user-marker and project-name regression tests |
| B-007 | `skill_inventory.rs` serde decode | serialized inventory workspace boost + malformed matcher tests |
| B-008 | optional selector wrappers | active recommend no-workspace regression |
| B-009 | inventory table + search audit | `rg` command in verification plan |

## 8. Planned Changes Manifest

<!-- specrail-requires-planned-changes-v1 -->
<!-- specrail-planned-changes
{"version":1,"issue":595,"complete":true,"paths":["specs/GH595/product.md","specs/GH595/tech.md","specs/GH595/tasks.md","src/state_model/mod.rs","src/commands/agent_cmds.rs","src/commands/agent_cmds/planning_helpers.rs","src/commands/convergence_status.rs","src/commands/plan_cmds/converge.rs","src/commands/plan_cmds/convergence_transaction/guards.rs","src/commands/provision/planner.rs","src/commands/provision/utils.rs","src/commands/skill_inspect.rs","src/commands/skill_inspect/command.rs","src/commands/skill_inspect/evidence.rs","src/commands/skill_recommend.rs","src/commands/skill_recommend_active.rs","src/commands/workflow_cmds/mod.rs","src/commands/skill_activation/plan.rs","src/commands/skill_activation/mod.rs","src/commands/codex_visibility/report_support.rs","src/commands/codex_visibility.rs","src/commands/skill_inventory.rs"],"spec_refs":["specs/GH595/product.md#4-behavior-invariants","specs/GH595/tech.md#2-call-site-inventory","specs/GH595/tech.md#5-verification-plan"]}
-->
