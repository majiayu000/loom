import type { RegistryBinding } from "../../generated/RegistryBinding";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { RegistryRule } from "../../generated/RegistryRule";
import type { RegistryTarget } from "../../generated/RegistryTarget";
import type { PendingOp } from "../../types";
import { normalizeAgentSlug } from "../agent_options";
import type { AgentSlug, Binding, Op, Ownership, ProjectionMethod, Skill, Target } from "../types";

/**
 * Return the backend slug verbatim (lowercased + trimmed). Unknown slugs
 * are preserved so the UI renders them with their real identity instead
 * of being silently relabelled as Claude (cf. Codex P2 on PR #7).
 */
function toAgentSlug(value: string): AgentSlug {
  return normalizeAgentSlug(value) as AgentSlug;
}

function toOwnership(value: string): Ownership {
  if (value === "managed" || value === "observed" || value === "external") return value;
  return "external";
}

function toMethod(value: string): ProjectionMethod {
  if (value === "symlink" || value === "copy" || value === "materialize") return value;
  return "symlink";
}

function profileFromPath(path: string): string {
  if (path.includes(".claude-work")) return "work";
  if (path.includes("/repo/") || path.startsWith("/repo")) return "repo";
  return "home";
}

function shortPath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

/**
 * Build a snapshot-wide projection index so adapters avoid O(projections × targets)
 * `Array.find` sweeps every poll cycle (cf. PR #7 review L1).
 */
export interface AdapterIndex {
  targetsById: Map<string, RegistryTarget>;
  projectionsBySkill: Map<string, RegistryProjection[]>;
  projectionsByTarget: Map<string, RegistryProjection[]>;
}

export function buildAdapterIndex(
  targets: RegistryTarget[],
  projections: RegistryProjection[],
): AdapterIndex {
  const targetsById = new Map<string, RegistryTarget>();
  for (const t of targets) targetsById.set(t.target_id, t);

  const projectionsBySkill = new Map<string, RegistryProjection[]>();
  const projectionsByTarget = new Map<string, RegistryProjection[]>();
  for (const p of projections) {
    const sArr = projectionsBySkill.get(p.skill_id);
    if (sArr) sArr.push(p);
    else projectionsBySkill.set(p.skill_id, [p]);
    const tArr = projectionsByTarget.get(p.target_id);
    if (tArr) tArr.push(p);
    else projectionsByTarget.set(p.target_id, [p]);
  }
  return { targetsById, projectionsBySkill, projectionsByTarget };
}

export function adaptTarget(t: RegistryTarget, index: AdapterIndex): Target {
  const projections = index.projectionsByTarget.get(t.target_id) ?? [];
  const skillsOnTarget = new Set(projections.map((p) => p.skill_id));
  return {
    id: t.target_id,
    agent: toAgentSlug(t.agent),
    profile: profileFromPath(t.path),
    path: shortPath(t.path),
    ownership: toOwnership(t.ownership),
    skills: skillsOnTarget.size,
    lastSync: t.created_at ? relativeTime(t.created_at) : "—",
  };
}

/**
 * Pick the projection with the newest `updated_at` — NOT by lex-comparing
 * commit hashes (cf. Codex P1 on PR #7; git hashes are not time-ordered).
 * Falls back to `created_at` on the target if the skill has no projections.
 */
export function adaptSkill(
  name: string,
  index: AdapterIndex,
  rules: RegistryRule[],
): Skill {
  const projForSkill = index.projectionsBySkill.get(name) ?? [];
  const targetIds = Array.from(new Set(projForSkill.map((p) => p.target_id)));
  const ruleCount = rules.filter((r) => r.skill_id === name).length;

  const newest = projForSkill.reduce<RegistryProjection | undefined>((acc, p) => {
    if (!p.updated_at) return acc;
    if (!acc || !acc.updated_at) return p;
    return p.updated_at > acc.updated_at ? p : acc;
  }, undefined);

  const latestRev = newest?.last_applied_rev ? newest.last_applied_rev.slice(0, 8) : "—";
  const changed = newest?.updated_at ? relativeTime(newest.updated_at) : "—";

  return {
    id: `s-${name}`,
    name,
    tag: inferTag(name),
    latestRev,
    ruleCount,
    changed,
    targets: targetIds,
  };
}

function inferTag(name: string): string {
  if (name.startsWith("rust-") || name.includes("rust")) return "rust";
  if (name.includes("commit") || name.includes("git")) return "git";
  if (name.includes("typescript") || name.includes("typed-api")) return "typescript";
  if (name.includes("sql") || name.includes("schema")) return "database";
  if (name.includes("onboard") || name.includes("doc")) return "docs";
  return "skill";
}

export function adaptBinding(b: RegistryBinding, rules: RegistryRule[]): Binding {
  const rule = rules.find((r) => r.binding_id === b.binding_id);
  return {
    id: b.binding_id,
    skill: rule?.skill_id ?? "—",
    target: b.default_target_id,
    matcher: `${b.workspace_matcher.kind}:${b.workspace_matcher.value}`,
    method: rule ? toMethod(rule.method) : "symlink",
    policy: b.policy_profile === "manual" ? "manual" : "auto",
  };
}

export function adaptPendingOp(op: PendingOp, index: number): Op {
  const details = op.details ?? {};
  const skillList = Array.isArray(details.skills)
    ? (details.skills as unknown[]).filter((s): s is string => typeof s === "string")
    : [];
  const targetField = typeof details.target === "string" ? (details.target as string) : "—";
  const methodField = typeof details.method === "string" ? toMethod(details.method as string) : "—";
  return {
    id: op.op_id ?? op.request_id ?? `op-${index}`,
    status: "pending",
    kind: op.command,
    skill:
      skillList.length > 0
        ? skillList.join(", ")
        : typeof details.skill === "string"
        ? (details.skill as string)
        : op.command,
    target: targetField,
    method: methodField,
    time: op.created_at ? relativeTime(op.created_at) : "queued",
  };
}

export function adaptProjectionOp(p: RegistryProjection, index: AdapterIndex): Op {
  const t = index.targetsById.get(p.target_id);
  const drifted = Boolean(p.observed_drift) || p.health !== "healthy";
  const status: Op["status"] = drifted ? "err" : "ok";
  return {
    id: p.instance_id,
    status,
    kind: "project",
    skill: `${p.skill_id}@${(p.last_applied_rev ?? "").slice(0, 7) || "—"}`,
    target: t ? `${toAgentSlug(t.agent)}/${profileFromPath(t.path)}` : p.target_id,
    method: toMethod(p.method),
    time: p.updated_at ? relativeTime(p.updated_at) : "—",
    reason: drifted ? `health=${p.health}${p.observed_drift ? "; drift observed" : ""}` : undefined,
  };
}

function relativeTime(iso: string): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const ms = Date.now() - then;
  if (ms < 0) return "now";
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  return `${day}d ago`;
}
