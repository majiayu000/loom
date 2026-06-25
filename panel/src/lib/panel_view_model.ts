import type { PanelDataMode, PanelLiveData } from "./api/usePanelData";
import { formatActionNeededBadge, formatQueuedWrites, summarizeOps } from "./count_labels";
import type { Binding, Op, PanelPageKey, ProjectionLink, ProjectionMethod, Skill, Target } from "./types";
import type { RegistryProjection } from "../generated/RegistryProjection";

export type FieldState = "available" | "unavailable";
export type StatusTone = "ok" | "warn" | "err" | "pending" | "muted";

export interface FieldViewModel {
  state: FieldState;
  label: string;
  raw?: string | number | boolean;
  title?: string;
}

export interface CountViewModel {
  key: string;
  label: string;
  value: number | null;
  display: string;
  state: FieldState;
  title?: string;
}

export interface PageViewModel {
  key: PanelPageKey;
  label: string;
  group: "build" | "operate";
  count?: number | null;
  countLabel?: string;
  countTitle?: string;
}

export interface SkillViewModel {
  id: string;
  name: FieldViewModel;
  description: FieldViewModel;
  sourceStatus: FieldViewModel;
  latestRev: FieldViewModel;
  changed: FieldViewModel;
  bindings: CountViewModel;
  projections: CountViewModel;
  targets: CountViewModel;
}

export interface TargetViewModel {
  id: string;
  agent: FieldViewModel;
  agentSource: FieldViewModel;
  profile: FieldViewModel;
  path: FieldViewModel;
  ownership: FieldViewModel;
  observedSkills: CountViewModel;
  projectedSkills: CountViewModel;
}

export interface BindingViewModel {
  id: string;
  skill: FieldViewModel;
  target: FieldViewModel;
  matcher: FieldViewModel;
  method: FieldViewModel;
  policy: FieldViewModel;
}

export interface ProjectionViewModel {
  id: FieldViewModel;
  skill: FieldViewModel;
  target: FieldViewModel;
  binding: FieldViewModel;
  method: FieldViewModel;
  health: FieldViewModel;
  materializedPath: FieldViewModel;
  lastAppliedRev: FieldViewModel;
  updatedAt: FieldViewModel;
  drifted: boolean;
}

export interface OperationViewModel {
  id: FieldViewModel;
  status: FieldViewModel;
  kind: FieldViewModel;
  skill: FieldViewModel;
  target: FieldViewModel;
  method: FieldViewModel;
  time: FieldViewModel;
  reason: FieldViewModel;
}

export interface ActionViewModel {
  key:
    | "addSkill"
    | "addTarget"
    | "addBinding"
    | "projectSkill"
    | "captureSkill"
    | "cleanOrphans"
    | "replayQueued"
    | "repairHistory"
    | "syncPull"
    | "syncPush";
  label: string;
  mutation: true;
  enabled: boolean;
  disabledReason?: string;
}

export interface ShellStatusViewModel {
  label: string;
  title: string;
  tone: StatusTone;
}

export interface ShellViewModel {
  status: ShellStatusViewModel;
  pages: PageViewModel[];
  counts: {
    skills: CountViewModel;
    targets: CountViewModel;
    bindings: CountViewModel;
    projections: CountViewModel;
    operations: CountViewModel;
    actionNeeded: CountViewModel;
    queuedWrites: CountViewModel;
    backend: {
      skills: CountViewModel;
      targets: CountViewModel;
      bindings: CountViewModel;
      projections: CountViewModel;
      operations: CountViewModel;
    };
  };
  registryRoot: FieldViewModel;
  remoteState: FieldViewModel;
  readOnly: boolean;
  readOnlyReason?: string;
}

export interface PanelViewModel {
  shell: ShellViewModel;
  skills: SkillViewModel[];
  targets: TargetViewModel[];
  bindings: BindingViewModel[];
  projections: ProjectionViewModel[];
  operations: OperationViewModel[];
  actions: Record<ActionViewModel["key"], ActionViewModel>;
  graphLinks: ProjectionLink[];
}

interface PanelViewModelOptions {
  page: PanelPageKey;
  readOnly: boolean;
  historyReadOnly?: boolean;
}

const UNAVAILABLE = "unavailable";
const UNKNOWN = "unknown";
const MISSING_SENTINEL = "\u2014";
const KNOWN_METHODS = new Set(["symlink", "copy", "materialize"]);
const KNOWN_OWNERSHIP = new Set(["managed", "observed", "external"]);
const KNOWN_SOURCE_STATUS = new Set(["present", "missing", "non-compliant"]);
const KNOWN_OPERATION_STATUS = new Set(["ok", "pending", "err"]);
const KNOWN_POLICIES = new Set(["auto", "manual"]);
const READ_ONLY_REASON = "registry offline";

