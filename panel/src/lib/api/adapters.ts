import type { V3Binding } from "../../generated/V3Binding";
import type { V3Projection } from "../../generated/V3Projection";
import type { V3Rule } from "../../generated/V3Rule";
import type { V3Target } from "../../generated/V3Target";
import type { PendingOp } from "../../types";
import type { AgentKind, Binding, Op, Ownership, ProjectionMethod, Skill, Target } from "../types";

const KNOWN_AGENTS: AgentKind[] = [
  "claude",
  "codex",
  "cursor",
  "windsurf",
  "cline",
  "copilot",
  "aider",
  "opencode",
  "gemini",
  "goose",
];

function toAgent(value: string): AgentKind {
  const normalized = value.toLowerCase() as AgentKind;
  return KNOWN_AGENTS.includes(normalized) ? normalized : "claude";
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

export function adaptTarget(t: V3Target, projections: V3Projection[]): Target {
  const skillsOnTarget = new Set(projections.filter((p) => p.target_id === t.target_id).map((p) => p.skill_id));
  return {
    id: t.target_id,
    agent: toAgent(t.agent),
    profile: profileFromPath(t.path),
    path: shortPath(t.path),
    ownership: toOwnership(t.ownership),
    skills: skillsOnTarget.size,
    lastSync: t.created_at ? relativeTime(t.created_at) : "—",
  };
}

export function adaptSkill(name: string, projections: V3Projection[], rules: V3Rule[]): Skill {
  const targetIds = Array.from(new Set(projections.filter((p) => p.skill_id === name).map((p) => p.target_id)));
  const ruleCount = rules.filter((r) => r.skill_id === name).length;
  const projForSkill = projections.filter((p) => p.skill_id === name);
  const latestRev = projForSkill.reduce<string | undefined>(
    (acc, p) => (p.last_applied_rev && (!acc || p.last_applied_rev > acc) ? p.last_applied_rev : acc),
    undefined,
  );
  const latestUpdate = projForSkill.reduce<string | undefined>(
    (acc, p) => (p.updated_at && (!acc || p.updated_at > acc) ? p.updated_at : acc),
    undefined,
  );
  return {
    id: `s-${name}`,
    name,
    tag: inferTag(name),
    version: latestRev ? latestRev.slice(0, 8) : "—",
    captures: ruleCount,
    released: latestRev ? latestRev.slice(0, 8) : "—",
    changed: latestUpdate ? relativeTime(latestUpdate) : "—",
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

export function adaptBinding(b: V3Binding, rules: V3Rule[]): Binding {
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

export function adaptProjectionOp(p: V3Projection, t: V3Target | undefined): Op {
  const drifted = Boolean(p.observed_drift) || p.health !== "healthy";
  const status: Op["status"] = drifted ? "err" : "ok";
  return {
    id: p.instance_id,
    status,
    kind: "project",
    skill: `${p.skill_id}@${(p.last_applied_rev ?? "").slice(0, 7) || "—"}`,
    target: t ? `${t.agent}/${profileFromPath(t.path)}` : p.target_id,
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
