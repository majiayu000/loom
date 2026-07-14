# GH510 Task Plan: Atomic Trash Activation Cleanup

Issue: https://github.com/majiayu000/loom/issues/510
Product spec: `specs/GH510/product.md`
Tech spec: `specs/GH510/tech.md`
Status: Draft for implementation under current `implx auto` authorization

## Scope

让 `skill trash add` 在安全边界内同时移除目标 Skill 的 active rules、projection records 与 Loom-managed symlinks，并提供 dry-run impact 与完整 pre-commit rollback。

不删除非 symlink projection 内容、bindings/targets，不自动恢复 activation，不发布 release。

## Tasks

- [ ] `SP510-T1` — Owner: coordinator; Done when: failing regressions reproduce dangling state, unsafe-path retention, dry-run impact and rollback gaps; Verify: `cargo test --test trash`.
- [ ] `SP510-T2` — Owner: coordinator; Done when: deterministic impact planner and safe-link classification satisfy P1-P3/P5; Verify: `cargo test --test trash`.
- [ ] `SP510-T3` — Owner: coordinator; Done when: cleanup and source move share a complete pre-commit rollback path satisfying P4/P6/P7; Verify: focused trash and doctor tests.
- [ ] `SP510-T4` — Owner: verification_owner; Done when: fresh deterministic checks and full suite pass; Verify: commands below.
- [ ] `SP510-T5` — Owner: gh510-merge-reviewer; Done when: independent review, CI, review threads and PR gate pass for the final head; Verify: current-head remote evidence.

### SP510-T1: Add Regression Coverage

Owner: coordinator

Files:

- `tests/trash.rs`
- `tests/doctor.rs` only if existing trash fixture cannot prove doctor health without duplication

Done when:

- Existing implementation freshly fails an active projected Skill trash regression.
- Tests cover multiple safe symlinks, unrelated activation preservation, missing path, unsafe symlink/regular path retention, dry-run no mutation, restore remaining inactive and injected rollback.
- Assertions inspect machine-readable impact and structured rollback errors rather than log text.

Verify:

```bash
cargo test --test trash
```

### SP510-T2: Plan And Apply Safe Activation Cleanup

Owner: coordinator
Depends on: SP510-T1

Files:

- `src/commands/trash_cmds.rs`
- `src/commands/trash_cmds/activation.rs`
- `src/fs_util.rs`
- `src/commands/codex_cmds.rs`
- `src/commands/skill_activation/apply.rs`

Done when:

- Planner loads existing registry state without mutating dry-run layout.
- Rules/projections are selected only by exact `skill_id`; bindings/targets remain unchanged.
- Deletable live paths satisfy target boundary, expected path, symlink type and source-target checks; paths are deduplicated.
- Apply 删除前重新验证 symlink target，并通过共享的 symlink-only primitive 删除；不得调用可递归删除目录的 helper。
- Stable JSON impact reports removed counts/IDs, deleted-link candidates and retained reasons.

Verify:

```bash
cargo test --test trash
```

### SP510-T3: Complete Transaction Rollback

Owner: coordinator
Depends on: SP510-T2

Files:

- `src/commands/trash_cmds.rs`
- `src/commands/trash_cmds/activation.rs`
- `tests/trash.rs`

Done when:

- All pre-commit failures restore source, registry state, original symlink targets, audit state and scoped Git index.
- Rollback failures are returned in `error.details.rollback_errors`.
- Successful trash leaves standard doctor checks healthy; restore returns source without activation.

Verify:

```bash
cargo test --test trash
cargo test --test doctor
```

### SP510-T4: Deterministic Verification

Owner: verification_owner
Depends on: SP510-T1, SP510-T2, SP510-T3

Done when:

- All fresh checks pass and large output is stored under `artifacts/logs/2026-07-14-loom-queue-t02`.

Verify:

```bash
git diff --check
cargo fmt --all -- --check
cargo test --test trash
cargo test --test doctor
cargo check --workspace --all-targets --all-features
make check
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH510-trash-convergence/specs/GH510
```

### SP510-T5: Independent Review And Merge Gate

Owner: gh510-merge-reviewer
Depends on: SP510-T4

Done when:

- Independent native reviewer maps P1-P7 and SP510-T1-T4 to the final diff and verification evidence.
- Current head CI passes, GraphQL review threads are resolved, merge state is clean, and offline PR gate returns `allowed`.
- Merge is remotely confirmed, Issue #510 closes, and the merged remote branch is deleted separately.

## Handoff Notes

- `src/commands/trash_cmds.rs` was 750 lines before this issue; new activation logic belongs in the searched-and-confirmed new submodule, not appended into the parent.
- Never delete `copy` / `materialize` content or any path whose Loom ownership cannot be proven.
- Current user authorization is `implx auto` plus explicit uninterrupted continuation; it authorizes merge after all gates, but does not waive independent review or verification.