const PAGE_LABELS: Record<PanelPageKey, string> = {
  overview: "Overview",
  skills: "Skills",
  targets: "Targets",
  bindings: "Bindings",
  projections: "Projections",
  ops: "Activity",
  history: "Audit log",
  sync: "Git sync",
  doctor: "Doctor",
  settings: "Settings",
};

const PAGE_GROUPS: Record<PanelPageKey, PageViewModel["group"]> = {
  overview: "build",
  skills: "build",
  targets: "build",
  bindings: "build",
  projections: "build",
  ops: "build",
  history: "operate",
  sync: "operate",
  doctor: "operate",
  settings: "operate",
};

const PAGE_ORDER: PanelPageKey[] = [
  "overview",
  "skills",
  "targets",
  "bindings",
  "projections",
  "ops",
  "history",
  "sync",
  "doctor",
  "settings",
];

function unavailable(reason: string): FieldViewModel {
  return { state: "unavailable", label: UNAVAILABLE, title: reason };
}

function textField(value: unknown, reason: string): FieldViewModel {
  if (value === null || value === undefined || value === "" || value === MISSING_SENTINEL) return unavailable(reason);
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return { state: "available", label: String(value), raw: value };
  }
  return { state: "available", label: String(value), raw: String(value) };
}

function enumField(value: unknown, known: Set<string>, reason: string): FieldViewModel {
  const field = textField(value, reason);
  if (field.state === "unavailable") return field;
  const raw = String(field.raw ?? field.label);
  if (known.has(raw)) return field;
  return {
    state: "available",
    label: UNKNOWN,
    raw,
    title: `Backend returned an unrecognized value: ${raw}`,
  };
}

function methodField(value: unknown, reason = "backend did not include a projection method"): FieldViewModel {
  return enumField(value, KNOWN_METHODS, reason);
}

function countField(key: string, label: string, value: unknown, reason: string): CountViewModel {
  if (typeof value !== "number" || Number.isNaN(value)) {
    return { key, label, value: null, display: UNAVAILABLE, state: "unavailable", title: reason };
  }
  return { key, label, value, display: String(value), state: "available" };
}

function arrayCount(key: string, label: string, value: number): CountViewModel {
  return { key, label, value, display: String(value), state: "available" };
}

function statusForLiveData(live: PanelLiveData): ShellStatusViewModel {
  if (live.mode === "offline-stale") {
    return {
      label: "live API offline / stale snapshot",
      title: "Last-known registry snapshot. The live API is unreachable.",
      tone: "err",
    };
  }
  if (live.error) return { label: "registry error", title: live.error, tone: "err" };
  if (live.loading) return { label: "connecting", title: "Connecting to the registry API", tone: "pending" };
  if (!live.live) return { label: "registry offline", title: "Registry API is offline", tone: "err" };
  if (live.mode === "first-run") {
    return {
      label: "registry setup required",
      title: "Registry state has not been initialized yet.",
      tone: "warn",
    };
  }

  const state = (live.remote?.sync_state ?? "").toUpperCase();
  if (state === "DIVERGED" || state === "CONFLICTED") {
    return { label: `remote ${state.toLowerCase()}`, title: "Remote and local history differ.", tone: "err" };
  }
  if (state === "PENDING_PUSH" || state === "LOCAL_ONLY" || live.queuedWriteCount > 0) {
    return {
      label: live.queuedWriteCount > 0 ? formatQueuedWrites(live.queuedWriteCount) : state.toLowerCase().replace("_", " "),
      title: `${formatQueuedWrites(live.queuedWriteCount)} waiting in the pending queue.`,
      tone: "warn",
    };
  }
  return { label: "registry clean", title: "Registry is in sync with the Git remote.", tone: "ok" };
}

function modeReadOnlyReason(mode: PanelDataMode): string {
  if (mode === "offline-stale") return "live API offline";
  if (mode === "offline-empty") return "registry offline";
  if (mode === "first-run") return "registry setup required";
  return READ_ONLY_REASON;
}

function mutationAction(
  key: ActionViewModel["key"],
  label: string,
  readOnly: boolean,
  enabledWhen = true,
  disabledReason = "action unavailable",
  readOnlyReason = READ_ONLY_REASON,
): ActionViewModel {
  if (readOnly) return { key, label, mutation: true, enabled: false, disabledReason: readOnlyReason };
  if (!enabledWhen) return { key, label, mutation: true, enabled: false, disabledReason };
  return { key, label, mutation: true, enabled: true };
}

