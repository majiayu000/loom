# GH523 Tech Spec: 防止 CLI、Agent Skill 与生命周期文档契约漂移

Issue: https://github.com/majiayu000/loom/issues/523
Product spec: `specs/GH523/product.md`
Status: Draft for maintainer review

## Codebase Context

| Area | Current evidence |
| --- | --- |
| Shipped Skill metadata | `skills/loom-registry/SKILL.md:1-4` 与 `loom.skill.toml:1-9` 没有 CLI contract range |
| Skill regression tests | `tests/shipped_registry_skill.rs:42-98` 检查关键字符串/触发边界；`:143-172` 用 denylist 检查几个已删除命令 |
| CLI doc checks | `tests/cli_surface.rs:82-127` 检查 contract 中的固定词；`:164-235` 只检查 README 包含命令字符串，不调用 parser |
| Release packaging | `.github/workflows/release.yml:120-146` 将 binary 与 Skill 打入同一 archive，但 smoke 只运行 binary help/status 与文件存在性 |
| Runtime version | `src/envelope.rs:35-45` 的 `version` 是 crate package version，没有独立 contract identity |

## 设计

### 1. Contract identity

在源码中新增单一常量 `CLI_CONTRACT_VERSION: u32`（建议放在
`src/contract.rs`），JSON envelope additive 增加 `contract_version`。crate semver 描述发布版本，
contract version 只在 machine-facing command/envelope 发生不兼容变化时递增。

`skills/loom-registry/loom.skill.toml` 增加：

```toml
[compatibility]
min_cli_contract = 1
max_cli_contract = 1
```

Skill 正文继续要求先运行 `loom --version`/JSON read；兼容检查使用 envelope 中的整数，缺失按
legacy unknown 处理并阻止 mutation。

### 2. Agent-facing surface inventory

新增 review-owned `docs/agent-command-surfaces.toml`，每个 entry 包含稳定 id、path、classification
和 extraction marker：

```toml
[[surface]]
id = "agent_usage.daily_commit"
path = "docs/AGENT_USAGE.md"
classification = "executable"
```

分类闭集：`executable`、`output_example`、`legacy`。新增包含 `loom ` shell 命令的公开文件若未被
inventory 覆盖，check 失败。inventory 是审查边界，不自动生成或自动改写。

### 3. Parser-backed checker

新增只读 checker（优先 Rust integration test，避免跨平台 shell parser 差异）：

1. 从 fenced shell blocks 与显式 inline markers 提取 `loom` argv；
2. 使用 fixture map 替换 `$ROOT`、`<skill>` 等 placeholder，保留 command/flag 结构；
3. 调用 Clap `Cli::try_parse_from`，不执行 `App::execute`；
4. 对动态 shell assignment 无法静态解析时，surface 必须提供 companion argv fixture；
5. 报告 stable surface id、file、line、argv、Clap error。

对源代码中的 `next_actions`，新增测试通过已知 error fixtures 收集实际 JSON，然后对每个
`cmd` 做同一 parser 验证，避免只扫描字符串字面量。

### 4. Release pairing

release build 生成只读 `contract-manifest.json`，内容包括 release version、contract version、
Skill compatibility range、inventory digest、binary target。该文件与 binary/Skill 一起打包。
archive smoke 必须：

1. 从 packaged binary 读取 JSON contract version；
2. 解析 packaged Skill range；
3. 验证 manifest digest 与 inventory；
4. 运行 parser-backed fixture 集；
5. 对 mismatch fixture 断言 fail closed。

manifest 写入 staging/dist，不回写 source tree，因此取消构建不会留下 tracked partial artifact。

### 5. Compatibility policy

1. Additive command/field 默认不递增 contract version。
2. 删除/重命名 command/flag、改变字段含义或 required shape 时递增。
3. 缩小 Skill range 必须同时修改 `CHANGELOG.md`/migration note，并由测试检查同一 diff 中存在。
4. 老 CLI 缺少 `contract_version` 时 shipped Skill 可执行 read-only `--version`/`--help` 诊断，
   但不得执行 mutation。

### 6. CI integration

把 checker 接入 `make check` 与 release workflow。默认命令：

```bash
cargo test --test agent_contract_surfaces
cargo test --test shipped_registry_skill
```

checker 只读运行；测试在临时目录设置 `HOME`、registry root，不读取真实用户状态。

## Product-to-Test Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | envelope + Skill compatibility metadata | `cargo test contract_version_is_exposed_and_declared` |
| B-002 | `loom-registry` compatibility route | `cargo test --test agent_contract_surfaces incompatible_cli_blocks_mutation` |
| B-003 | `agent-command-surfaces.toml` coverage | `cargo test --test agent_contract_surfaces inventory_covers_public_surfaces` |
| B-004 | Clap parser checker | `cargo test --test agent_contract_surfaces executable_examples_parse` |
| B-005 | deleted command negative fixture | `cargo test --test agent_contract_surfaces removed_commands_fail` |
| B-006 | classification validation | `cargo test --test agent_contract_surfaces unclassified_command_fails` |
| B-007 | read-only snapshot test | `cargo test --test agent_contract_surfaces checker_is_read_only_and_repeatable` |
| B-008 | malformed surface fixture | `cargo test --test agent_contract_surfaces parse_failure_is_terminal` |
| B-009 | archive pairing smoke | release workflow local smoke + `cargo test packaged_contract_mismatch_fails` |
| B-010 | compatibility policy diff fixture | `cargo test contract_range_requires_migration_note` |
| B-011 | checker write guard | `cargo test --test agent_contract_surfaces checker_never_rewrites_sources` |
| B-012 | staging manifest test | `cargo test release_manifest_is_atomic_and_untracked` |
| B-013 | missing/empty fixture set | `cargo test --test agent_contract_surfaces missing_inputs_fail_closed` |

## 风险与回滚

1. **复杂 shell 示例误报**：允许显式 companion argv fixture，但不允许 skip；fixture 与文档通过
   stable surface id 绑定。
2. **命令 surface 扫描性能**：只 build 一次 test binary，所有 argv 使用 Clap parser 内存验证。
3. **双版本负担**：contract version 只描述 breaking machine contract，规则写入 ADR/contract。
4. **历史文档噪声**：legacy 文件显式登记，不纳入 executable gate。
5. **回滚**：可从 CI 暂时移除新 checker，但保留 additive envelope field 和 Skill range；不得
   回到 silent mismatch。

## 规格门禁

- 本 PR 只新增 SpecRail 规格，不实现 checker、manifest 或 metadata。
- 维护者需要批准 inventory 作为 review-owned source，而不是 generator-owned output。
