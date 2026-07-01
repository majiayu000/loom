# GH372 Product Spec: Skill Improve And Regression Workflow

Issue: https://github.com/majiayu000/loom/issues/372
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

Add a guided single-skill improvement loop that validates local edits before save/release:

```text
edit -> lint -> scan -> deps -> eval -> compare -> save/release
```

Proposed commands:

```bash
loom skill improve <skill> [--agent <agent>] [--workspace <path>] [--baseline <ref>] [--real-eval] [--dry-run]
loom skill regression <skill> [--agent <agent>] [--from <ref>] [--to working-tree]
loom skill save <skill> --preflight [--message <msg>]
loom skill release <skill> <version> --preflight --baseline <previous-release-ref|ref>
```

## Users

1. Skill author: wants a single preflight report before saving local edits.
2. Maintainer: wants regression gates before release.
3. Agent: needs a deterministic report and typed failure before mutating Git state.

## Scope For First Implementation

1. `skill improve` returns a consolidated read-only preflight report.
2. `skill regression` compares a baseline ref to target ref/working tree.
3. `skill save --preflight` runs the same gates before commit.
4. `skill release --preflight` runs the same gates before tag/release operation.
5. Reports can feed future `skill inspect` quality/safety sections.

## Non-Goals

1. No LLM-driven editing or automatic patch generation.
2. No hidden mutation in `improve` or `regression`.
3. No network/real-agent eval by default.
4. No release if source state is dirty beyond the selected skill.
5. No override unless a later policy explicitly defines one.

## Checks

`skill improve` should run:

1. source drift detection;
2. portable lint;
3. agent-specific lint when `--agent` is set;
4. safety scan (#370);
5. dependency readiness (#371);
6. offline eval fixtures;
7. real eval compare only when #369 runner is available and `--real-eval` is
   explicitly selected;
8. security diff against baseline when available.

## Output Contract

```json
{
  "skill": "fixflow",
  "baseline": "HEAD",
  "target": "working-tree",
  "checks": {
    "lint": "pass",
    "safety": "warning",
    "dependencies": "pass",
    "offline_eval": "pass",
    "real_eval": "skipped"
  },
  "regressions": [],
  "recommendation": {
    "action": "save",
    "command": "loom skill save fixflow --preflight --message 'improve fixflow workflow'"
  }
}
```

## Regression Gate

`skill regression` fails when:

1. portable lint regresses from pass to fail;
2. high/critical safety findings are newly introduced;
3. eval pass rate drops beyond threshold;
4. trigger false positives/false negatives worsen beyond threshold;
5. `SKILL.md` exceeds configured size threshold without moving content into references;
6. dependency readiness regresses from ready to not ready.

Thresholds can be constants in v1 and configurable later.

## Save/Release Integration

`skill save --preflight`:

1. runs preflight;
2. blocks commit if gates fail;
3. includes full report in error details;
4. commits only when gates pass.

`skill release --preflight`:

1. requires clean selected skill state after save;
2. requires an explicit release baseline such as the previous release tag or an
   implementation-provided previous-release ref;
3. runs the same gates against that baseline instead of comparing the release
   candidate to itself;
4. blocks tag/release if gates fail;
5. includes full report in error details.

## Acceptance Criteria

1. `skill improve` produces a consolidated preflight report without mutation.
2. `skill regression` compares working tree or target ref to a baseline ref.
3. `skill save --preflight` blocks on lint/eval/security/dependency regressions.
4. `skill release --preflight` blocks on the same gates and requires clean
   source state plus a release baseline that is not the candidate ref itself.
5. Reports are reusable by `skill inspect` quality/safety sections.
6. Tests cover no drift, safe drift, lint regression, safety regression, dependency regression, eval regression, save preflight pass/fail, and release preflight pass/fail.
