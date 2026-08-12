import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/api/client";
import { UseSkillForm } from "./UseSkillForm";

afterEach(() => vi.restoreAllMocks());

describe("UseSkillForm", () => {
  it("forwards explicit observed-skill adoption intent", async () => {
    const useSkill = vi.spyOn(api, "skillUse").mockResolvedValue({
      ok: true,
      cmd: "skill.use",
      request_id: "req-use",
      data: { steps: [] },
    });

    render(<UseSkillForm skillName="demo" targets={[]} readOnly={false} onMutation={() => {}} />);
    fireEvent.click(screen.getByRole("checkbox", { name: /Adopt an existing observed skill/ }));
    fireEvent.click(screen.getByRole("button", { name: "Plan" }));

    await waitFor(() => {
      expect(useSkill).toHaveBeenCalledWith(
        "demo",
        expect.objectContaining({ adopt: true, apply: false }),
      );
    });
  });
});
