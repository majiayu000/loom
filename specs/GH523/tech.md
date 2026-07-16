# GH523 Tech Spec: 防止 CLI、Agent Skill 与生命周期文档契约漂移

Issue: https://github.com/majiayu000/loom/issues/523
Product spec: `specs/GH523/product.md`
Status: Maintainer architecture decisions approved; follow-up spec amendment

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

在源码中新增单一 SemVer 常量 `CLI_CONTRACT_VERSION: &str`（建议由 package 内共享 contract
module 持有），JSON envelope additive 增加字符串 `cli_contract_version`。crate semver 描述软件
发布版本，contract SemVer 独立描述 agent-facing machine contract capability。

`skills/loom-registry/loom.skill.toml` 增加：

```toml
[compatibility]
cli_contract = ">=1.0.0,<2.0.0"
```

Skill 正文继续要求先运行 `loom --version`/JSON read；兼容检查按 SemVer requirement 判断。缺失、
非法或不在范围内均按 legacy/unsupported 处理并阻止 mutation。

### 2. Agent-facing surface inventory

新增 review-owned `docs/agent-command-surfaces.toml`。文件级 entry 只登记扫描边界；每个命令
example 另有稳定 id、path、行区间/marker 与 classification，禁止用整文件 classification 跳过
同文件中的 active 命令：

```toml
[[surface]]
id = "agent_usage"
path = "docs/AGENT_USAGE.md"

[[example]]
id = "agent_usage.daily_commit"
surface = "agent_usage"
start_marker = "<!-- contract:daily-commit:start -->"
end_marker = "<!-- contract:daily-commit:end -->"
classification = "executable"

[[panel_mutation]]
id = "panel.skill.trash"
label_path = "panel/src/components/..."
action_id = "skill.trash.add"
backend_route = "POST /api/v1/skills/{skill}/trash"
cli_argv = ["loom", "skill", "trash", "add", "<skill>"]
binding = "cli_equivalent"
```

分类闭集：`executable`、`output_example`、`legacy`、`non_command`。`non_command` 只允许不含
`loom` argv 的纯说明文本；包含 command 的值不能用它跳过 parser。marker 必须唯一、非重叠并
解析到具体行区间。
新增包含 `loom ` shell 命令的公开文件若未被 inventory 覆盖，或文件内 command 无 example
覆盖，check 失败。inventory 是审查边界，不自动生成或自动改写。

Panel mutation 使用独立 entry 将稳定 action id、label source、后端 route/handler 与公开 CLI argv
绑定。`binding` 闭集为 `cli_equivalent`、`no_cli_equivalent`；前者的 argv 必须通过同一 parser/public
visibility gate，后者必须包含 review-owned rationale 且不得填写伪造 argv。静态 scan 枚举 Panel
mutation action/label registry 与后端 mutation routes，要求每项恰有一个绑定，并校验 action → route
在实际 handler 中存在；删除/重命名 CLI、route 或 action 任一端都会使 coverage 失败。

### 3. Parser-backed checker

新增只读 checker，避免跨平台 shell parser 差异：

1. 从 inventoried active 文件的 fenced shell blocks 与所有 inline code spans 提取 `loom` argv；
   inline command 不要求预先带 marker，未分类时默认 executable 并使 inventory coverage gate 失败；
2. 使用 fixture map 替换 `$ROOT`、`<skill>` 等 placeholder，保留 command/flag 结构；
3. 调用同一 Clap parser，不执行 `App::execute`；解析成功后还要遍历匹配的 command path 与实际
   出现的每个 Arg，依据 Clap visibility metadata/public allowlist 拒绝 hidden/deferred command、
   flag 与 option；公开 command 不得使其 hidden 参数变成公开契约；
4. 对动态 shell assignment 无法静态解析时，surface 必须提供 companion argv fixture；
5. 报告 stable surface id、file、line、argv、Clap error。

