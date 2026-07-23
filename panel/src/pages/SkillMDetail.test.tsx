import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Skill } from "../lib/types";
import { convergenceApi } from "../lib/api/convergence";
import { SkillMDetail } from "./SkillMDetail";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function skill(name: string): Skill {
  return {
    id: name,
    name,
    tag: "local",
    sourceStatus: "present",
    releaseTags: [],
    snapshotTags: [],
    latestRev: "deadbeef",
    ruleCount: 0,
    bindingCount: 0,
    projectionCount: 0,
    changed: "now",
    targets: [],
  };
}

describe("SkillMDetail", () => {
  it("discards the prior Skill plan when the selected Skill changes", async () => {
    vi.spyOn(convergenceApi, "plan").mockResolvedValue({ ok: true, cmd: "plan.converge", request_id: "req-plan", data: { plan_id: "plan-demo", plan_digest: "sha256:demo", safe_to_apply: true, execution_enabled: true } });
    const { rerender } = render(<SkillMDetail skill={skill("demo")} convergenceSupported onApplied={() => undefined} />);

    await userEvent.click(screen.getByRole("button", { name: "Plan convergence" }));
    expect(screen.getByText("plan_id: plan-demo")).toBeTruthy();

    rerender(<SkillMDetail skill={skill("other")} convergenceSupported onApplied={() => undefined} />);
    expect(screen.getByRole("complementary", { name: "other detail" })).toBeTruthy();
    expect(screen.queryByTestId("convergence-plan-review")).toBeNull();
    expect(screen.queryByRole("button", { name: "Apply reviewed plan" })).toBeNull();
  });
});
