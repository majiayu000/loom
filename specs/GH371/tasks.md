# GH371 Tasks: Runtime Dependency And MCP Readiness

Issue: https://github.com/majiayu000/loom/issues/371
Product spec: `specs/GH371/product.md`
Tech spec: `specs/GH371/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the read-only readiness foundation:

```text
dependency declaration parser + tool/env/MCP/network checks + skill deps command + lint/inspect-ready helper
```

Do not implement:

```text
MCP install/apply, network probes, secret printing, mutating config changes
```

## Tasks

- [ ] `SP371-T001` Owner: cli | Done when: `skill deps <skill> [--agent] [--workspace]` parses and returns JSON-ready output | Verify: `cargo test --test cli_surface`
- [ ] `SP371-T002` Owner: parser | Done when: dependencies are read from `loom.skill.toml`, SKILL frontmatter metadata, compatibility text, scripts, and agent metadata with source attribution | Verify: `cargo test --test skill_deps`
- [ ] `SP371-T003` Owner: tools | Done when: tool checks use PATH lookup, optional argv version timeout, install hints, and no shell interpolation | Verify: `cargo test --test skill_deps`
- [ ] `SP371-T004` Owner: env-mcp-network | Done when: env values are redacted, MCP configured/missing/unknown states are reported, and network expectation is inferred without network calls | Verify: `cargo test --test skill_deps`
- [ ] `SP371-T005` Owner: integrations | Done when: readiness helper is reusable by `skill lint --quality`, future `skill inspect`, and Codex doctor without duplicating parsing | Verify: `cargo test --test skill_lint`
- [ ] `SP371-T006` Owner: docs | Done when: CLI contract and specs document declaration precedence, readiness decisions, redaction, and verification commands | Verify: `git diff --check`

### SP371-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs`
- `src/cli/deps.rs`
- `src/commands/mod.rs`

Done when:

- `loom skill deps <skill>` is available.
- `--agent` and `--workspace` selectors parse.
- Missing skill returns typed `SKILL_NOT_FOUND`.

Verify:

```bash
cargo test --test cli_surface
```

### SP371-T2: Add Declaration Parser

Owner: implementation

Files:

- `src/commands/skill_deps.rs`
- shared manifest parser if extracted

Done when:

- Manifest, frontmatter, compatibility text, scripts, and agent metadata can contribute requirements.
- Source precedence is deterministic.
- Malformed declarations become findings.

Verify:

```bash
cargo test --test skill_deps
```

### SP371-T3: Add Readiness Probes

Owner: implementation

Done when:

- Tools are checked without shell interpolation.
- Env presence is reported without values.
- MCP state supports configured/missing/unknown.
- Network is inferred without network calls.

Verify:

```bash
cargo test --test skill_deps
```

### SP371-T4: Integrate With Existing Surfaces

Owner: implementation

Done when:

- `skill lint --quality` can report missing dependency declarations or missing required local tools where appropriate.
- Future inspect/doctor integrations can reuse the same model.
- No duplicate parser is introduced.

Verify:

```bash
cargo test --test skill_lint
```

### SP371-T5: Verification And Handoff

Owner: implementation

Done when:

- Focused dependency tests pass.
- Full compile check passes.
- SpecRail packet validation passes.
- PR body uses `Refs #371` unless every acceptance criterion is implemented and verified.

Verify:

```bash
git diff --check
cargo test --test skill_deps
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/GH371-dependency-readiness-spec/specs/GH371
```

## Handoff Notes

- Use `Refs #371` for partial implementation slices.
- Keep all checks read-only.
- Never print env values.
- Report MCP unsupported states as unknown, not pass.
