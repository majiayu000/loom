# GH600 Tech Spec - 安全解析 Codex tool-read telemetry

Issue: https://github.com/majiayu000/loom/issues/600
Product spec: `specs/GH600/product.md`
Route: `write_spec`
Status: implx auto implementation and local verification complete; independent review and exact-head CI pending

## 1. 当前行为与锚点

- `src/commands/telemetry/ingest/codex.rs:71` 仅把
  `response_item.payload.type=function_call` 路由到 tool-read helper，并在有
  names 后校验稳定 identity/session/timestamp；因此 helper 静默返回空集合会
  把 recognized schema drift 伪装为 `Ignored`。
- `src/commands/telemetry/ingest/codex/tool_read.rs:3` 返回裸 `Vec<String>`；
  `exec_command` JSON 使用 `.ok()`，全文 read-token 搜索与全文 path 搜索互不
  关联，trusted-root 判断是 substring。
- `tests/fixtures/telemetry_ingest/codex/sessions/2026/07/session-codex.jsonl`
  只有一个 `exec_command` 正例；`tests/telemetry_ingest.rs:36` 的 fixture
  E2E 已断言 raw agent home/workspace 不持久化，但尚未覆盖 raw tool
  command/path、`exec` 与 schema rejection。
- `docs/LOOM_CLI_CONTRACT_OPERATIONS.md:492` 只声明 known root/read/non-read
  的高层行为，尚未声明 segment association、actual-home component 校验和
  recognized malformed schema 的 stable rejection。

## 2. 设计

1. 将 tool-read helper 改为 `Result<Vec<String>, &'static str>`。未知 function
   name 返回空集合；recognized `exec_command`/`exec` 的 schema 错误返回稳定
   reason，由 `codex.rs` 原样映射为 `ParseOutcome::Rejected`。
2. `exec_command` 严格解析 `arguments` JSON object 与 string `cmd`。
   `exec` 仅用保守 scanner 定位顶层 direct/awaited
   `tools.exec_command(...)` 调用并以完整 UTF-8 scalar 提取 object 中的
   quoted `cmd` string；dormant body、字符串/注释中的相似文本、缺失/非
   string `cmd` 或无法闭合的结构均 fail closed。
3. `tool_read/shell.rs` 使用 std-only shell tokenizer 与 operand classifier：
   - 在 single/double quotes 与 escape 状态外识别 segment boundary，保留
     `&&`/`||` conditional 状态；heredoc 全 command fail closed；
   - 每个 word 保留 single/double/unquoted/escaped provenance；仅 unquoted
     `~` 及 unquoted/double-quoted `$HOME`/`${HOME}` 可展开；Windows
     Get-Content 支持反斜杠 separator，反斜杠换行按 continuation 移除；
   - 每个 B-002 verb 使用独立的闭集 flag/value/program/path-option grammar，
     `sed` program、option value、unknown option、help/version 不进入
     operands，tail optional inline follow mode 与 `--` 显式建模；
   - command substitution、backtick、未闭合 quote/escape 等无法保守解释的
     segment 不授权 invocation。
4. trusted path validator 从 `HOME`（Windows fallback `USERPROFILE`）构造
   roots，支持语义等价的 `~`、`$HOME`、`${HOME}` 前缀；先拒绝任何
   `ParentDir`，再做 lexical normalization 和 `Path::strip_prefix`。普通 roots
   接受 root 下以 `<skill>/SKILL.md` 结尾的 normalized descendant（兼容
   `.system/<skill>` 等既有布局）；plugin cache 接受 component-aware
   `<home>/.codex/plugins/cache/**/skills/<skill>/SKILL.md`，且 `skills`
   之前至少有 plugin cache 子组件。
5. 保留 `codex::Context.read_skills` 的 per-turn 去重与现有
   `skill-entrypoint-read-<ordinal>` identity；重复的相同 stable `turn_id`
   context 保留集合与 ordinal，真实新 turn 才重置。拒绝不污染合法 retry。
