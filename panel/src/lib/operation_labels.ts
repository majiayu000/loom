import type { RegistryOperationRecord } from "./api/client";
import type { Op, OpStatus } from "./types";

export interface OperationLabel {
  category: string;
  title: string;
  details: string[];
  technicalId: string;
}

type OperationStatus = "ok" | "pending" | "err";

interface IntentDescriptor {
  category: string;
  phrase: string;
}

const INTENT_LABELS: Record<string, IntentDescriptor> = {
  "target.add": { category: "Target", phrase: "target registration" },
  "target.remove": { category: "Target", phrase: "target removal" },
  "workspace.binding.add": { category: "Binding", phrase: "workspace binding registration" },
  "workspace.binding.remove": { category: "Binding", phrase: "workspace binding removal" },
  use: { category: "Use", phrase: "skill use flow" },
  "skill.project": { category: "Projection", phrase: "skill projection" },
  "skill.commit": { category: "Commit", phrase: "skill commit" },
  "skill.import_observed": { category: "Import", phrase: "observed skill import" },
  "skill.monitor_observed": { category: "Monitor", phrase: "observed skill monitor" },
  "import-observed": { category: "Import", phrase: "observed skill import" },
  "monitor-observed": { category: "Monitor", phrase: "observed skill monitor" },
  "skill.orphan.clean": { category: "Cleanup", phrase: "orphan cleanup" },
  "skill.release": { category: "Release", phrase: "skill release" },
  "sync.push": { category: "Sync", phrase: "remote sync push" },
  "sync.pull": { category: "Sync", phrase: "remote sync pull" },
  "sync.replay": { category: "Sync", phrase: "pending sync replay" },
  "sync-push": { category: "Sync", phrase: "remote sync push" },
  "sync-pull": { category: "Sync", phrase: "remote sync pull" },
  "sync-replay": { category: "Sync", phrase: "pending sync replay" },
};

const ACTION_LABELS: Record<string, string> = {
  "target.add": "注册目标目录",
  "target.remove": "移除目标目录",
  "workspace.binding.add": "新增绑定规则",
  "workspace.binding.remove": "移除绑定规则",
  use: "使用技能流程",
  "skill.project": "投影到目标目录",
  "skill.commit": "提交技能变更",
  "skill.import_observed": "导入观测到的技能",
  "skill.monitor_observed": "扫描观测目录",
  "import-observed": "导入观测到的技能",
  "monitor-observed": "扫描观测目录",
  "skill.orphan.clean": "清理孤立技能",
  "skill.release": "发布版本或锚点",
  "skill.rollback": "回滚技能",
  "sync.push": "推送远端同步",
  "sync.pull": "拉取远端同步",
  "sync.replay": "重放同步队列",
  "sync-push": "推送远端同步",
  "sync-pull": "拉取远端同步",
  "sync-replay": "重放同步队列",
};

export function describeActivityOperation(op: Op): OperationLabel {
  const descriptor = descriptorForIntent(op.kind);
  const status = statusWord(op.status);
  const subject = subjectForActivity(op, descriptor);
  const targetDetail = meaningfulField(op.target) ? `target ${op.target}` : null;
  const methodDetail = meaningfulField(op.method) ? `method ${op.method}` : null;
  const details = compact([
    `intent ${normalizeIntent(op.kind)}`,
    targetDetail,
    methodDetail,
    `id ${op.id}`,
    op.reason ? `${op.status === "err" ? "blocked" : "note"} ${op.reason}` : null,
  ]);

  return {
    category: descriptor.category,
    title: buildTitle(subject, descriptor.phrase, status),
    details,
    technicalId: op.id,
  };
}

export function describeRegistryOperation(op: RegistryOperationRecord): OperationLabel {
  const descriptor = descriptorForIntent(op.intent);
  const status = statusWord(bucketRegistryOperation(op));
  const technicalId = registryOperationDisplayId(op);
  const subject = subjectForRegistryOperation(op, descriptor);
  const skills = meaningfulString(op.skill) ? splitOperationSkills(op.skill) : [];
  const details = compact([
    `intent ${normalizeIntent(op.intent)}`,
    skills.length > 3 ? `skills ${skills.length}` : skills.length > 0 ? `skill ${skills.join(", ")}` : null,
    meaningfulString(op.target) ? `target ${op.target}` : null,
    meaningfulString(op.binding) ? `binding ${op.binding}` : null,
    meaningfulString(op.method) ? `method ${op.method}` : null,
    op.ack ? "synced" : "not synced",
    `id ${technicalId}`,
  ]);

  return {
    category: descriptor.category,
    title: buildTitle(subject, descriptor.phrase, status),
    details,
    technicalId,
  };
}

export function operationActionLabel(kind: string): string {
  const normalized = normalizeIntent(kind);
  return ACTION_LABELS[normalized] ?? normalized.replace(/[._-]/g, " ");
}

export function operationStatusLabel(status: OpStatus): string {
  if (status === "ok") return "已完成";
  if (status === "err") return "失败";
  return "待处理";
}

export function registryOperationStatusLabel(op: RegistryOperationRecord): string {
  const status = bucketRegistryOperation(op);
  if (status === "ok") return "已完成";
  if (status === "err") return "失败";
  return "待处理";
}

export function splitOperationSkills(value: string): string[] {
  return value
    .split(",")
    .map((part) => part.trim().replace(/@\S+$/, ""))
    .filter((part, index, items) => part.length > 0 && items.indexOf(part) === index);
}