`docs/agent-command-surfaces.toml` 同时维护 `[[next_action_emitter]]`，包含稳定 emitter id、源文件、
constructor/field selector、输出 shape（string/object）与覆盖它的 fixture ids。静态 coverage scan
枚举源码中所有 `next_actions` 构造、赋值和 typed producer；发现未登记 producer、登记项无 fixture
或 fixture 未实际发出对应 emitter id 时 gate 失败。fixture harness 必须通过 test-only observation trace
记录 `{emitter_id, payload}`，其中 emitter id 来自 review-owned inventory 并在 producer 被调用时写入；
不要求改变公开 JSON envelope，但禁止用另一个 producer 的同文 command text 交叉满足覆盖。随后从完整
emitter fixture matrix 收集实际 JSON，
同时支持字符串形式和对象形式 `{"cmd": ...}`。任何包含 `loom` command 的字符串都必须提取 argv
并通过同一 parser/public visibility 验证；纯说明文本必须以稳定 fixture id 显式分类为
`non_command`，不能因不是对象或缺少 `cmd` 字段被静默跳过。

新增 package 内 library target，并由它拥有 Clap schema、public visibility catalog 与
`CLI_CONTRACT_VERSION`。只暴露窄 `cli_contract` facade，例如 parser-backed
`validate_public_argv` 与 public command inventory；raw `Cli`、`App` 和 execution internals 不成为
受支持的 library contract。binary entrypoint 通过同一 library schema 解析，checker/integration
tests 直接调用该 facade，且不得调用 `App::execute`。

本决策不新增 public 或 hidden checker command/leaf，也不新增独立 workspace crate。若后续出现
真实跨 package reuse，再单独通过 ADR 评估 crate split。

### 4. Release pairing

release 与 Homebrew share staging 生成只读 `contract-manifest.json`，内容包括 release version、contract version、
Skill compatibility range、inventory digest、binary target 与 `binary_sha256`、`skill_tree_digest`。
该文件与 binary/Skill 一起打包，并由 Homebrew share 安装路径消费同一份已校验 manifest。
archive smoke 必须：

1. 从 packaged binary 读取 JSON contract version；
2. 解析 packaged Skill range；
3. 从 archive 内容重新计算并验证 binary、shipped Skill tree 与 inventory digest；
4. 运行 parser-backed fixture 集；
5. 对 mismatch fixture 断言 fail closed。

`binary_sha256` 对 binary 原始 bytes 计算。Skill tree digest 对 relative POSIX path 的 bytewise 排序
逐项哈希 `(entry_type, path, normalized_mode, length, payload)`；regular file payload 使用原始 bytes
（不改换行），mode 只保留 executable bit；symlink payload 是 link target，且逃出 Skill root 时失败；
目录本身不入 hash。inventory digest 同样对原始 bytes 计算，禁止平台相关的隐式规范化。

每次生成使用同一目标目录旁的唯一临时目录，写完所有文件后 flush/fsync，重新校验 manifest 与
三类 digest，再以原子 rename 发布完整目录。并发 publisher 通过目标 lock 串行化；lock 内若目标
已存在，仅允许 digest 完全相同的幂等成功，否则失败。取消/故障注入只能留下可清理的未发布临时
目录，消费者只读取原子发布目录，绝不读取 staging。源码树、Git index/refs 不得改变。

### 5. Compatibility policy

1. 新增 shipped Skill、公开 `next_actions` 或 active agent-facing surface 可依赖的
   command/flag/field/capability 时递增 contract minor；不产生新 capability 的澄清或 metadata 修正
   才可递增 patch。
2. 删除/重命名 command/flag、改变字段含义、required shape 或兼容语义时递增 contract major。
3. 兼容范围写入 append-only `docs/cli-contract-history.toml`；缩小 Skill range 必须新增历史记录并
   同时修改 `CHANGELOG.md`/migration note。range-policy gate 必须接收显式 diff base：PR CI fetch
   `github.event.pull_request.base.sha` 并以 `LOOM_CONTRACT_DIFF_BASE` 传入；push/local 路径也必须传入
   可读取的 base tree/SHA。base 缺失、浅克隆中不可达或解析失败均 fail closed。规则测试使用确定性的
   before/after fixture，集成测试再证明当前 diff 新增记录、保留旧记录且 migration note 同步存在。
