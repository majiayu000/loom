import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import { api } from "../../lib/api/client";
import type { Binding, Target } from "../../lib/types";
import { BindingsPage } from "./BindingsPage";

afterEach(() => {
  vi.restoreAllMocks();
});

const target: Target = {
  id: "target-1",
  agent: "claude",
  profile: "home",
  path: "~/.claude/skills",
  ownership: "managed",
  skills: 0,
  lastSync: "now",
};

const binding: Binding = {
  id: "binding-1",
  skill: "skill.writer",
  target: "target-1",
  matcher: "path_prefix:/repo",
  method: "copy",
  policy: "auto",
};

const orphanProjection: RegistryProjection = {
  instance_id: "inst-orphan",
  skill_id: "skill.writer",
  target_id: "target-1",
  materialized_path: "/tmp/orphan",
  method: "copy",
  last_applied_rev: "deadbeefcafebabe",
  health: "orphaned",
};

describe("BindingsPage orphan cleanup", () => {
  it("requires confirmation before deleting live paths", async () => {
    const clean = vi.spyOn(api, "orphanClean").mockResolvedValue({
      ok: true,
      cmd: "skill.orphan.clean",
      request_id: "req-clean",
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);

    render(
      <BindingsPage
        bindings={[binding]}
        targets={[target]}
        projections={[orphanProjection]}
        readOnly={false}
        mutationVersion={0}
        onMutation={() => {}}
      />,
    );

    fireEvent.click(screen.getByLabelText("Also delete all live paths"));
    const destructiveSubmit = screen.getByRole("button", {
      name: "Delete live paths and clean metadata",
    });

    fireEvent.click(destructiveSubmit);

    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("Delete live paths for 1 orphaned projection"));
    expect(clean).not.toHaveBeenCalled();

    confirm.mockReturnValue(true);
    fireEvent.click(destructiveSubmit);

    await waitFor(() => {
      expect(clean).toHaveBeenCalledWith({ delete_live_paths: true });
    });
  });
});
