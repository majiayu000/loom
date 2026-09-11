import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/api/client";
import { UseSkillForm } from "./UseSkillForm";

afterEach(() => vi.restoreAllMocks());

describe("UseSkillForm", () => {
  it("disables Plan/Apply and skips skillUse when workspace is blank", async () => {
    const useSkill = vi.spyOn(api, "skillUse").mockResolvedValue({
      ok: true,
      cmd: "skill.use",
      request_id: "req-use",
      data: { steps: [] },
    });

    render(<UseSkillForm skillName="demo" targets={[]} readOnly={false} onMutation={() => {}} />);

    expect(screen.getByRole("button", { name: "Plan" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Plan" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(useSkill).not.toHaveBeenCalled();
  });

  it("forwards explicit managed-target adoption intent when workspace is set", async () => {
    const useSkill = vi.spyOn(api, "skillUse").mockResolvedValue({
      ok: true,
      cmd: "skill.use",
      request_id: "req-use",
      data: { steps: [] },
    });

    render(<UseSkillForm skillName="demo" targets={[]} readOnly={false} onMutation={() => {}} />);
    fireEvent.change(screen.getByPlaceholderText("workspace path"), {
      target: { value: "/repo/demo" },
    });
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: /Adopt the existing agent skills directory as a managed Loom target/,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Plan" }));

    await waitFor(() => {
      expect(useSkill).toHaveBeenCalledWith(
        "demo",
        expect.objectContaining({
          adopt: true,
          apply: false,
          workspace: "/repo/demo",
          scope: "project",
        }),
      );
    });
  });
});
