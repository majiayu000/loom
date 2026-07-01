# GH384 Tasks: Compiled Runtime Interface

Issue: https://github.com/majiayu000/loom/issues/384
Product spec: `specs/GH384/product.md`
Tech spec: `specs/GH384/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement only the non-mutating compiler foundation:

```text
skill compile --dry-run + artifact manifest model + verify/list planning
```

Do not implement in the first PR:

```text
compiled activation writes, remote LLM summarization, eval benchmark claims,
automatic promotion to valid, provider publishing, or source mutation
```

## Tasks

- [ ] `SP384-T1` Owner: implementation | Done when: compile dry-run/list/verify CLI parses and command ids classify read-only operations correctly | Verify: `cargo test --test cli_surface`
- [ ] `SP384-T2` Owner: implementation | Done when: manifest, gate, token estimate, digest, and artifact plan models parse and serialize deterministically | Verify: `cargo test --test skill_compile`
- [ ] `SP384-T3` Owner: implementation | Done when: `skill compile --dry-run` returns layout, digest inputs, gate status, token estimates, and writes no files | Verify: `cargo test --test skill_compile`
- [ ] `SP384-T4` Owner: implementation | Done when: list/verify detect missing files, malformed manifests, stale digest, and blocked gates | Verify: `cargo test --test skill_compile`
- [ ] `SP384-T5` Owner: implementation | Deferred until #366/#367/#373: inspect reports artifact status and compiled activation rejects missing, stale, blocked, or invalid artifacts | Verify: `cargo test --test skill_compile`
- [ ] `SP384-T6` Owner: implementation | Done when: CLI docs, tests, and SpecRail packet cover first-slice acceptance criteria | Verify: `git diff --check && cargo check --workspace --all-targets --all-features`

### SP384-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs` or a split skill compile args module
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom skill compile <skill> --dry-run [--agent <agent>] [--profile <profile>]`
  parses successfully.
- `loom skill compile list <skill>` parses successfully.
- `loom skill compile verify <skill> [--artifact <id>]` parses successfully.
- command ids classify dry-run/list/verify as read operations.
- mutating compile without `--dry-run` is either not exposed or returns a typed
  not-implemented result until artifact writes are intentionally added.

Verify:

```bash
cargo test --test cli_surface
```

### SP384-T2: Add Compile Manifest And Plan Model

Owner: implementation
Depends on: SP384-T1

Files:

- new compiler model module
- new `tests/skill_compile.rs`

Done when:

- manifest, gate status, token estimate, source digest, and planned artifact
  path models parse and serialize deterministically.
- manifest records dependency gate status and generated content hashes for
  activation and sidecar files.
- artifact ids validate as safe path segments before any artifact path join.
- status values are constrained to `planned`, `experimental`, `valid`, `stale`,
  `blocked`, and `invalid`.
- gate values are constrained to `pass`, `warning`, `missing`, `blocked`, and
  `fail`.
- malformed manifests return typed errors.

Verify:

```bash
cargo test --test skill_compile
```

### SP384-T3: Implement Dry-Run Planner

Owner: implementation
Depends on: SP384-T1, SP384-T2

Commands:

- `loom skill compile <skill> --dry-run`

Done when:

- missing skills fail with the existing typed skill-not-found behavior.
- quarantined or blocked skills return blocked gate status.
- plan output includes artifact layout, digest inputs, gate status, and token
  estimates.
- no artifact, registry, target-agent, or lockfile writes occur.
- small skills report a no-op compile result with explanation.

Verify:

```bash
cargo test --test skill_compile
```

### SP384-T4: Implement Verification

Owner: implementation
Depends on: SP384-T2

Commands:

- `loom skill compile verify <skill> [--artifact <id>]`
- `loom skill compile list <skill>`

Done when:

- list reports known artifacts without mutating state.
- verify detects missing files.
- verify detects malformed manifest and malformed JSON sidecar files.
- verify rejects manifest skill/artifact identity mismatches.
- verify rejects unsafe artifact ids before path resolution.
- verify without `--artifact` checks every artifact for the skill in
  deterministic artifact-id order.
- verify detects source digest mismatch after source edits.
- verify detects generated activation or sidecar content hash mismatches.
- verify rejects absolute paths, `..` traversal, and canonical path escapes in
  indexed sidecar paths.
- verify validates generated `activation.md` or projected activation text
  itself, not only the source skill lint status.
- verify runs safety checks on generated activation/projection text, not only the
  source skill.
- verify prevents `valid` status when lint, safety, dependency, or eval gates
  are missing, blocked, or failed.
- verify requires eval evidence to match the generated content hashes.
- verify returns structured output that `skill inspect` can consume later.

Verify:

```bash
cargo test --test skill_compile
```

### SP384-T5: Wire Inspect And Activation Preconditions

Owner: implementation
Depends on: SP384-T2, SP384-T4

Files:

- existing inspect/status module after #366
- existing activation/projection module after #367 and #373

Done when:

- `skill inspect` can report compiled artifact status, digest freshness, gate
  results, and source fallback.
- compiled activation rejects missing, stale, blocked, or invalid artifacts with
  typed next actions.
- normal activation without `--compiled` works when no compiled artifacts exist.
- compiled activation remains deferred or explicitly blocked if #367/#373
  primitives are not present.

Verify:

```bash
cargo test --test skill_compile
cargo test --test skill_activate
```

### SP384-T6: Update Docs And End-To-End Verification

Owner: implementation
Depends on: SP384-T1, SP384-T2, SP384-T3, SP384-T4

Done when:

- CLI contract documents compile dry-run, list, verify, artifact layout, and
  no-source-replacement invariant.
- tests cover all first-slice acceptance criteria.
- SpecRail packet reflects deferred compiled activation and eval-gated claims.
- repository checks pass.

Verify:

```bash
git diff --check
cargo test --test skill_compile
cargo check --workspace --all-targets --all-features
```

## Handoff Notes

- Use `Refs #384` for a first-slice PR unless the PR implements compiled
  activation, inspect integration, artifact writes, verification, and every
  acceptance criterion from the GitHub issue.
- Do not use `Fixes #384` until compiled activation is wired and eval-backed
  artifact validity is implemented.
- Do not claim token savings or runtime quality improvements without eval
  evidence from #369.
