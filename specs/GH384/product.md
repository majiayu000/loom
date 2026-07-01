# GH384 Product Spec: Compiled Runtime Interface

Issue: https://github.com/majiayu000/loom/issues/384
Parent: https://github.com/majiayu000/loom/issues/376
Status: Draft for implementation
Locale: en-US

## Goal

Add an optional compile step that turns portable skill source into smaller,
structured, agent-specific runtime artifacts while preserving `SKILL.md` as the
source of truth.

The first mergeable slice is read-only by default:

```bash
loom skill compile <skill> [--agent <agent>] [--profile <profile>] --dry-run
loom skill compile verify <skill> [--artifact <id>]
loom skill compile list <skill>
```

## Users

1. Individual users who want lower activation token cost for large skills.
2. Maintainers who need a diagnosable runtime interface derived from portable
   skill source.
3. Future advanced features that need bounded activation artifacts, including
   capability recommendation, workflows, provisioning, and telemetry.

## Scope For First PR

The first implementation PR should define and implement the compile planning
and verification foundation:

- `skill compile --dry-run` returns planned artifact paths, token estimates,
  source digest inputs, and gate status without writing files.
- `skill compile list` reports known artifacts when artifact storage exists.
- `skill compile verify` validates manifest shape, required files, source
  digest freshness, lint status, safety status, and eval status.
- Artifact writes remain explicit and separate from dry-run planning.
- Normal `skill activate` remains valid without compiled artifacts.

## Non-Goals

1. Do not replace portable `SKILL.md` as source of truth.
2. Do not require compiled artifacts for basic activation.
3. Do not hide source content, references, scripts, or assets from users.
4. Do not claim runtime, quality, or correctness benefits without eval
   evidence.
5. Do not execute generated artifacts during compile or verify.
6. Do not introduce remote LLM calls in the compiler path.
7. Do not bypass trust, quarantine, dependency, or policy gates.

## Behavior Invariants

1. `--dry-run` never writes artifact files, state files, agent target files, or
   lockfiles.
2. Every artifact is derived from one source skill and one source digest.
3. A digest mismatch makes an existing artifact stale and invalid for compiled
   activation.
4. A compile artifact may be smaller than `SKILL.md`, but the command must
   report no-op when compilation would not reduce useful runtime context.
5. The compiler may summarize activation instructions, but it must preserve
   safety constraints, trigger boundaries, non-goals, dependencies, and tool
   requirements.
6. References, scripts, and assets are indexed rather than inlined unless the
   selected agent profile explicitly requires inlining.
7. Eval evidence is mandatory before claiming runtime benefit or promoting an
   artifact from experimental to valid.
8. Safety and lint failures produce typed failures or blocked gate status, not
   warning-only silent degradation.
9. Compiled activation is opt-in through `--compiled`; activation without that
   flag continues to use portable source projection.
10. `skill inspect` remains able to explain the compiled artifact, source
    digest, gate status, and fallback path.

## Artifact Layout

Derived artifacts live under registry state or another generated artifact
directory selected by implementation:

```text
state/compiled/skills/<skill>/<artifact-id>/
  manifest.json
  activation.md
  catalog.json
  boundaries.json
  tool-interface.json
  references.index.json
  source-digest.txt
```

The manifest must include at least:

```json
{
  "artifact_id": "compiled_fixflow_codex_...",
  "skill": "fixflow",
  "agent": "codex",
  "profile": "default",
  "source_ref": "HEAD",
  "source_tree_oid": "...",
  "source_digest": "...",
  "compiler_version": "loom-compiled-v1",
  "status": "planned",
	  "gates": {
	    "lint": "pass",
	    "safety": "pass",
	    "dependency": "pass",
	    "eval": "missing"
	  },
	  "content_hashes": {
	    "activation_md": "sha256:...",
	    "catalog_json": "sha256:...",
	    "boundaries_json": "sha256:...",
	    "tool_interface_json": "sha256:...",
	    "references_index_json": "sha256:..."
	  },
  "token_estimate": {
    "source_skill_md": 4200,
    "activation_md": 1200
  }
}
```

## User-Facing CLI

Required first-slice commands:

```bash
loom skill compile <skill> --dry-run [--agent <agent>] [--profile <profile>] [--json]
loom skill compile list <skill> [--json]
loom skill compile verify <skill> [--artifact <id>] [--json]
```

Deferred commands:

```bash
loom skill compile <skill> [--agent <agent>] [--profile <profile>]
loom skill activate <skill> --agent <agent> --compiled [--artifact <id>]
loom skill inspect <skill> --compiled [--artifact <id>]
loom skill compile clean <skill> [--artifact <id>]
```

Artifact ids are path segments, not paths. Commands accepting `--artifact`
must validate ids against a strict safe segment grammar before joining paths
under `state/compiled/skills/<skill>/`.

When `verify` is called without `--artifact`, it verifies every artifact under
the selected skill and returns a deterministic list sorted by artifact id. It
must not pick an arbitrary filesystem entry as the default target.

Deferred inspect integration should explain compiled artifact status, stale
reason, gate results, and source fallback once #366 wiring is included.

## Acceptance Criteria

1. `loom skill compile fixflow --dry-run --agent codex --json` returns planned
   artifact layout, digest inputs, gate status, and token estimates without
   writing files.
2. The planner reports no-op when the source skill is already below the
   configured compile threshold.
3. Artifact manifests include source ref, source tree OID or equivalent digest
   input, compiler version, agent, profile, status, gates including dependency
   readiness, generated content hashes, and token estimates.
4. `skill compile verify` detects missing files and malformed manifests.
5. `skill compile verify` detects stale artifacts after source edits.
6. Lint, safety, dependency, and eval gate failures prevent a `valid` artifact
   status.
7. Compiled activation remains opt-in and basic activation works when no
   compiled artifact exists.
8. Tests cover dry-run planning, no-op small skills, artifact manifest parsing,
   missing file verification, stale digest verification, manifest identity
   verification, generated-content hash/eval matching, gate failure handling,
   artifact-id path validation, sidecar path confinement, activation-artifact
   lint and safety verification, and all-artifact verification when no artifact
   id is provided.

## Open Questions

1. Whether artifact storage should be permanent registry state or generated
   cache state with rebuild semantics.
2. Whether token estimates should use a deterministic local estimator only or
   allow pluggable agent-specific estimators later.
3. Whether compiled activation should materialize an agent-compatible
   `SKILL.md` or pass `activation.md` directly to adapters that support it.
