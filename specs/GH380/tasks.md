# GH380 Tasks: Provider Catalog And Safe Install

Issue: https://github.com/majiayu000/loom/issues/380
Product spec: `specs/GH380/product.md`
Tech spec: `specs/GH380/tech.md`
Status: Blocked design packet

## Scope For First PR

Implement provider/catalog foundations:

```text
provider config + locator parser + preview + install dry-run + provenance/lockfile plan
```

Do not implement:

```text
first-party hosted marketplace, auto-trust, preview script execution, direct gh skill install, automatic activation
```

## Tasks

- [ ] `SP380-T001` Owner: provider-config | Done when: provider add/list/remove persists deterministic `state/registry/providers.json` and malformed state fails without overwrite | Verify: `cargo test --test provider_cli`
- [ ] `SP380-T002` Owner: locator | Done when: GitHub/local/custom-provider locators parse into provider id, source location, subdir, requested ref, and pinned-ref status, while reserved-but-unsupported `team:` fails clearly | Verify: `cargo test --test provider_cli`
- [ ] `SP380-T003` Owner: catalog | Done when: catalog search/show return advisory results with warnings and do not write registry or target state | Verify: `cargo test --test provider_cli`
- [ ] `SP380-T004` Owner: preview | Done when: preview renders metadata, file tree, scripts, license/provenance hints, lint/safety summaries, and never executes code | Verify: `cargo test --test provider_cli`
- [ ] `SP380-T005` Owner: install-dry-run | Done when: `skill install --dry-run` resolves locator, evaluates pin policy, stages fetch plan, reports lint/safety/provenance/trust, and writes nothing | Verify: `cargo test --test provider_cli`
- [ ] `SP380-T006` Owner: install-apply | Done when: pinned install creates provenance, `loom.lock`, trust state, audit records, and registry import without auto-activation | Verify: `cargo test --test skill_provenance`
- [ ] `SP380-T007` Owner: policy | Done when: strict policy rejects unpinned refs using explicit or registry-default policy profile, critical scan findings block install, and public installs default to `third-party-unreviewed` | Verify: `cargo test --test skill_policy`
- [ ] `SP380-T008` Owner: regression | Done when: focused and full repository checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP380-T1: Add Provider Config

Owner: backend

Files:

- `src/cli.rs`
- new provider CLI module
- new provider command module
- provider state tests

Done when:

- Providers can be added, listed, and removed.
- Provider ids are validated.
- Provider URLs with embedded credentials or token-like query parameters are
  rejected before persistence.
- Provider records are sorted before write.
- Malformed state fails without overwrite.

Verify:

```bash
cargo test --test provider_cli
```

### SP380-T2: Add Locator Parser

Owner: backend
Depends on: SP380-T1

Done when:

- GitHub, local, and custom provider-id locators parse.
- `team:` locators are reserved but unsupported in v1 unless a later provider
  contract defines them.
- Subdirectory and ref syntax is deterministic.
- Pinned and moving refs are classified.
- Invalid locators fail with structured errors.

Verify:

```bash
cargo test --test provider_cli
```

### SP380-T3: Add Catalog Search And Show

Owner: backend
Depends on: SP380-T2

Done when:

- Search returns advisory catalog results.
- Show returns one normalized result.
- Missing providers return structured warnings/errors.
- Read commands do not mutate state.

Verify:

```bash
cargo test --test provider_cli
```

### SP380-T4: Add Safe Preview

Owner: backend
Depends on: SP380-T2

Done when:

- Preview uses isolated staging.
- Preview reports metadata, file tree, scripts, license, lint and safety
  summaries.
- Preview never executes scripts or build hooks.
- Preview writes no registry or target state.

Verify:

```bash
cargo test --test provider_cli
```

### SP380-T5: Add Install Dry-Run

Owner: backend
Depends on: SP380-T4

Done when:

- Dry-run resolves locator and pin policy.
- Dry-run uses explicit `--policy-profile` or the registry default policy
  profile; missing strict policy input fails closed for unpinned refs.
- Dry-run reports staging/fetch plan.
- Dry-run reports lint, safety, provenance, trust, and next actions.
- Dry-run writes nothing.

Verify:

```bash
cargo test --test provider_cli
```

### SP380-T6: Add Pinned Install Apply

Owner: backend
Depends on: SP380-T5

Done when:

- Pinned installs import into the registry.
- Provenance and `loom.lock` are written deterministically.
- Trust state is recorded.
- Command audit is recorded.
- No activation happens automatically.

Verify:

```bash
cargo test --test skill_provenance
```

### SP380-T7: Add Policy Gates

Owner: security
Depends on: SP380-T5, SP380-T6

Done when:

- Strict policy rejects unpinned moving refs.
- Critical scan findings block install.
- Public installs default to `third-party-unreviewed`.
- Secrets are never printed in provider config or preview output.

Verify:

```bash
cargo test --test skill_policy
```

### SP380-T8: Full Verification

Owner: testing
Depends on: SP380-T1, SP380-T2, SP380-T3, SP380-T4, SP380-T5, SP380-T6, SP380-T7

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

Use `Refs #380` for design-only or partial provider slices. Use `Fixes #380`
only when provider config, search/show, preview, dry-run install, pinned
install, provenance/lockfile, trust defaults, and policy gates are complete.
