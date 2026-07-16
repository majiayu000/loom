# GH534 Product Spec - Module size ceiling guard automation

Issue: https://github.com/majiayu000/loom/issues/534
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

模块行数上限只靠人工报告维护，已经漂移：`docs/module-ceiling-signal-report.md` 只覆盖 3 个早已修复的旧文件，而当前有 22 个非测试源文件超过 700 行（最大 981 行，`bb9b738` 复核），无任何 CI 护栏。

## 2. Goals

1. 行数上限由 CI 自动强制，不再依赖人工报告。
2. 现存超标文件进入显式 allowlist，新增超标文件直接 fail。
3. allowlist 只能收缩：新增条目必须挂拆分 issue。
4. signal report 与真实状态保持一致（刷新或由 guard 输出替代）。

## 3. Non-Goals

1. 不在本 issue 内拆分现存的 23 个超标文件。
2. 不改变 800 行硬上限本身的数值约定。
3. 不覆盖 `tests/`、生成代码与 `target/`。

## 4. Behavior Invariants

1. guard 对非测试 `src/**/*.rs` 生效，测试文件与 `#[cfg(test)]` 不强制。
2. allowlist 中每个文件必须带 issue 引用。
3. guard 失败输出具体文件、当前行数与上限，可直接定位。
4. guard 在本地（make）与 CI 行为一致。

## 5. Acceptance Criteria

1. 新增一个超过上限的新文件时 CI 失败。
2. allowlist 内文件不阻塞，但 guard 输出其当前行数与关联 issue。
3. allowlist 文件行数下降后可从 allowlist 移除且 guard 通过。
4. `docs/module-ceiling-signal-report.md` 刷新为当前真实清单或指向 guard 输出。

## 6. Edge Cases

1. 文件被重命名/移动后 allowlist 条目失配（应报错而非静默放行）。
2. 巨型生成文件（如 bindings）需要显式豁免类别。
3. allowlist 条目对应文件已删除（应要求清理条目）。

## 7. Open Questions

1. 上限取 700（当前观测线）还是 800（既有硬上限约定）？
2. guard 放 `scripts/` shell 还是并入 `perf-smoke` 风格的既有检查链？
