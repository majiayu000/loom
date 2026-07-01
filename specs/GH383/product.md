# GH383 Product Spec: Assisted Skill Authoring

Issue: https://github.com/majiayu000/loom/issues/383
Parent: https://github.com/majiayu000/loom/issues/376
Status: Blocked design packet
Locale: zh-CN

## Goal

Add guarded LLM-assisted authoring and refactoring commands that generate
reviewable patch artifacts and eval cases for one skill.

LLM output must produce drafts and patches. It must not silently mutate source
files, activate skills, release skills, or upload private context without
explicit opt-in.

## Blocking Dependencies

Production implementation is blocked by:

- #364 generated skeleton quality.
- #365 portable/agent/quality lint.
- #366 single-skill inspect/status.
- #369 eval evidence.
- #370 safety/trust/quarantine.
- #372 edit/improve/regression workflow.

## User-Facing Commands

Target command surface:

```bash
loom skill draft <name> --from-session <path|id> [--agent <agent>] [--dry-run]
loom skill extract <name> --from-diff <git-range> [--dry-run]
loom skill rewrite <skill> --goal "improve trigger precision" [--dry-run]
loom skill tune-description <skill> --agent <agent> [--cases evals/triggers.jsonl] [--dry-run]
loom skill generate-evals <skill> [--count <n>] [--from-history] [--dry-run]
loom skill apply-patch <patch-id> --idempotency-key <key>
```

Generation commands default to dry-run/patch output.

## Use Cases

### Draft From Repeated Workflow

Input: transcript, notes, or command history.

Output: proposed `SKILL.md`, references, scripts, and eval cases.

### Tune Description

Input: trigger eval false positives/false negatives.

Output: frontmatter description patch and new positive/negative trigger cases.

### Refactor For Progressive Disclosure

Input: oversized `SKILL.md`.

Output: move background sections to `references/`, add concise pointers.

### Generate Evals

Input: skill body and references.

Output: positive/negative trigger cases and task fixture stubs.

## Patch Artifact

LLM-assisted commands should produce a patch artifact:

```json
{
  "patch_id": "skillpatch_...",
  "skill": "fixflow",
  "goal": "improve trigger precision",
  "source_ref": "HEAD",
  "files": [
    {"path": "SKILL.md", "diff": "@@ ..."},
    {"path": "evals/triggers.jsonl", "diff": "@@ ..."}
  ],
  "validation_plan": [
    "loom skill lint fixflow --portable --quality",
    "loom skill eval fixflow --agent codex"
  ],
  "risk_notes": []
}
```

## Guardrails

1. Commands produce patch artifacts by default and do not mutate source.
2. `apply-patch` requires idempotency key and revalidates source ref.
3. Prompt material is redacted; secrets and raw private session content are not
   included unless explicitly selected.
4. Generated scripts are review-required and non-executable unless explicitly
   approved.
5. Lint, scan, and eval preflight run before and after patch apply.
6. Safety scan blocks high-risk unreviewed changes.
7. Generated skills are never automatically activated or released.

## Non-Goals

1. No automatic activation of LLM-generated skills.
2. No unreviewed release.
3. No dependency on one hosted model provider in core.
4. No hidden prompt/context uploads without explicit user opt-in.
5. No acceptance of unvalidated patches.

## Acceptance Criteria

1. Commands produce patch artifacts by default and do not mutate source.
2. `apply-patch` requires idempotency key and revalidates source ref.
3. Generated patches include validation commands and risk notes.
4. Safety scan runs after patch application and blocks high-risk unreviewed
   changes.
5. Description tuning can add positive/negative trigger cases.
6. Progressive disclosure refactor moves long content into references with
   relative links.
7. Tests use a mock LLM provider and cover draft, rewrite, tune-description,
   generate-evals, patch apply, source-ref mismatch, and safety regression.