export function selectProjectionLinks(projections: readonly RegistryProjection[]): ProjectionLink[] {
  return projections.map((projection) => {
    const method = methodField(projection.method);
    const methodValue =
      method.state === "available" && (method.label === "symlink" || method.label === "copy" || method.label === "materialize")
        ? method.label
        : "unknown";
    return {
      skillId: `s-${projection.skill_id}`,
      targetId: projection.target_id,
      method: methodValue,
    };
  });
}

export function selectPanelViewModel(live: PanelLiveData, options: PanelViewModelOptions): PanelViewModel {
  const opCounts = summarizeOps(live.ops);
  const backendCounts = live.counts;
  const actionNeededCount = opCounts.actionNeeded;
  const readOnlyReason = options.readOnly ? modeReadOnlyReason(live.mode) : undefined;
  const actionReadOnlyReason = readOnlyReason ?? READ_ONLY_REASON;

  const shellCounts = {
    skills: arrayCount("skills", "Skills", live.skills.length),
    targets: arrayCount("targets", "Targets", live.targets.length),
    bindings: arrayCount("bindings", "Bindings", live.bindings.length),
    projections: arrayCount("projections", "Projections", live.projections.length),
    operations: arrayCount("operations", "Operations", live.ops.length),
    actionNeeded: arrayCount("actionNeeded", "Action needed", actionNeededCount),
    queuedWrites: arrayCount("queuedWrites", "Queued writes", live.queuedWriteCount),
    backend: {
      skills: countField("backendSkills", "Backend skills", backendCounts.skills, "backend counts.skills is unavailable"),
      targets: countField("backendTargets", "Backend targets", backendCounts.targets, "backend counts.targets is unavailable"),
      bindings: countField("backendBindings", "Backend bindings", backendCounts.bindings, "backend counts.bindings is unavailable"),
      projections: countField(
        "backendProjections",
        "Backend projections",
        backendCounts.projections,
        "backend counts.projections is unavailable",
      ),
      operations: countField(
        "backendOperations",
        "Backend operations",
        backendCounts.operations,
        "backend counts.operations is unavailable",
      ),
    },
  };

  const pages = PAGE_ORDER.map((key): PageViewModel => {
    const countByPage: Partial<Record<PanelPageKey, CountViewModel>> = {
      skills: shellCounts.skills,
      targets: shellCounts.targets,
      bindings: shellCounts.bindings,
      projections: shellCounts.projections,
      ops: shellCounts.actionNeeded,
    };
    const count = countByPage[key];
    return {
      key,
      label: PAGE_LABELS[key],
      group: PAGE_GROUPS[key],
      count: count?.value,
      countLabel: key === "ops" && actionNeededCount > 0 ? formatActionNeededBadge(actionNeededCount) : count?.display,
      countTitle: key === "ops" ? "Replayable or failed activity rows" : count?.label,
    };
  });

  const actions: Record<ActionViewModel["key"], ActionViewModel> = {
    addSkill: mutationAction("addSkill", "Add skill", options.readOnly, true, "action unavailable", actionReadOnlyReason),
    addTarget: mutationAction("addTarget", "Add target", options.readOnly, true, "action unavailable", actionReadOnlyReason),
    addBinding: mutationAction(
      "addBinding",
      "Add binding",
      options.readOnly,
      live.targets.length > 0,
      "add a target first",
      actionReadOnlyReason,
    ),
    projectSkill: mutationAction(
      "projectSkill",
      "Project skill",
      options.readOnly,
      live.bindings.length > 0,
      "add a binding first",
      actionReadOnlyReason,
    ),
    captureSkill: mutationAction(
      "captureSkill",
      "Capture skill",
      options.readOnly,
      live.bindings.length > 0,
      "add a binding first",
      actionReadOnlyReason,
    ),
    cleanOrphans: mutationAction(
      "cleanOrphans",
      "Clean orphan projections",
      options.readOnly,
      live.projections.some((projection) => !projection.binding_id),
      "no orphan projections",
      actionReadOnlyReason,
    ),
    replayQueued: mutationAction(
      "replayQueued",
      "Replay queued writes",
      options.readOnly,
      live.queuedWriteCount > 0,
      "queue empty",
      actionReadOnlyReason,
    ),
    repairHistory: mutationAction(
      "repairHistory",
      "Repair history",
      options.historyReadOnly ?? options.readOnly,
      true,
      "history is read-only",
      options.historyReadOnly && !options.readOnly ? "pending operations must be replayed or purged first" : actionReadOnlyReason,
    ),
    syncPull: mutationAction("syncPull", "Pull remote", options.readOnly, true, "action unavailable", actionReadOnlyReason),
    syncPush: mutationAction(
      "syncPush",
      "Push remote",
      options.readOnly,
      live.queuedWriteCount === 0,
      "replay queued writes first",
      actionReadOnlyReason,
    ),
  };

  return {
    shell: {
      status: statusForLiveData(live),
      pages,
      counts: shellCounts,
      registryRoot: textField(live.registryRoot, "workspace root is unavailable"),
      remoteState: textField(live.remote?.sync_state, "remote sync state is unavailable"),
      readOnly: options.readOnly,
      readOnlyReason,
    },
    skills: live.skills.map(selectSkillViewModel),
    targets: live.targets.map(selectTargetViewModel),
    bindings: live.bindings.map(selectBindingViewModel),
    projections: live.projections.map(selectProjectionViewModel),
    operations: live.ops.map(selectOperationViewModel),
    actions,
    graphLinks: selectProjectionLinks(live.projections),
  };
}

