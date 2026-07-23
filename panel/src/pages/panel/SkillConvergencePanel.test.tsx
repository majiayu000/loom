import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { convergenceApi } from "../../lib/api/convergence";
import { SkillConvergencePanel } from "./SkillConvergencePanel";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe("SkillConvergencePanel", () => {
  it("hides the mutation when the backend does not report apply support", () => {
    render(<SkillConvergencePanel skillName="demo" supported={false} onApplied={() => undefined} />);
    expect(screen.queryByRole("region", { name: "Skill convergence" })).toBeNull();
  });

  it("reviews one immutable plan before applying its exact digest and stable key", async () => {
    const plan = vi.spyOn(convergenceApi, "plan").mockResolvedValue({ ok: true, cmd: "plan.converge", request_id: "req-plan", data: { plan_id: "plan-1", plan_digest: "sha256:plan-1", safe_to_apply: true, execution_enabled: true, effects: [{ instance_id: "projection-1", action: "refresh" }], risks: [{ code: "review-fixture" }], input_conflicts: [], required_approvals: [] } });
    const apply = vi.spyOn(convergenceApi, "apply").mockResolvedValue({ ok: true, cmd: "apply", request_id: "req-apply", data: { complete: false, completion_blockers: ["visibility.restart_required"] } });
    const onApplied = vi.fn();
    render(<SkillConvergencePanel skillName="demo" supported onApplied={onApplied} />);

    await userEvent.click(screen.getByRole("button", { name: "Plan convergence" }));
    expect(plan).toHaveBeenCalledWith("demo", expect.objectContaining({ push_remote: false }));
    expect(apply).not.toHaveBeenCalled();
    expect(screen.getByText("plan_id: plan-1")).toBeTruthy();
    expect(screen.getByText(/"action": "refresh"/)).toBeTruthy();
    expect(screen.getByText(/"code": "review-fixture"/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply reviewed plan" })).toBeDisabled();

    await userEvent.click(screen.getByRole("checkbox", { name: /I reviewed this exact plan/ }));
    const keyInput = screen.getByRole("textbox", { name: "Convergence idempotency key" }) as HTMLInputElement;
    const key = keyInput.value;
    expect(keyInput).toHaveAttribute("readonly");
    expect(keyInput.readOnly).toBe(true);
    expect(keyInput).toHaveValue(key);
    await userEvent.click(screen.getByRole("button", { name: "Apply reviewed plan" }));
    await waitFor(() => expect(apply).toHaveBeenCalledWith({ plan_id: "plan-1", plan_digest: "sha256:plan-1", idempotency_key: key, approvals: [] }));
    expect(screen.getByText(/visibility\.restart_required/)).toBeTruthy();
    expect(onApplied).toHaveBeenCalledTimes(1);
  });

  it("shows restart-required next actions even when apply reports complete", async () => {
    vi.spyOn(convergenceApi, "plan").mockResolvedValue({ ok: true, cmd: "plan.converge", request_id: "req-plan", data: { plan_id: "plan-1", plan_digest: "sha256:plan-1", safe_to_apply: true, execution_enabled: true } });
    vi.spyOn(convergenceApi, "apply").mockResolvedValue({ ok: true, cmd: "apply", request_id: "req-apply", data: { complete: true, outcome: "complete_with_restart_required", completion_blockers: [], next_actions: [{ reason: "restart the affected agent runtime first, then recheck visibility", cmd: "loom skill visibility demo --agent codex" }] } });
    render(<SkillConvergencePanel skillName="demo" supported onApplied={() => undefined} />);

    await userEvent.click(screen.getByRole("button", { name: "Plan convergence" }));
    await userEvent.click(screen.getByRole("checkbox", { name: /I reviewed this exact plan/ }));
    await userEvent.click(screen.getByRole("button", { name: "Apply reviewed plan" }));

    expect(await screen.findByText(/complete_with_restart_required/)).toBeTruthy();
    expect(screen.getByText(/restart the affected agent runtime first/)).toBeTruthy();
    expect(screen.getByText(/loom skill visibility demo --agent codex/)).toBeTruthy();
  });

  it("invalidates the reviewed plan when a plan selector changes", async () => {
    vi.spyOn(convergenceApi, "plan").mockResolvedValue({ ok: true, cmd: "plan.converge", request_id: "req-plan", data: { plan_id: "plan-1", plan_digest: "sha256:plan-1", safe_to_apply: true, execution_enabled: true } });
    const apply = vi.spyOn(convergenceApi, "apply");
    render(<SkillConvergencePanel skillName="demo" supported onApplied={() => undefined} />);

    await userEvent.click(screen.getByRole("button", { name: "Plan convergence" }));
    await userEvent.click(screen.getByRole("checkbox", { name: /I reviewed this exact plan/ }));
    expect(screen.getByRole("button", { name: "Apply reviewed plan" })).toBeEnabled();

    await userEvent.click(screen.getByRole("checkbox", { name: "push remote last" }));
    expect(screen.queryByTestId("convergence-plan-review")).toBeNull();
    expect(screen.queryByRole("button", { name: "Apply reviewed plan" })).toBeNull();
    expect(apply).not.toHaveBeenCalled();
  });
});
