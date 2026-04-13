import type { PendingOp, SkillView, V3Projection, V3Target } from "../types";
import { projectionIsDrifted } from "./panel_data";

export type ProjectionMethodFilter = "all" | "symlink" | "copy" | "materialize";
export type ProjectionHealthFilter = "all" | "healthy" | "drifted" | "warning";

export function filterSkillViews(skillViews: SkillView[], query: string) {
  const value = query.trim().toLowerCase();
  if (!value) return skillViews;
  return skillViews.filter((skill) => {
    const nameMatch = skill.name.toLowerCase().includes(value);
    const bindingMatch = skill.bindings.some((binding) =>
      `${binding.binding_id} ${binding.profile_id}`.toLowerCase().includes(value),
    );
    const targetMatch = skill.targets.some((target) => target.target_id.toLowerCase().includes(value));
    return nameMatch || bindingMatch || targetMatch;
  });
}

export function filterPendingOps(ops: PendingOp[], query: string) {
  const value = query.trim().toLowerCase();
  if (!value) return ops;
  return ops.filter((op) => {
    const haystacks = [
      op.op_id,
      op.request_id,
      op.command,
      ...Object.entries(op.details).flatMap(([key, detail]) => [key, String(detail ?? "")]),
    ];
    return haystacks.some((entry) => entry?.toLowerCase().includes(value));
  });
}

export function filterOverviewOps(ops: PendingOp[], query: string) {
  return filterPendingOps(ops, query).slice(0, 4);
}

export function filterOverviewWarnings(warnings: string[], query: string) {
  const value = query.trim().toLowerCase();
  if (!value) return warnings;
  return warnings.filter((warning) => warning.toLowerCase().includes(value));
}

export function buildProjectionMatrix(projections: V3Projection[]) {
  const matrix = new Map<string, V3Projection[]>();
  for (const projection of projections) {
    const key = `${projection.skill_id}::${projection.target_id}`;
    const current = matrix.get(key) ?? [];
    current.push(projection);
    matrix.set(key, current);
  }
  return matrix;
}

export function filterProjectionTargets(targets: V3Target[], scope: string) {
  const value = scope.trim();
  if (!value || value === "all") return targets;
  return targets.filter((target) => {
    const haystack = `${target.target_id} ${target.path} ${target.ownership} ${target.agent}`.toLowerCase();
    return haystack.includes(value.toLowerCase());
  });
}

export function filterProjectionViews(
  skillViews: SkillView[],
  projections: V3Projection[],
  targets: V3Target[],
  filters: {
    method: ProjectionMethodFilter;
    health: ProjectionHealthFilter;
    scope: string;
  },
) {
  const allowedTargets = new Set(filterProjectionTargets(targets, filters.scope).map((target) => target.target_id));

  const filteredProjections = projections.filter((projection) => {
    if (allowedTargets.size > 0 && !allowedTargets.has(projection.target_id)) {
      return false;
    }
    if (filters.method !== "all" && projection.method !== filters.method) {
      return false;
    }
    if (filters.health === "drifted") {
      return projectionIsDrifted(projection);
    }
    if (filters.health === "warning") {
      return projection.health === "warning" && !projectionIsDrifted(projection);
    }
    if (filters.health === "healthy") {
      return !projectionIsDrifted(projection) && projection.health === "healthy";
    }
    return true;
  });

  const visibleSkills = skillViews.filter((skill) =>
    filteredProjections.some((projection) => projection.skill_id === skill.name),
  );
  const visibleTargets = targets.filter((target) => allowedTargets.size === 0 || allowedTargets.has(target.target_id));

  return {
    projections: filteredProjections,
    skills: visibleSkills,
    targets: visibleTargets,
  };
}
