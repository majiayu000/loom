import type { Op, OpStatus } from "./types";

export const COUNT_TERMS = {
  queuedWrites: "Queued writes",
  actionNeeded: "Action needed",
  activityRows: "Activity rows",
  replayableWrites: "Replayable writes",
  loadedAuditChanges: "Loaded audit changes",
  succeeded: "Succeeded",
  failed: "Failed",
} as const;

export interface OperationCounts {
  all: number;
  pending: number;
  ok: number;
  err: number;
  actionNeeded: number;
}

export function summarizeOps(ops: Array<Pick<Op, "status">>): OperationCounts {
  const counts: OperationCounts = { all: ops.length, pending: 0, ok: 0, err: 0, actionNeeded: 0 };
  for (const op of ops) {
    counts[op.status] += 1;
  }
  counts.actionNeeded = counts.pending + counts.err;
  return counts;
}

export function formatQueuedWrites(count: number): string {
  return `${count} queued ${plural(count, "write", "writes")}`;
}

export function formatReplayableWrites(count: number): string {
  return `${count} replayable ${plural(count, "write", "writes")}`;
}

export function formatActionNeededBadge(count: number): string {
  return `${count} needs action`;
}

export function opStatusLabel(status: OpStatus): string {
  if (status === "ok") return "done";
  if (status === "err") return "failed";
  return "replayable";
}

export function filterLabel(key: "all" | OpStatus): string {
  if (key === "pending") return "replayable";
  if (key === "ok") return "done";
  if (key === "err") return "failed";
  return key;
}

function plural(count: number, singular: string, pluralValue: string): string {
  return count === 1 ? singular : pluralValue;
}
