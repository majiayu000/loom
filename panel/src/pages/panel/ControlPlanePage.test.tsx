import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { Binding, Skill, Target } from "../../lib/types";
import { ControlPlanePage } from "./ControlPlanePage";

const target: Target = {
  id: "target-1",
  agent: "claude",
  profile: "home",
  path: "~/.claude/skills",
  ownership: "managed",
  skills: 1,
  projectedSkills: 1,
  lastSync: "now",
};

const skill: Skill = {
  id: "s-skill.writer",
  name: "skill.writer",
  tag: "v1",
  sourceStatus: "present",
  releaseTags: ["v1"],
  snapshotTags: [],
  latestRev: "abc12345",
  ruleCount: 2,
  bindingCount: 1,
  projectionCount: 2,
  changed: "now",
  targets: ["target-1"],
};

const multiBinding: Binding = {
  id: "binding-1",
  skill: "multi",
  target: "target-1",
  matcher: "path_prefix:/repo",
  method: "unknown",
  policy: "auto",
  ruleCount: 2,
  skillCount: 2,
};

const projections: RegistryProjection[] = [
  {
    instance_id: "projection-1",
    skill_id: "skill.writer",
    binding_id: "binding-1",
    target_id: "target-1",
    materialized_path: "/tmp/projection-1",
    method: "copy",
    last_applied_rev: "abc12345",
    health: "healthy",
  },
  {
    instance_id: "projection-2",
    skill_id: "skill.reader",
    binding_id: "binding-1",
    target_id: "target-1",
    materialized_path: "/tmp/projection-2",
    method: "symlink",
    last_applied_rev: "def67890",
    health: "drifted",
  },
];

function renderControlPlane(initialTab: "targets" | "bindings" | "projections" = "targets") {
  const onNavigate = vi.fn();
  render(
    <ControlPlanePage
      initialTab={initialTab}
      skills={[skill]}
      targets={[target]}
      bindings={[multiBinding]}
      projections={projections}
      selectedTarget={null}
      onSelectTarget={() => {}}
      selectedBinding={null}
      onSelectBinding={() => {}}
      onRemoveTarget={() => {}}
      onMutation={() => {}}
      onNavigate={onNavigate}
      readOnly={false}
      mutationVersion={0}
    />,
  );
  return { onNavigate };
}

describe("ControlPlanePage", () => {
  it("keeps direct target, binding, and projection entry points as tabs", () => {
    renderControlPlane("bindings");

    expect(screen.getByRole("heading", { name: "Control Plane" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Bindings" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText(/Rules mapping skills to targets/)).toBeInTheDocument();
  });

  it("renders a graph from real projection method and health values", () => {
    renderControlPlane("targets");

    fireEvent.click(screen.getByRole("tab", { name: "Graph" }));

    expect(screen.getByText("copy 1 · symlink 1")).toBeInTheDocument();
    expect(screen.getAllByText("healthy 1 · drifted 1").length).toBeGreaterThan(0);
    expect(screen.getByText("writes projections · 1 binding · 2 projections")).toBeInTheDocument();

    const routes = screen.getByRole("table");
    expect(within(routes).getAllByText("multi")).toHaveLength(2);
    expect(within(routes).getByText("binding-1")).toBeInTheDocument();
  });

  it("navigates to the matching page key when a direct tab is selected", () => {
    const { onNavigate } = renderControlPlane("targets");

    fireEvent.click(screen.getByRole("tab", { name: "Projections" }));

    expect(onNavigate).toHaveBeenCalledWith("projections");
  });
});
