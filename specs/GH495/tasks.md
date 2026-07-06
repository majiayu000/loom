# GH495 Tasks: Real Codex CLI Eval Runner

Issue: https://github.com/majiayu000/loom/issues/495
Product spec: `specs/GH495/product.md`
Tech spec: `specs/GH495/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement one complete real-runner slice:

```text
CodexCliRunner + trigger routing + compile --require-real-eval + tests
```

Do not implement:

```text
cross-ref real compare execution
Codex auth setup
network/model configuration management
default runner changes
state schema migration
```

## Tasks

### SP495-T1: Add Codex CLI Runner

Owner: implementation

Files:

- `src/commands/skill_eval_harness/codex_cli.rs`
- `src/commands/skill_eval_harness/runner.rs`
- `src/commands/skill_eval_harness/mod.rs`
- `src/commands/skill_eval_harness/report.rs`

Done when:

- `CodexCliRunner` prepares isolated workspaces;
- invokes `codex exec --json`;
- parses JSONL trace;
- scores task checks with existing check semantics;
- reports timeout/unparseable trace as typed `EVAL_FAILED`;
- cleanup reports are preserved.

Verify:

```bash
cargo test --test skill_eval
```

### SP495-T2: Route Trigger Eval Through Real Runner

Owner: implementation
Depends on: SP495-T1

Files:

- `src/commands/skill_eval_harness/mod.rs`
- `src/commands/skill_eval_harness/codex_cli.rs`

Done when:

- `skill eval trigger --runner mock` remains fixture/lexical;
- `skill eval trigger --runner codex-cli` invokes Codex CLI per trigger case;
- observed trigger values are redacted and scored against fixture expectations.

Verify:

```bash
cargo test --test skill_eval
```

### SP495-T3: Add Compile Real Evidence Flag

Owner: implementation
Depends on: SP495-T1

Files:

- `src/cli/skill_compile_args.rs`
- `src/commands/skill_compile/mod.rs`

Done when:

- `skill compile` default evidence remains `offline_fixture`;
- `skill compile --require-real-eval` runs Codex CLI eval and stores `mode: real_codex_cli`;
- real runner infrastructure failures fail loudly.

Verify:

```bash
cargo test --test skill_compile
```

### SP495-T4: Add Tests

Owner: implementation
Depends on: SP495-T1, SP495-T2, SP495-T3

Files:

- `tests/skill_eval.rs`
- `tests/skill_compile.rs`

Done when:

- fake `codex` binary covers success without network;
- missing authorization/executable gates remain typed;
- malformed JSONL fails typed;
- compile real evidence is asserted.

Verify:

```bash
cargo test --test skill_eval
cargo test --test skill_compile
```

### SP495-T5: Final Verification

Owner: implementation
Depends on: SP495-T1, SP495-T2, SP495-T3, SP495-T4

Done when:

- focused tests pass;
- formatting/check/clippy pass;
- full workspace tests pass;
- PR body maps acceptance criteria to evidence.

Verify:

```bash
git diff --check
cargo fmt --check
cargo test --test skill_eval
cargo test --test skill_compile
cargo check --workspace --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```
