import { missingLabel, pick } from "../i18n";
import type { Locale } from "../i18n";
import type {
  PanelData,
  PendingOp,
  SkillView,
  V3Binding,
  V3Model,
  V3Payload,
  V3Projection,
  V3Target,
} from "../types";

export const EMPTY_COUNTS: V3Model["counts"] = {
  skills: 0,
  targets: 0,
  bindings: 0,
  active_bindings: 0,
  rules: 0,
  projections: 0,
  drifted_projections: 0,
  operations: 0,
};

export const EMPTY_PANEL_DATA: PanelData = {
  health: {},
  info: {},
  skills: [],
  remote: {},
  pending: {
    count: 0,
    ops: [],
    warnings: [],
  },
  v3: {
    available: false,
    counts: EMPTY_COUNTS,
    bindings: [],
    targets: [],
    rules: [],
    projections: [],
  },
  remoteWarnings: [],
  live: false,
  lastUpdated: "",
};

export function formatShortPath(value: string | undefined, locale: Locale) {
  if (!value) return missingLabel(locale);
  const parts = value.split("/");
  return parts.length <= 4 ? value : `.../${parts.slice(-3).join("/")}`;
}

export function formatSyncTone(state?: string) {
  switch ((state ?? "").toUpperCase()) {
    case "SYNCED":
    case "ACTIVE":
      return "is-success";
    case "DIVERGED":
    case "CONFLICTED":
      return "is-danger";
    case "PENDING_PUSH":
    case "LOCAL_ONLY":
      return "is-warning";
    default:
      return "is-primary";
  }
}

export function projectionIsDrifted(projection?: V3Projection | null) {
  if (!projection) return false;
  return Boolean(projection.observed_drift) || projection.health !== "healthy";
}

export function projectionToneClass(projection?: V3Projection | null) {
  if (!projection) return "is-muted";
  if (projectionIsDrifted(projection)) return "is-danger";
  if (projection.health === "warning") return "is-warning";
  return "is-success";
}

export function truncateText(value: string, limit = 40) {
  if (value.length <= limit) return value;
  return `${value.slice(0, limit - 1)}…`;
}

export function pendingSkillList(details: Record<string, unknown>) {
  const raw = details.skills;
  if (!Array.isArray(raw)) return [];
  return raw.filter((value): value is string => typeof value === "string");
}

export function formatDetailValue(key: string, value: unknown) {
  if (Array.isArray(value)) return `${value.length}`;
  if (value && typeof value === "object") return "{…}";
  const text = String(value ?? "—");
  if (key === "commit") return truncateText(text, 12);
  if (key.endsWith("_id") || key === "request_id") return truncateText(text, 18);
  return truncateText(text, 36);
}

export function summarizeDetails(details: Record<string, unknown>) {
  const skills = pendingSkillList(details);
  const summary = Object.entries(details)
    .slice(0, 3)
    .map(([key, value]) => {
      if (key === "skills" && skills.length > 0) {
        const preview = skills.slice(0, 2).join(", ");
        const overflow = skills.length > 2 ? ` +${skills.length - 2}` : "";
        return `skills[${skills.length}]: ${preview}${overflow}`;
      }
      if (Array.isArray(value)) return `${key}[${value.length}]`;
      if (value && typeof value === "object") return `${key}:{…}`;
      return `${key}:${formatDetailValue(key, value)}`;
    })
    .join(" · ");
  return summary || "no-details";
}

export function detailLabel(key: string, locale: Locale) {
  switch (key) {
    case "commit":
      return pick(locale, "Commit", "提交");
    case "skill":
      return pick(locale, "Skill", "技能");
    case "skills":
      return pick(locale, "Skills", "技能数");
    case "binding":
    case "binding_id":
      return pick(locale, "Binding", "绑定");
    case "target":
    case "target_id":
      return pick(locale, "Target", "目标");
    case "request_id":
      return pick(locale, "Request", "请求");
    default:
      return key.split("_").join(" ");
  }
}

