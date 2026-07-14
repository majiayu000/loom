# Tech Spec

## Linked Issue

GH-510

## Product Spec

见 `product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Trash orchestration | `src/commands/trash_cmds.rs` | snapshot audit 后直接 rename source、记录 operation 并 commit | 未读取或清理 active registry state，是根因；文件已 750 行，需要拆分 |
| Activation ownership | `src/commands/skill_activation/*`, `src/commands/codex_visibility.rs` | activation 创建 rules/projections；已有安全 symlink ownership 判定 | cleanup 必须复用同一 ownership 语义，避免误删用户文件 |
| Symlink filesystem primitive | `src/fs_util.rs`, `src/commands/codex_cmds.rs`, `src/commands/skill_activation/apply.rs` | Codex reconcile 与 deactivate 原先各自维护相同的跨平台 symlink-only 删除函数 | 收敛为通用 primitive，确保 trash apply 复检后不会调用可递归删除目录的泛化 helper |
| Registry persistence | `src/state_model/persistence.rs` | `save_bindings_rules_projections` 原子写三份相关 state | 用 batch writer 保证 rules/projections 一致落盘 |
| Rollback helpers | `src/commands/file_ops.rs`, `src/commands/skill_cmds/shared.rs`, `src/commands/projections.rs` | 已有 path backup/restore、registry rollback、audit snapshot/restore | 可组成 pre-commit transaction，禁止另造静默 best-effort 路径 |
| Regression tests | `tests/trash.rs`, `tests/skill_activation.rs`, `tests/doctor.rs` | trash tests 只覆盖 source move/restore/audit rollback | 缺少 active projected Skill、dry-run impact 与 live-link rollback |

## 设计方案

1. 新增 `src/commands/trash_cmds/activation.rs`，集中实现 read-only impact planning、safe-link 分类、registry-state cleanup 与 live-link backup/restore；`trash_cmds.rs` 保留命令编排，避免超过 800 行。
2. 在任何 source/live/state mutation 前加载 registry snapshot，按 `skill_id` 选择 rules 与 projections，并保留 bindings/targets。
3. 对每个目标 projection 计算 registered target 下的 expected path，按规范化 path 去重。仅对实际安全指向 `skills/<skill>` 的 symlink 建立原始 link-target backup，并列为 deletable；missing 与 retained paths 进入稳定 impact report。apply 删除前再次校验目标，并复用 `fs_util::remove_symlink` fail closed，避免 plan/apply 间 path 替换导致泛化删除。
4. dry-run 只调用 planner，不执行 `ensure_layout`、audit、backup、rename、registry save 或 Git 操作。
5. apply 顺序为：建立所有必要备份 → 删除安全 symlink → 过滤并原子保存 rules/projections → rename source / 写 trash metadata → record operation → stage / commit。operation effects 与成功响应携带同一 impact summary。
6. 任一 commit 前失败统一回滚：恢复 registry snapshot、按原始 target 恢复 symlink、恢复 source/remove incomplete trash entry、恢复 audit snapshot、unstage 限定 paths。每个 rollback failure 通过现有 `rollback_errors` 结构返回，不只记录 warning。
7. commit 后 autosync failure 沿用现有写操作契约，不尝试回滚已提交 Git transaction。
8. `trash restore` 保持 schema v1 与 source-only 语义，不持久化 activation snapshot。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P7 | active-state planner + batch save | active projected Skill trash test；doctor test |
| P2/P3 | safe-link classifier and retained report | correct/wrong symlink、regular file、missing path、multi-target tests |
| P4 | transaction rollback coordinator | operation/state/link fault-injection tests and rollback-error assertion |
| P5 | read-only impact planner | dry-run state/head/audit/live-path invariance test |
| P6 | existing restore flow plus regression | trash → restore remains inactive test |

## 数据流

`TrashAddArgs` → load registry snapshot → deterministic `TrashActivationImpact` → dry-run JSON 或 apply backups → safe link deletion → filtered rules/projections batch save → source move + metadata → audit operation → scoped Git commit → additive result/meta。

## 备选方案

- 调用一次 `skill deactivate`：该命令需要 agent/scope selector，不能原子覆盖同一 Skill 的所有 projections，也会产生独立 operation/commit。
- 删除整个 target `<skill>` path：无法证明 ownership，会误删普通目录、copy/materialize 内容或用户替换的 symlink。
- 在 restore metadata 中保存 activation snapshot：扩大 schema 与 restore contract，并可能重建已过期 target；本 issue 明确采用 source-only restore。

## 风险

- Security: path 删除必须同时验证 registry ownership、registered target boundary 与 symlink target；不得跟随任意 link 或删除目录。
- Compatibility: 成功 JSON 只增加字段；bindings/targets 与 trash metadata schema 不变。
- Performance: registry 与 projection 集合为本地小规模 JSON；path 分类和去重为线性成本。
- Maintenance: transaction helper必须完整返回 rollback errors；不得把 restore failure 降级为 warning。

## 测试计划

- [ ] Regression-first: `cargo test --test trash`
- [ ] Doctor integration: `cargo test --test doctor`
- [ ] Static/build: `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --all-features`, `git diff --check`
- [ ] Full repository: `make check`
- [ ] Spec packet: `python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH510-trash-convergence/specs/GH510`

## 回滚方案

回滚本 PR 即恢复旧 trash 行为；无 persisted schema migration。若新 cleanup 出现兼容问题，可整体回滚，已完成的 trash 不会包含可自动恢复的 activation snapshot，用户需显式重新 activate。
