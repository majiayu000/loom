import { expect, test } from "vitest";
import {
  COUNT_TERMS,
  filterLabel,
  formatActionNeededBadge,
  formatQueuedWrites,
  formatReplayableWrites,
  opStatusLabel,
  summarizeOps,
} from "./count_labels";

test("count terms keep queue, activity, and audit sources distinct", () => {
  expect(COUNT_TERMS.queuedWrites).toBe("Queued writes");
  expect(COUNT_TERMS.replayableWrites).toBe("Replayable writes");
  expect(COUNT_TERMS.loadedAuditChanges).toBe("Loaded audit changes");
});

test("summarizeOps derives action-needed counts from replayable and failed rows", () => {
  expect(summarizeOps([{ status: "pending" }, { status: "err" }, { status: "ok" }, { status: "pending" }])).toEqual({
    all: 4,
    pending: 2,
    ok: 1,
    err: 1,
    actionNeeded: 3,
  });
});

test("count labels avoid overloaded pending terminology", () => {
  expect(formatQueuedWrites(1)).toBe("1 queued write");
  expect(formatQueuedWrites(2)).toBe("2 queued writes");
  expect(formatReplayableWrites(1)).toBe("1 replayable write");
  expect(formatActionNeededBadge(3)).toBe("3 needs action");
  expect(opStatusLabel("pending")).toBe("replayable");
  expect(filterLabel("pending")).toBe("replayable");
});