export function selectSkillViewModel(skill: Skill): SkillViewModel {
  const targetCount = Array.isArray(skill.targets) ? skill.targets.length : undefined;
  return {
    id: skill.id,
    name: textField(skill.name, "skill name is unavailable"),
    description: textField(skill.description, "skill description is unavailable"),
    sourceStatus: enumField(skill.sourceStatus, KNOWN_SOURCE_STATUS, "skill source status is unavailable"),
    latestRev: textField(skill.latestRev, "latest revision is unavailable"),
    changed: textField(skill.changed, "latest update time is unavailable"),
    bindings: countField("bindings", "Bindings", skill.bindingCount, "binding count is unavailable"),
    projections: countField("projections", "Projections", skill.projectionCount, "projection count is unavailable"),
    targets: countField("targets", "Targets", targetCount, "target list is unavailable"),
  };
}

export function selectTargetViewModel(target: Target): TargetViewModel {
  return {
    id: target.id,
    agent: textField(target.agent, "target agent is unavailable"),
    agentSource: textField(target.agentSource ?? "unknown", "target agent source is unavailable"),
    profile: textField(target.profile, "target profile is unavailable"),
    path: textField(target.path, "target path is unavailable"),
    ownership: enumField(target.ownership, KNOWN_OWNERSHIP, "target ownership is unavailable"),
    observedSkills: countField("observedSkills", "Observed skills", target.observedSkills, "observed skill count is unavailable"),
    projectedSkills: countField("projectedSkills", "Projected skills", target.projectedSkills, "projected skill count is unavailable"),
  };
}

export function selectBindingViewModel(binding: Binding): BindingViewModel {
  return {
    id: binding.id,
    skill: textField(binding.skill, "binding skill is unavailable"),
    target: textField(binding.target, "binding target is unavailable"),
    matcher: textField(binding.matcher, "workspace matcher is unavailable"),
    method: methodField(binding.method),
    policy: enumField(binding.policy, KNOWN_POLICIES, "binding policy is unavailable"),
  };
}

export function selectProjectionViewModel(projection: RegistryProjection): ProjectionViewModel {
  return {
    id: textField(projection.instance_id, "projection instance id is unavailable"),
    skill: textField(projection.skill_id, "projection skill is unavailable"),
    target: textField(projection.target_id, "projection target is unavailable"),
    binding: textField(projection.binding_id, "projection binding is unavailable"),
    method: methodField(projection.method),
    health: textField(projection.health, "projection health is unavailable"),
    materializedPath: textField(projection.materialized_path, "projection path is unavailable"),
    lastAppliedRev: textField(projection.last_applied_rev, "last applied revision is unavailable"),
    updatedAt: textField(projection.updated_at, "projection update time is unavailable"),
    drifted: Boolean(projection.observed_drift),
  };
}

export function selectOperationViewModel(operation: Op): OperationViewModel {
  return {
    id: textField(operation.id, "operation id is unavailable"),
    status: enumField(operation.status, KNOWN_OPERATION_STATUS, "operation status is unavailable"),
    kind: textField(operation.kind, "operation kind is unavailable"),
    skill: textField(operation.skill, "operation skill is unavailable"),
    target: textField(operation.target, "operation target is unavailable"),
    method: methodField(operation.method, "operation method is unavailable"),
    time: textField(operation.time, "operation timestamp is unavailable"),
    reason: textField(operation.reason, "operation reason is unavailable"),
  };
}