export function pendingFactEntries(details: Record<string, unknown>) {
  const priority = ["commit", "skill", "binding", "binding_id", "target", "target_id", "request_id"];
  const facts = Object.entries(details)
    .filter(([, value]) => !Array.isArray(value) && (!value || typeof value !== "object"))
    .sort((left, right) => {
      const leftIndex = priority.indexOf(left[0]);
      const rightIndex = priority.indexOf(right[0]);
      return (leftIndex === -1 ? priority.length : leftIndex) - (rightIndex === -1 ? priority.length : rightIndex);
    })
    .slice(0, 4)
    .map(([key, value]) => ({
      key,
      value: formatDetailValue(key, value),
    }));

  const skills = pendingSkillList(details);
  if (skills.length > 0 && !facts.some((fact) => fact.key === "skills")) {
    facts.push({ key: "skills", value: String(skills.length) });
  }

  return facts.slice(0, 4);
}

export function explainPendingOp(op: PendingOp, locale: Locale) {
  const commit = typeof op.details.commit === "string" ? truncateText(op.details.commit, 12) : null;
  const skill = typeof op.details.skill === "string" ? op.details.skill : null;
  const skills = pendingSkillList(op.details);

  switch (op.command) {
    case "import":
      return pick(
        locale,
        `Imported ${skills.length} skills from commit ${commit ?? "unknown"} into the local workspace. The import is already done; this record is only waiting to sync out.`,
        `把提交 ${commit ?? "未知"} 里的 ${skills.length} 个技能导入到了本地。导入已经做完，这里只是还没同步出去的记录。`,
      );
    case "save":
      return pick(
        locale,
        `Saved the local skill ${skill ?? "unknown"} and recorded commit ${commit ?? "unknown"}. The save is already done; this record is only waiting to sync out.`,
        `把本地技能 ${skill ?? "未知"} 做了一次保存，并记录到了提交 ${commit ?? "未知"}。保存已经做完，这里只是还没同步出去的记录。`,
      );
    default:
      return pick(
        locale,
        `Recorded a local ${op.command} operation. The change already happened; this page is showing the unsynced record.`,
        `记录了一条本地 ${op.command} 操作。改动本身已经发生，这里显示的是尚未同步的记录。`,
      );
  }
}

export function queueStateLabel(index: number, remoteState: string | undefined, locale: Locale) {
  if (remoteState === "LOCAL_ONLY") {
    return pick(locale, "local only", "仅存在本地");
  }
  if (remoteState === "DIVERGED" || remoteState === "CONFLICTED") {
    return pick(locale, "sync blocked", "同步受阻");
  }
  return index === 0 ? pick(locale, "waiting to sync", "等待同步") : pick(locale, "queued", "队列中");
}

export function explainPendingQueue(data: PanelData, locale: Locale) {
  const count = data.pending.count;
  const syncState = data.remote.sync_state;

  if (count === 0) {
    return pick(
      locale,
      "This page shows local operations that are still waiting to sync. The queue is empty right now.",
      "这页显示的是本地操作的待同步记录。当前队列是空的。",
    );
  }

  if (syncState === "LOCAL_ONLY") {
    return pick(
      locale,
      `${count} local operations are parked here because the workspace has no usable remote sync target.`,
      `这里有 ${count} 条本地操作记录，因为当前工作区没有可用的远端同步目标，所以它们先停在本地。`,
    );
  }

  if (syncState === "DIVERGED" || syncState === "CONFLICTED") {
    return pick(
      locale,
      `${count} local operations are queued, but sync is blocked by a remote divergence or conflict.`,
      `这里有 ${count} 条本地操作记录，但因为远端分叉或冲突，同步目前被卡住了。`,
    );
  }

  return pick(
    locale,
    `${count} local operations are queued here until the next successful sync or push.`,
    `这里有 ${count} 条本地操作记录，等下次同步或推送成功后才会离开队列。`,
  );
}

export function pendingNextStep(data: PanelData, locale: Locale) {
  const syncState = data.remote.sync_state;

  if (syncState === "LOCAL_ONLY") {
    return pick(
      locale,
      "Configure a remote first. Until then, these records stay local and will not leave the queue.",
      "先配置远端仓库。否则这些记录只会留在本地，不会离开队列。",
    );
  }

  if (syncState === "DIVERGED" || syncState === "CONFLICTED") {
    return pick(
      locale,
      "Resolve the remote divergence or replay conflict, then sync again.",
      "先解决远端分叉或回放冲突，再重新同步。",
    );
  }

  return pick(
    locale,
    "Run the next sync or push. Once it succeeds, these records will be removed from the queue.",
    "执行下一次同步或推送。成功后，这些记录就会从队列里移除。",
  );
}

