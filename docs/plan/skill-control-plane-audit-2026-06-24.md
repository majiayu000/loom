# Skill Control Plane Audit: Correctness Roadmap

Date: 2026-06-24
Status: Active plan

## Decision

Loom's direction is sound: it should remain a Git-backed skill registry and
projection control plane, not a generic skill marketplace. The durable product
boundary is:

1. upstream providers find or fetch skills;
2. Loom records provenance and policy;
3. Loom plans, applies, audits, projects, evaluates, and rolls back local
   installations across agent targets.

## Current Findings

1. Remote `skill add` clones a Git URL, copies the clone into `skills/<name>`,
   and stages that directory. The copy helper rejects symlinks but did not
   explicitly exclude nested Git metadata. That can leak `.git` data into a
   registry skill and risks embedded-repository behavior.
2. V1 docs already require portable skills to use `SKILL.md` plus `name` and
   `description` frontmatter, but runtime validation still accepts legacy
   `skill.md` and parses descriptions by line scanning.
3. Panel/API have a skill read model, but CLI lacks first-class
   `skill list/show/search/resolve` commands.
4. Loom has Git-backed lifecycle primitives, release tags, and projection
   revisions, but does not yet have package-lock-style source provenance,
   resolved commits, artifact digests, or project lock entries.
5. Security docs are honest about current limits: Loom does not validate skill
   contents or verify upstream signatures. Existing guardrails remain valuable
   but are not a substitute for source locking, policy, review, sandboxing,
   and rollback.
6. `agent preflight` and command dry-runs are useful planning foundations, but
   there is no durable plan-id/apply protocol with idempotency.
7. Agent hosts are currently fixed built-ins. That is acceptable for V1, but an
   adapter protocol is the right long-term extension point.
8. Loom can prove projection health, not whether a skill is useful. Trigger,
   process, outcome, and efficiency evals should be separate product work.

## Issue Map

| Issue | Priority | Scope |
|---|---:|---|
| #338 | P0 | Prevent remote skill add from importing nested Git metadata. |
| #339 | P1 | Add strict Agent Skills lint and compatibility modes. |
| #340 | P1 | Add CLI skill inventory and discovery commands. |
| #341 | P1 | Add a human-friendly `loom use` flow. |
| #342 | P1 | Introduce source provenance and `loom.lock`. |
| #343 | P1 | Add skill trust, capability, and policy checks. |
| #344 | P1 | Add durable agent plan/apply with idempotency. |
| #345 | P2 | Define configurable agent adapter protocol. |
| #346 | P2 | Add skill eval matrix. |
| #347 | P2 | Define GitHub and `gh skill` provider boundary. |

## P0 Acceptance

The first implementation slice is intentionally narrow:

1. Recursive skill-source copies skip nested `.git` metadata.
2. Normal non-VCS dotfiles remain importable.
3. Symlink rejection remains unchanged.
4. Tests prove skipped Git metadata cannot be staged as a skill payload.
5. The follow-up provider design can move toward `git archive` or immutable
   content-addressed staging after this safety fix is merged.