export function registryOperationSubjectLabel(op: RegistryOperationRecord): string {
  const skills = meaningfulString(op.skill) ? splitOperationSkills(op.skill) : [];
  if (skills.length > 3) return `${skills.length} 个 skill`;
  if (skills.length > 0) return skills.join(", ");
  if (meaningfulString(op.target)) return op.target;
  if (meaningfulString(op.binding)) return op.binding;
  if (meaningfulString(op.source)) return op.source;
  return operationActionLabel(op.intent);
}

export function operationSubjectLabel(op: Op): string {
  const skills = splitOperationSkills(op.skill).filter((name) => name !== op.kind);
  if (skills.length > 3) return `${skills.length} 个 skill`;
  if (skills.length > 0) return skills.join(", ");
  if (meaningfulField(op.target)) return op.target;
  return operationActionLabel(op.kind);
}

export function registryOperationTargetLabel(op: RegistryOperationRecord): string {
  return op.target ?? op.binding ?? op.source ?? "registry";
}

export function registryOperationDetailParts(op: RegistryOperationRecord): string[] {
  const skills = meaningfulString(op.skill) ? splitOperationSkills(op.skill) : [];
  return compact([
    skills.length > 3 ? `本次批量操作包含 ${skills.length} 个 skill` : null,
    meaningfulString(op.target) ? `target ${op.target}` : null,
    meaningfulString(op.binding) ? `binding ${op.binding}` : null,
    meaningfulString(op.method) ? `方式 ${op.method}` : null,
    op.last_error?.message ? `错误 ${op.last_error.message}` : null,
    op.ack ? "已同步" : "未同步",
    `id ${registryOperationDisplayId(op)}`,
  ]);
}

export function operationDetailParts(op: Op): string[] {
  return compact([
    meaningfulField(op.target) ? `target ${op.target}` : null,
    meaningfulField(op.method) ? `方式 ${op.method}` : null,
    op.reason ? `${op.status === "err" ? "错误" : "说明"} ${op.reason}` : null,
    `id ${op.id}`,
  ]);
}

export function registryOperationDisplayId(op: RegistryOperationRecord): string {
  return op.op_id ?? op.audit_id ?? op.request_id ?? `${op.intent}-${op.updated_at}`;
}

export function bucketRegistryOperation(op: RegistryOperationRecord): OperationStatus {
  if (op.last_error) return "err";
  const s = op.status.toLowerCase();
  if (s === "pending" || s === "enqueued" || s === "in_flight" || s === "retrying") return "pending";
  if (s === "ok" || s === "applied" || s === "completed" || s === "done" || s === "succeeded") return "ok";
  if (s === "err" || s === "error" || s === "failed") return "err";
  return op.ack ? "ok" : "pending";
}

export function statusWord(status: OperationStatus): string {
  if (status === "ok") return "done";
  if (status === "err") return "failed";
  return "pending";
}

function descriptorForIntent(intent: string): IntentDescriptor {
  const normalized = normalizeIntent(intent);
  return INTENT_LABELS[normalized] ?? {
    category: titleCase(normalized.split(/[._-]/)[0] ?? "Change"),
    phrase: normalized.replace(/[._-]/g, " "),
  };
}

function subjectForActivity(op: Op, descriptor: IntentDescriptor): string {
  const targetSubject = subjectFromTarget(op.target);
  if (descriptor.category === "Target") return targetSubject ?? "";
  if (descriptor.category === "Binding") return subjectFromBinding(op.target) ?? targetSubject ?? "";
  if (meaningfulField(op.skill) && op.skill !== op.kind) return op.skill;
  return targetSubject ?? "";
}

function subjectForRegistryOperation(op: RegistryOperationRecord, descriptor: IntentDescriptor): string {
  const targetSubject = subjectFromTarget(op.target ?? null);
  if (descriptor.category === "Target") return targetSubject ?? "";
  if (descriptor.category === "Binding") return subjectFromBinding(op.binding ?? null) ?? targetSubject ?? "";
  if (meaningfulString(op.skill)) {
    const skills = splitOperationSkills(op.skill);
    return skills.length > 3 ? `${skills.length} skills` : skills.join(", ");
  }
  return targetSubject ?? "";
}

function subjectFromTarget(target: string | null | undefined): string | null {
  const value = target?.trim();
  if (!value || value === "—") return null;
  const withoutPrefix = value.startsWith("target_") ? value.slice("target_".length) : value;
  const candidate = withoutPrefix.split(/[_/-]/).find((part) => part.length > 0);
  return candidate && candidate !== "target" ? titleCase(candidate) : null;
}

function subjectFromBinding(binding: string | null | undefined): string | null {
  const value = binding?.trim();
  if (!value || value === "—") return null;
  return value.startsWith("bind_") ? `${titleCase(value.slice("bind_".length).split(/[_/-]/)[0] ?? "")} binding` : "Workspace";
}

function normalizeIntent(intent: string): string {
  return intent.trim().toLowerCase();
}

function meaningfulField(value: string | null | undefined): value is string {
  return Boolean(value && value.trim() !== "" && value !== "—");
}

function meaningfulString(value: string | null | undefined): value is string {
  return Boolean(value && value.trim() !== "");
}

function titleCase(value: string): string {
  if (!value) return value;
  return value.charAt(0).toUpperCase() + value.slice(1).replace(/-/g, " ");
}

function buildTitle(subject: string, phrase: string, status: string): string {
  const action = subject ? `${subject} ${phrase}` : titleCase(phrase);
  return `${action} ${status}`;
}

function compact(values: Array<string | null | undefined>): string[] {
  return values.filter((value): value is string => Boolean(value && value.trim() !== ""));
}