4. 老 CLI 缺少 `cli_contract_version` 时 shipped Skill 可执行 read-only `--version`/`--help` 诊断，
   但不得执行 mutation。

### 6. CI integration

把 checker 接入 `make check` 与 release workflow。默认命令：

```bash
cargo test --test agent_contract_surfaces
cargo test --test shipped_registry_skill
```

PR workflow 在运行 compatibility range 集成测试前必须 fetch 明确的 base SHA，并设置
`LOOM_CONTRACT_DIFF_BASE`；不得把 checkout 后的最终 tree 当成 diff evidence。

checker 只读运行；测试在临时目录设置 `HOME`、registry root，不读取真实用户状态。

## Product-to-Test Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | envelope + Skill compatibility metadata | `cargo test cli_contract_semver_is_exposed_and_declared` |
| B-002 | `loom-registry` compatibility route | `cargo test --test agent_contract_surfaces incompatible_cli_blocks_mutation && cargo test --test agent_contract_surfaces new_skill_old_cli_blocks_mutation` |
| B-003 | surface + emitter + Panel mutation mapping coverage | `cargo test --test agent_contract_surfaces inventory_covers_public_surfaces && cargo test --test agent_contract_surfaces emitter_inventory_is_complete && cargo test --test agent_contract_surfaces emitter_fixture_identity_is_observable && cargo test --test agent_contract_surfaces panel_mutations_are_mapped` |
| B-004 | shared Clap parser + public visibility checker | `cargo test --test agent_contract_surfaces executable_examples_parse && cargo test --test agent_contract_surfaces hidden_commands_fail && cargo test --test agent_contract_surfaces hidden_flags_fail` |
| B-005 | deleted command negative fixture | `cargo test --test agent_contract_surfaces removed_commands_fail` |
| B-006 | classification validation | `cargo test --test agent_contract_surfaces unclassified_command_fails` |
| B-007 | read-only snapshot test | `cargo test --test agent_contract_surfaces checker_is_read_only_and_repeatable` |
| B-008 | malformed surface fixture | `cargo test --test agent_contract_surfaces parse_failure_is_terminal` |
| B-009 | archive/Homebrew pairing and canonical content digests | release workflow local smoke + `cargo test packaged_contract_mismatch_fails && cargo test packaged_contract_digests_match && cargo test homebrew_share_contract_matches` |
| B-010 | compatibility policy diff fixture | `cargo test contract_additive_capability_requires_minor_bump && cargo test contract_range_requires_migration_note_with_explicit_base && cargo test contract_range_missing_diff_base_fails` |
| B-011 | checker write guard | `cargo test --test agent_contract_surfaces checker_never_rewrites_sources` |
| B-012 | isolated staging + atomic publish | `cargo test release_manifest_is_atomic_and_untracked && cargo test release_manifest_concurrent_publish && cargo test release_manifest_cancel_before_publish` |
| B-013 | missing/empty inputs | `cargo test --test agent_contract_surfaces missing_inputs_fail_closed && cargo test packaged_contract_missing_inputs_fail_closed` |

## 风险与回滚

1. **复杂 shell 示例误报**：允许显式 companion argv fixture，但不允许 skip；fixture 与文档通过
   stable surface id 绑定。
2. **library facade 膨胀**：只导出 contract validation 与 binary runner 所需的窄接口；raw parser
   DTO、execution service 与 IO adapter 不承诺稳定公共 API。
3. **双版本负担**：contract SemVer 描述 agent-facing capability/compatibility，crate semver 描述
   软件发布；两者的 bump 规则写入 ADR/contract history 并由 diff gate 校验。
4. **历史文档噪声**：legacy 文件显式登记，不纳入 executable gate。
5. **回滚**：可从 CI 暂时移除新 checker，但保留 additive envelope field 和 Skill range；不得
   回到 silent mismatch。

## 规格门禁

- 本 follow-up PR 只修订 SpecRail 规格，不实现 library、checker、manifest 或 metadata。
- 维护者已于 2026-07-16 批准独立 contract SemVer、最小共享 parser library facade 与
  review-owned inventory。
- implementation-ready 仍要求本 follow-up spec amendment 合并，并在实现 PR 中通过完整 gate。
