# GH370 Tasks: Safety Scan, Trust, Quarantine, And Security Diff

Issue: https://github.com/majiayu000/loom/issues/370
Product spec: `specs/GH370/product.md`
Tech spec: `specs/GH370/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the safety foundation in safe slices:

```text
trust state + scan report + projection gate + quarantine metadata + security diff
```

Do not implement:

```text
org RBAC approvals, malware verdicts, script execution, automatic unsafe cleanup
```

## Tasks

- [ ] `SP370-T001` Owner: state | Done when: registry trust metadata loads absent-as-unknown, saves sorted records, rejects malformed state, and never writes `SKILL.md` | Verify: `cargo test --test skill_safety`
- [ ] `SP370-T002` Owner: scan | Done when: `skill scan` returns unified policy/safety findings with severity, path, message, suggested action, decision, and activation_allowed | Verify: `cargo test --test skill_safety`
- [ ] `SP370-T003` Owner: trust | Done when: `skill trust`, `skill quarantine`, and `skill unquarantine` mutate trust state with audit/commit semantics and preserve source files | Verify: `cargo test --test skill_safety`
- [ ] `SP370-T004` Owner: gate | Done when: blocked/quarantined skills fail before projection/activation target mutation with full report details | Verify: `cargo test --test skill_policy`
- [ ] `SP370-T005` Owner: diff | Done when: `skill diff --security` reports only security-relevant changed scripts/frontmatter/references and new risky patterns | Verify: `cargo test --test skill_safety`
- [ ] `SP370-T006` Owner: inventory | Done when: trust/quarantine state is visible in skill inventory/search and ready for future `skill inspect` | Verify: `cargo test --test skill_inventory_cli`
- [ ] `SP370-T007` Owner: docs | Done when: CLI contract and specs document trust levels, scan limits, gating, quarantine semantics, and verification commands | Verify: `git diff --check`

### SP370-T1: Add Trust State

Owner: implementation

Files:

- `src/state_model/mod.rs`
- registry persistence helpers
- new or existing command module tests

Done when:

- `state/registry/trust.json` loads and saves deterministically.
- Absent state means `trust=unknown`.
- Malformed trust state fails without overwrite.

Verify:

```bash
cargo test --test skill_safety
```

### SP370-T2: Add Skill Scan

Owner: implementation

Files:

- `src/cli/safety.rs` or `src/cli/policy.rs`
- `src/commands/skill_safety.rs` or `src/commands/skill_policy.rs`

Done when:

- Scan wraps existing policy findings.
- Additional instruction/script heuristics produce stable finding ids.
- Decisions map to allowed/review_required/blocked/quarantined.

Verify:

```bash
cargo test --test skill_safety
```

### SP370-T3: Add Trust And Quarantine Commands

Owner: implementation
Depends on: SP370-T1

Done when:

- Trust level updates are audited and committed.
- Quarantine marks the skill blocked from future activation/projection.
- Unquarantine clears quarantine but leaves trust conservative.
- Source files are preserved.

Verify:

```bash
cargo test --test skill_safety
```

### SP370-T4: Enforce Safety Gate

Owner: implementation
Depends on: SP370-T2, SP370-T3

Done when:

- `skill project` and future #367 activation path call the unified safety decision before target mutation.
- Error details include report and suggested actions.
- Existing policy tests remain valid.

Verify:

```bash
cargo test --test skill_policy
```

### SP370-T5: Add Security Diff

Owner: implementation

Done when:

- `skill diff --security` compares two refs without full unrelated diff output.
- New script, network, secret, destructive, frontmatter, and risky reference changes are highlighted.
- Missing refs fail with typed errors.

Verify:

```bash
cargo test --test skill_safety
```

### SP370-T6: Verification And Handoff

Owner: implementation

Done when:

- Focused safety/policy/inventory tests pass.
- Full compile check passes.
- SpecRail packet validation passes.
- PR body uses `Refs #370` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test skill_safety
cargo test --test skill_policy
cargo test --test skill_inventory_cli
cargo check --workspace --all-targets --all-features
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH370-safety-trust-spec/specs/GH370
```

## Handoff Notes

- Use `Refs #370` for partial implementation slices.
- Do not claim scan is a complete malware detector.
- Do not delete skill source or external target entries during quarantine.
- Keep `skill policy` and `skill scan` on one shared decision model.
