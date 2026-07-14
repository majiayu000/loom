# GH512 Task Plan: First-party Loom Registry Agent Skill

Issue: https://github.com/majiayu000/loom/issues/512
Product spec: `specs/GH512/product.md`
Tech spec: `specs/GH512/tech.md`
Status: Draft for implementation under current `implx auto` authorization

## Scope

提供 collision-resistant 的 `loom-registry` Agent Skill、真实 trigger/compatibility regression、release archive 与 Homebrew package-share 打包，以及 fail-closed 安装和更新后的 agent runbook。

不创建 `loom` alias，不自动写用户 home，不覆盖既有 Skill，不发布 release。

## Tasks

- [ ] `SP512-T1` — Owner: coordinator; Done when: canonical Skill skeleton and regression fixtures exist and the pre-implementation gap is reproducible; Verify: focused test fails before packaging/docs are complete.
- [ ] `SP512-T2` — Owner: coordinator; Done when: Skill contract, metadata, manifest and trigger matrix satisfy P1-P4; Verify: skill-creator validation and focused test.
- [ ] `SP512-T3` — Owner: coordinator; Done when: archive, Homebrew package share and fail-closed install docs satisfy P5-P7; Verify: focused test and workflow source audit.
- [ ] `SP512-T4` — Owner: verification_owner; Done when: fresh deterministic checks and full suite pass; Verify: commands below.
- [ ] `SP512-T5` — Owner: gh512-merge-reviewer; Done when: independent forward-use validation, implementation review, CI, review threads and PR gate pass for final head; Verify: current-head remote evidence.

### SP512-T1: Create Regression Harness

Owner: coordinator

Files:

- `tests/shipped_registry_skill.rs`

Done when:

- Test copies the real tracked Skill into an isolated registry and never mutates the Loom checkout.
- Assertions cover canonical naming, no alias, strict portable lint, Claude/Codex compatibility, local positives, Loom.com/video negatives, and release/docs contract.
- The existing repository state can be shown to fail because the shipped Skill is absent.

Verify:

```bash
cargo test --test shipped_registry_skill
```

### SP512-T2: Author And Validate The Skill

Owner: coordinator
Depends on: SP512-T1

Files:

- `skills/loom-registry/SKILL.md`
- `skills/loom-registry/agents/openai.yaml`
- `skills/loom-registry/loom.skill.toml`
- `skills/loom-registry/evals/triggers.jsonl`

Done when:

- `skill-creator` initializes the searched-and-confirmed new directory and its validators pass.
- `loom-registry` triggers only for the local Loom registry/CLI domain and explicitly excludes Loom.com/video.
- Workflow uses explicit JSON/root, dry-run/approval checks, error/warning handling, current lifecycle commands, and links to detailed repo docs.
- Manifest uses only existing schema fields and declares `loom` as the required tool.

Verify:

```bash
python3 /Users/apple/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/loom-registry
cargo test --test shipped_registry_skill
```

### SP512-T3: Ship And Document Installation

Owner: coordinator
Depends on: SP512-T2

Files:

- `.github/workflows/release.yml`
- `README.md`
- `docs/AGENT_USAGE.md`
- `tests/shipped_registry_skill.rs`

Done when:

- Every platform archive includes the full Skill and unpack smoke validates required files.
- Generated Homebrew formula installs `skills/` under `pkgshare`; neither workflow nor docs writes user home without an explicit copy command.
- README documents fail-closed Claude/Codex copy from archive and Homebrew plus new-session discovery.
- Runbook names `loom-registry` and uses `skill commit` / `skill release --anchor`.

Verify:

```bash
cargo test --test shipped_registry_skill
rg -n 'skills/loom-registry|pkgshare.install|skill commit|release .*--anchor' .github/workflows/release.yml README.md docs/AGENT_USAGE.md
```

### SP512-T4: Deterministic Verification

Owner: verification_owner
Depends on: SP512-T1, SP512-T2, SP512-T3

Done when:

- All fresh checks pass and large output is stored under `artifacts/logs/2026-07-14-loom-queue-t02`.

Verify:

```bash
git diff --check
cargo fmt --all -- --check
python3 /Users/apple/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/loom-registry
cargo test --test shipped_registry_skill
cargo check --workspace --all-targets --all-features
make check
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH512-first-party-skill/specs/GH512
```

### SP512-T5: Independent Review And Merge Gate

Owner: gh512-merge-reviewer
Depends on: SP512-T4

Done when:

- Fresh subagent use validates the Skill from its actual path without relying on coordinator conclusions.
- Independent reviewer maps P1-P7 and SP512-T1-T4 to final diff and evidence.
- Current-head CI passes, GraphQL review threads are resolved, merge state is clean, and offline PR gate returns `allowed`.
- Merge is remotely confirmed, Issue #512 closes, and remote implementation branch is deleted separately.

## Handoff Notes

- `loom-registry` is the only canonical name; a shorter alias reintroduces the collision this issue fixes.
- Use the existing Loom lint/eval surfaces rather than a parallel trigger engine.
- Release workflow changes prepare future artifacts only. Current user authorization does not request publishing a version or release.
