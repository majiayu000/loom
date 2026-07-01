# GH382 Tasks: Remote And Devcontainer Provisioning

Issue: https://github.com/majiayu000/loom/issues/382
Product spec: `specs/GH382/product.md`
Tech spec: `specs/GH382/tech.md`
Status: Blocked design packet

## Scope For First PR

Implement the dry-run-first provisioning foundation:

```text
provision plan + devcontainer/shell/tar outputs + doctor + idempotent apply gates
```

Do not implement:

```text
background daemon, direct cloud deployment without provider config, secret copying, org policy bypass
```

## Tasks

- [ ] `SP382-T001` Owner: plan-model | Done when: provision plans include target kind, workspace/container paths, agents, registry source plus cloneable URL, active views, skillsets, dependency readiness, reviewed file changes, Loom CLI prerequisite, secrets required, policy, and guards | Verify: `cargo test --test provision_cli`
- [ ] `SP382-T002` Owner: adapter-paths | Done when: target paths come from adapter metadata and Codex project scope uses `.agents/skills` | Verify: `cargo test --test provision_cli`
- [ ] `SP382-T003` Owner: devcontainer | Done when: devcontainer output is deterministic, idempotent, JSONC-aware, parameterized from reviewed paths, and fails safely on incompatible existing config | Verify: `cargo test --test provision_cli`
- [ ] `SP382-T004` Owner: export-import | Done when: shell/tar export and import dry-run are deterministic and never include secret values | Verify: `cargo test --test provision_cli`
- [ ] `SP382-T005` Owner: apply | Done when: provision apply revalidates guards, requires idempotency key, accepts and validates approval tokens when required, writes atomically, and returns recovery commands | Verify: `cargo test --test provision_cli`
- [ ] `SP382-T006` Owner: doctor | Done when: provision doctor is read-only and reports generated files, adapter paths, dependencies, required secrets, and policy state | Verify: `cargo test --test provision_cli`
- [ ] `SP382-T007` Owner: regression | Done when: focused and full repository checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP382-T1: Add Provision Plan Command

Owner: backend

Files:

- `src/cli.rs`
- new provision CLI module
- new provision command module
- tests

Done when:

- `provision plan --target devcontainer` returns a plan without target writes.
- Plan includes active skills, skillsets, dependency readiness, reviewed file
  changes, policy gates, and required secrets names.
- Plan records reviewed setup script content/patch digests, normalized
  `registry_clone_url`, target workspace paths, and Loom CLI prerequisite.
- Plan can be replayed from a durable command event or explicit plan artifact.
- Plan stores enough guards to revalidate apply.

Verify:

```bash
cargo test --test provision_cli
```

### SP382-T2: Use Adapter Metadata For Paths

Owner: backend
Depends on: SP382-T1
Blocked by: #373

Done when:

- Codex project active view path is `.agents/skills`.
- User/legacy roots are only used when selected by adapter metadata.
- Unsupported agent/scope path resolution fails clearly.

Verify:

```bash
cargo test --test provision_cli
```

### SP382-T3: Generate Devcontainer Output

Owner: backend
Depends on: SP382-T1, SP382-T2

Done when:

- Plan includes `.devcontainer/loom-setup.sh`.
- Plan includes structured changes for `.devcontainer/devcontainer.json`.
- Existing devcontainer files are parsed as JSONC.
- Existing incompatible config returns a merge conflict without overwrite.
- Generated shell script uses `set -euo pipefail`.
- Generated shell script defines plan-derived registry/workspace variables before
  use, normalizes `git+` registry sources to cloneable URLs, updates existing
  clones idempotently, verifies Loom CLI availability, materializes or verifies
  the reviewed active view, and checks every planned active skill.

Verify:

```bash
cargo test --test provision_cli
```

### SP382-T4: Add Shell/Tar Export And Import Dry-Run

Owner: backend
Depends on: SP382-T1

Done when:

- Shell export is deterministic.
- Tar export includes registry/active-view artifacts but no secret values.
- Import dry-run reports what would be applied.
- Import dry-run writes nothing.

Verify:

```bash
cargo test --test provision_cli
```

### SP382-T5: Add Apply Gate

Owner: backend
Depends on: SP382-T3, SP382-T4

Done when:

- Apply requires idempotency key.
- Apply accepts approval tokens and validates them against the reviewed plan
  policy decision when approval is required.
- Apply revalidates registry head, active-view digest, target paths, target-file
  preimage digests, generated content digests, and policy.
- File writes are atomic.
- Repeated apply with same key is idempotent.
- Recovery commands are returned.

Verify:

```bash
cargo test --test provision_cli
```

### SP382-T6: Add Provision Doctor

Owner: backend
Depends on: SP382-T1

Done when:

- Doctor checks target kind, workspace, generated files, registry remote,
  adapter paths, dependencies, required secrets names, and policy state.
- Doctor is read-only.
- Secret values are never printed.

Verify:

```bash
cargo test --test provision_cli
```

### SP382-T7: Full Verification

Owner: testing
Depends on: SP382-T1, SP382-T2, SP382-T3, SP382-T4, SP382-T5, SP382-T6

Done when:

- Focused tests cover every acceptance criterion.
- Full check and test suites pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #382` for design-only or partial provisioning slices. Use
`Fixes #382` only after plan, apply, export/import, doctor, idempotency,
redaction, and policy gates are complete.