6. 扩展 unit tests 与 tracked session fixture，覆盖：
   `exec_command`/`exec` 正例、quoted paths、chain/pipeline operand 隔离、
   free text、非 read、伪前缀、`..`、nested `.codex`、plugin cache 和全部
   recognized schema drift。E2E 断言 stable rejected counts、合法 invocation
   counts、重复 ingest 幂等及 raw command/path/prompt 不持久化。
7. 同步 CLI contract；若文档行号变化，更新
   `docs/agent-command-surfaces.toml` 的既有 line-range 元数据，不新增命令面。

## 3. 风险与回滚

- 保守 tokenizer 允许 false negative，不允许 false positive；未知复杂 shell
  语法保持不计数，不作为“尽力猜测”的理由。
- home env 缺失时 trusted roots 为空，候选 read 按 `Ignored` 不计数且不产生
  rejection；HOME/USERPROFILE mutation 后变量路径也保持 untrusted。
- 这是 parser/fixture/docs additive correction，无持久化 schema migration。
  回滚该 commit 即恢复旧 parser；已写 events 仍符合既有 telemetry schema。

## 4. Product-to-Test Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | provenance-aware shell segment/state scanner | `cargo test --locked --lib commands::telemetry::ingest::codex::tool_read::shell` |
| B-002 | per-verb operand grammar + conservative UTF-8 exec extractor | `cargo test --locked --lib commands::telemetry::ingest::codex::tool_read` |
| B-003 | `tool_read.rs` normalized actual-home path validator | `cargo test --locked commands::telemetry::ingest::codex::tool_read::tests::trusted_paths_are_component_aware` |
| B-004 | `tool_read.rs` typed parse result + `codex.rs` rejection routing | `cargo test --locked commands::telemetry::ingest::codex::tests::recognized_tool_schema_drift_is_rejected` |
| B-005 | `codex.rs` Context dedupe and stable record construction | `cargo test --locked commands::telemetry::ingest::codex::tests::current_exec_command_skill_read_is_parsed_once_per_turn` |
| B-006 | existing redaction/event/cursor path + expanded E2E assertions | `cargo test --locked --test telemetry_ingest codex_tool_reads_are_precise_rejected_and_private` |
| B-007 | existing structured injection parser/tests | `cargo test --locked commands::telemetry::ingest::codex::tests::rollout_context_and_structured_skill_are_parsed` |
| B-008 | unit missing-home/negative matrix + E2E fixture | `cargo test --locked commands::telemetry::ingest::codex::tool_read::shell::tests::missing_home_keeps_reads_untrusted_and_ignored && cargo test --locked --test telemetry_ingest` |

## 5. 验证计划

1. focused red：先加入 B-001/B-003/B-004/B-006 负例并确认现实现失败。
2. focused green：
   `cargo test --locked commands::telemetry::ingest::codex` 与
   `cargo test --locked --test telemetry_ingest codex_tool_reads_are_precise_rejected_and_private`。
3. 交付验证：
   `cargo fmt --all -- --check`、
   `cargo check --locked`、
   `cargo test --locked commands::telemetry::ingest::codex`、
   `cargo test --locked --test telemetry_ingest`、`git diff --check`。
4. 审计 durable `events.jsonl`、cursor 与 envelope，确认 fixture raw
   command/path/prompt 字符串均无命中。

## 6. Planned Changes Manifest

<!-- specrail-requires-planned-changes-v1 -->
<!-- specrail-planned-changes
{"version":1,"issue":600,"complete":true,"paths":["src/commands/telemetry/ingest/codex.rs","src/commands/telemetry/ingest/codex/tool_read.rs","src/commands/telemetry/ingest/codex/tool_read/shell.rs","tests/telemetry_ingest.rs","tests/fixtures/telemetry_ingest/codex/sessions/2026/07/session-codex.jsonl","docs/LOOM_CLI_CONTRACT_OPERATIONS.md","docs/agent-command-surfaces.toml","specs/GH600/product.md","specs/GH600/tech.md","specs/GH600/tasks.md"],"spec_refs":["specs/GH600/product.md#4-行为不变量","specs/GH600/tech.md#5-验证计划"]}
-->
