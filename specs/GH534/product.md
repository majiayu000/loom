# GH534 Product Spec - Module size ceiling guard automation

Issue: https://github.com/majiayu000/loom/issues/534
Route: `write_spec`
State: `ready_to_implement`
Locale: `zh-CN`

## 1. Problem

模块行数上限只靠人工报告维护，已经漂移：`docs/module-ceiling-signal-report.md` 只覆盖 3 个早已修复的旧文件。按最终 guard 规则排除 `tests/` 路径与 `*_tests.rs` 后，当前有 21 个生产源文件超过 700 行（最大 981 行，`bb9b738` 复核），无任何 CI 护栏。

## 2. Goals

1. 行数上限由 CI 自动强制，不再依赖人工报告。
2. 现存超标文件进入显式 allowlist，新增超标文件直接 fail。
3. allowlist 只能收缩：新增条目必须挂拆分 issue。
4. signal report 与真实状态保持一致（刷新或由 guard 输出替代）。

## 3. Non-Goals

1. 不在本 issue 内拆分现存的 3 个超过 800 行的文件；按 #544、#545、#546 执行 split-on-touch。
2. 不改变 800 行硬上限本身的数值约定。
3. 不覆盖 `tests/`、生成代码与 `target/`。

## 4. Behavior Invariants

1. guard 对非测试 `src/**/*.rs` 的完整物理文件行数生效；`tests/` 路径和 `*_tests.rs` 等 test-only 文件排除，但生产文件内联的 `#[cfg(test)]` 代码仍计入文件总行数。
2. allowlist 中每个文件必须带 issue 引用。
3. guard 失败输出具体文件、当前行数与上限，可直接定位。
4. guard 在本地（make）与 CI 行为一致。

## 5. Acceptance Criteria

1. 新增一个超过 800 行且未在 allowlist 的生产文件时 CI 失败；700–800 行文件输出 warning。
2. allowlist 内文件不阻塞，但 guard 输出其当前行数与关联 issue。
3. allowlist 文件降到 800 行或以下后可移除条目且 guard 通过；仅下降但仍超过 800 时必须保留条目，并将更低的当前值作为新 baseline 接受显式 review 后更新。
4. `docs/module-ceiling-signal-report.md` 刷新为当前真实清单或指向 guard 输出。

## 6. Edge Cases

1. 文件被重命名/移动后 allowlist 条目失配（应报错而非静默放行）。
2. 巨型生成文件（如 bindings）需要显式豁免类别。
3. allowlist 条目对应文件已删除（应要求清理条目）。

## 7. Maintainer Decisions（2026-07-16）

1. 采用 800 行 hard-fail、700 行 warning band；800 行本身允许，超过 800 才失败。`bb9b738` 下为 3 个 hard violation + 18 个 warning。
2. guard 放在 `scripts/module-ceiling.sh`，使用独立 Makefile target，并接入 CI `verify` job 的 lint 之后。
3. allowlist 格式为 `path<TAB>baseline_lines<TAB>issue-ref`；初始只允许 #544、#545、#546 对应的 3 个文件。
4. 拆分采用 split-on-touch；跟踪 issue 立即建立，代码拆分留到下一次功能修改。
