# GH370 Product Spec: Safety Scan, Trust, Quarantine, And Security Diff

Issue: https://github.com/majiayu000/loom/issues/370
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

把单个 skill 的安全状态提升为一等 read/write lifecycle 对象，在 skill 被 activate、release、include in skillset 或推荐之前给出明确 trust/risk/quarantine decision。

Proposed commands:

```bash
loom skill scan <skill> [--mode install|activate|release] [--strict]
loom skill trust <skill> --level local-draft|reviewed|team-approved|third-party-unreviewed|blocked
loom skill quarantine <skill> [--reason <text>]
loom skill unquarantine <skill>
loom skill diff --security <skill> <from> <to>
```

Existing `skill policy` must feed the same decision model instead of diverging.

## Users

1. Local user: wants to know whether a skill is safe enough to activate.
2. Maintainer: needs auditable trust/quarantine state without mutating portable `SKILL.md`.
3. Agent: needs structured findings and activation gates before projecting a skill into an agent-visible target.

## Scope For First Implementation

1. Add registry-owned trust metadata for one skill.
2. Add `skill scan` that wraps and extends existing `skill policy` findings.
3. Add `skill trust`, `skill quarantine`, and `skill unquarantine` mutating commands with audit/commit behavior.
4. Enforce blocked/quarantined state in activation/projection paths.
5. Add `skill diff --security` to show security-relevant changes between refs.
6. Surface trust/quarantine state in inventory and future `skill inspect`.

## Non-Goals

1. 不把 heuristic scan 当成 malware verdict 或 sandbox guarantee。
2. 不把 trust state 写入 portable `SKILL.md`; trust is registry metadata.
3. 不实现 org RBAC/team approvals beyond local trust fields; #381 owns org policy.
4. 不执行 skill scripts during scan.
5. 不 auto-unquarantine after edits; human or explicit command must update state.
6. 不绕过 existing `skill policy` projection gate.

## Trust Levels

Allowed trust values:

```text
local-draft
reviewed
team-approved
third-party-unreviewed
blocked
quarantined
```

State should include:

```json
{
  "skill": "fixflow",
  "trust": "reviewed",
  "quarantined": false,
  "reason": null,
  "updated_at": "2026-07-01T00:00:00Z",
  "updated_by": "local-user"
}
```

## Scan Decision

Output shape:

```json
{
  "skill": "fixflow",
  "mode": "activate",
  "decision": "review_required",
  "trust": "third-party-unreviewed",
  "summary": {
    "critical": 0,
    "high": 1,
    "medium": 3,
    "low": 2
  },
  "findings": [
    {
      "id": "script_network_access",
      "severity": "high",
      "path": "scripts/install.sh",
      "message": "script invokes curl",
      "suggested_action": "review network destination and pin download checksum"
    }
  ],
  "activation_allowed": false
}
```

Decision values:

```text
allowed
review_required
blocked
quarantined
```

## Scan Checks

Instruction checks:

1. over-broad activation language;
2. prompt-injection-like language targeting higher-priority instructions;
3. hidden or obfuscated instructions in comments, HTML, or base64-like blocks;
4. secret exfiltration instructions;
5. approval/sandbox bypass instructions;
6. description over-trigger risk.

Script checks:

1. network-capable commands;
2. secret-reading patterns;
3. destructive commands;
4. shell injection hazards;
5. writes outside skill/workspace roots;
6. executable scripts without review notes.

Provenance/permission checks:

1. missing pinned ref for external source;
2. provenance digest drift;
3. uncommitted source drift before release;
4. imported source without license;
5. broad `allowed-tools`;
6. undeclared tool/MCP requirements.

## Activation Gating

1. `blocked` and `quarantined` skills cannot be activated or projected.
2. `third-party-unreviewed` with high/critical findings requires explicit trust update or future override.
3. Existing `skill project` / #367 `skill activate` must call the unified safety decision before writing target files.
4. The error details must include the scan/policy report and suggested actions.

## Security Diff

`loom skill diff --security <skill> <from> <to>` should report only security-relevant changes:

1. new/changed scripts;
2. new network/destructive/secret patterns;
3. frontmatter `description`, `allowed-tools`, `compatibility`, `metadata`;
4. references containing policy-bypass or exfiltration instructions;
5. provenance/trust-relevant manifest changes.

## Acceptance Criteria

1. `skill scan` returns structured findings with severity, path, message, and suggested action.
2. `skill trust` persists registry-owned trust metadata with audit/commit behavior.
3. `skill quarantine` marks a skill quarantined and deactivates/hides it without deleting source.
4. `skill unquarantine` clears quarantine but does not automatically elevate trust.
5. Activation/projection refuses blocked or quarantined skills.
6. Trust state is visible in inventory and future `skill inspect`.
7. `skill diff --security` compares two refs and highlights new risky patterns.
8. Tests cover harmless skill, network script, secret-reading script, prompt-injection-like instruction, blocked activation, quarantine/deactivate behavior, unquarantine behavior, and security diff.
