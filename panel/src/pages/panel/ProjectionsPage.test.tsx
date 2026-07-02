import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/api/client";
import type { Binding, Target } from "../../lib/types";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import { ProjectionsPage } from "./ProjectionsPage";

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

function projection(overrides: Partial<RegistryProjection> = {}): RegistryProjection {
  return {
    instance_id: "inst-visible",
    skill_id: "skill.writer",
    binding_id: "binding-1",
    target_id: "target-1",
    materialized_path: "/tmp/visible",
    method: "copy",
    last_applied_rev: "deadbeefcafebabe",
    health: "healthy",
    ...overrides,
  };
}

function projectionRow(instanceId: string): HTMLTableRowElement {
  const cell = screen.getAllByText(instanceId).find((element) => element.closest("tr"));
  const row = cell?.closest("tr");
  if (!row) throw new Error(`projection row ${instanceId} not found`);
  return row as HTMLTableRowElement;
}

describe("ProjectionsPage orphan cleanup", () => {
  it("requires confirmation before deleting live paths from the bulk cleanup surface", async () => {
    const orphan = projection({
      instance_id: "inst-orphan",
      binding_id: undefined,
      materialized_path: "/tmp/orphan",
      health: "orphaned",
    });
    const clean = vi.spyOn(api, "orphanClean").mockResolvedValue({
      ok: true,
      cmd: "skill.orphan.clean",
      request_id: "req-clean",
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);

    render(
      <ProjectionsPage
        projections={[orphan]}
        targets={[target]}
        bindings={[binding]}
        readOnly={false}
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

describe("ProjectionsPage filters", () => {
  it("keeps detail actions constrained to the active filter", async () => {
    const visible = projection({ instance_id: "inst-visible", materialized_path: "/tmp/visible" });
    const hidden = projection({
      instance_id: "inst-hidden",
      binding_id: undefined,
      materialized_path: "/tmp/hidden",
      health: "orphaned",
    });
    const commitProjection = vi.spyOn(api, "commitProjection").mockResolvedValue({
      ok: true,
      cmd: "skill.commit",
      request_id: "req-capture",
    });

    render(
      <ProjectionsPage
        projections={[visible, hidden]}
        targets={[target]}
        bindings={[binding]}
        readOnly={false}
        onMutation={() => {}}
      />,
    );

    fireEvent.click(projectionRow("inst-hidden"));
    expect(screen.getByText("/tmp/hidden")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "healthy" }));
    expect(screen.queryByText("/tmp/hidden")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Commit" }));

    await waitFor(() => {
      expect(commitProjection).toHaveBeenCalledWith({ skill: "skill.writer", instance: "inst-visible" });
    });
  });
});