export function opToneClass(op: PanelData["pending"]["ops"][number] | null, remoteState?: string, index = 0) {
  if (!op) return "is-muted";
  if (index === 0 && (remoteState === "DIVERGED" || remoteState === "CONFLICTED")) return "is-danger";
  if (op.command.includes("release") || op.command.includes("snapshot")) return "is-success";
  if (op.command.includes("sync") || op.command.includes("project")) return "is-primary";
  return "is-warning";
}

export function scheduledOpIcon(index: number) {
  return index === 0 ? "timer" : "sync_problem";
}

export async function fetchRequiredJson<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(`GET ${path} failed with ${response.status}`);
  }
  return (await response.json()) as T;
}

export function normalizeV3(payload: V3Payload, locale: Locale): V3Model {
  if (!payload.ok || !payload.data) {
    return {
      available: false,
      counts: EMPTY_COUNTS,
      bindings: [],
      targets: [],
      rules: [],
      projections: [],
      error: payload.error?.message ?? pick(locale, "v3 state unavailable", "v3 状态不可用"),
    };
  }

  return {
    available: true,
    counts: {
      ...EMPTY_COUNTS,
      ...(payload.data.counts ?? {}),
    },
    bindings: payload.data.bindings ?? [],
    targets: payload.data.targets ?? [],
    rules: payload.data.rules ?? [],
    projections: payload.data.projections ?? [],
    checkpoint: payload.data.checkpoint,
  };
}

export async function loadPanelData(locale: Locale): Promise<PanelData> {
  const [health, info, skills, v3, remote, pending] = await Promise.all([
    fetchRequiredJson<PanelData["health"]>("/api/health"),
    fetchRequiredJson<PanelData["info"]>("/api/info"),
    fetchRequiredJson<{ skills?: string[] }>("/api/skills"),
    fetchRequiredJson<V3Payload>("/api/v3/status"),
    fetchRequiredJson<{ remote?: PanelData["remote"]; warnings?: string[] }>("/api/remote/status"),
    fetchRequiredJson<PanelData["pending"]>("/api/pending"),
  ]);

  return {
    health,
    info,
    skills: skills.skills ?? [],
    v3: normalizeV3(v3, locale),
    remote: remote.remote ?? {},
    pending: {
      count: pending.count ?? 0,
      ops: pending.ops ?? [],
      journal_events: pending.journal_events,
      history_events: pending.history_events,
      warnings: pending.warnings ?? [],
    },
    remoteWarnings: remote.warnings ?? [],
    live: true,
    lastUpdated: new Date().toISOString(),
  };
}

export function buildSkillViews(data: PanelData): SkillView[] {
  const bindingMap = new Map(data.v3.bindings.map((binding) => [binding.binding_id, binding]));
  const targetMap = new Map(data.v3.targets.map((target) => [target.target_id, target]));
  const allNames = new Set<string>([
    ...data.skills,
    ...data.v3.rules.map((rule) => rule.skill_id),
    ...data.v3.projections.map((projection) => projection.skill_id),
  ]);

  const result: SkillView[] = [];
  for (const name of allNames) {
    const rules = data.v3.rules.filter((rule) => rule.skill_id === name);
    const projections = data.v3.projections.filter((projection) => projection.skill_id === name);
    const bindings = new Map<string, V3Binding>();
    const targets = new Map<string, V3Target>();
    const methods = new Set<string>();

    for (const rule of rules) {
      methods.add(rule.method);
      const binding = bindingMap.get(rule.binding_id);
      const target = targetMap.get(rule.target_id);
      if (binding) bindings.set(binding.binding_id, binding);
      if (target) targets.set(target.target_id, target);
    }

    for (const projection of projections) {
      methods.add(projection.method);
      const binding = bindingMap.get(projection.binding_id);
      const target = targetMap.get(projection.target_id);
      if (binding) bindings.set(binding.binding_id, binding);
      if (target) targets.set(target.target_id, target);
    }

    result.push({
      name,
      rules,
      projections,
      bindings: [...bindings.values()],
      targets: [...targets.values()],
      methods: [...methods.values()],
      driftedCount: projections.filter(projectionIsDrifted).length,
    });
  }

  return result.sort((left, right) => left.name.localeCompare(right.name));
}
