# GH482 Product Spec - Provider outdated and re-pin workflow

Issue: https://github.com/majiayu000/loom/issues/482
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

Provider-backed install 要求 immutable pinned ref，但没有 first-class outdated / re-pin 流程。安装时安全，安装后可能长期腐化。

## 2. Goals

1. 用户可以只读查看 provider-backed skills 是否落后于 provider head 或 latest ref。
2. 用户可以生成 re-pin/update plan，而不是直接写入 skill 内容。
3. pin policy 必须继续 fail closed：advisory head 不能替代 immutable ref。
4. 输出必须包含当前 pin、候选 pin、digest 和风险状态。

## 3. Non-Goals

1. 不自动更新所有 installed skills。
2. 不信任 unpinned branch/tag 作为最终 provenance。
3. 不在本 issue 内实现远端 marketplace。

## 4. Behavior Invariants

1. outdated 检查默认 read-only。
2. provider unreachable 必须与 up-to-date 可区分。
3. unpinned candidate 必须标为 advisory，不能进入 apply。
4. re-pin plan 必须要求明确 apply/review gate。
5. provenance verify 与 outdated 检查必须可以组合使用。

## 5. Acceptance Criteria

1. up-to-date provider-backed skill 输出 clean status。
2. outdated provider-backed skill 输出 current ref/digest 与 candidate ref/digest。
3. unreachable provider 输出 degraded/error status，不假装 up-to-date。
4. re-pin plan 不修改 source tree，直到显式 apply。
5. local digest source 与 GitHub pinned commit source 都有测试。

## 6. Edge Cases

1. provider source 已删除。
2. lockfile 与 sources.json 不一致。
3. candidate ref 可解析但 digest 不匹配。
4. 多个 skills 指向同一 provider source。

## 7. Open Questions

1. command 名称应为 `skill outdated`、`provider outdated`，还是 `skill provenance outdated`？
2. latest ref 来源是 provider API、Git default branch，还是 catalog metadata？
