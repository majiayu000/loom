import { describe, expect, it } from "vitest";
import { dedupePanelOps } from "./usePanelData";
import type { Op } from "../types";

function op(overrides: Partial<Op>): Op {
  return {
    id: "op-1",
    status: "pending",
    kind: "sync.push",
    skill: "",
    target: "registry",
    method: "copy",
    time: "now",
    ...overrides,
  };
}

describe("dedupePanelOps", () => {
  it("keeps operation backlog rows first and removes matching activity rows", () => {
    const pending = op({ id: "request-1", status: "pending" });
    const activity = op({ id: "request-1", status: "ok" });
    const other = op({ id: "request-2", kind: "skill.commit" });

    expect(dedupePanelOps([pending], [activity, other])).toEqual([pending, other]);
  });

  it("dedupes id-less rows by their visible operation identity", () => {
    const first = op({ id: "", kind: "skill.commit", skill: "docs", target: "codex", time: "10:00" });
    const duplicate = op({ id: "", kind: "skill.commit", skill: "docs", target: "codex", time: "10:00" });
    const changed = op({ id: "", kind: "skill.commit", skill: "docs", target: "claude", time: "10:00" });

    expect(dedupePanelOps([first], [duplicate, changed])).toEqual([first, changed]);
  });
});
