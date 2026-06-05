import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/api/client";
import type { Target } from "../../lib/types";
import { OverviewPage } from "./OverviewPage";

afterEach(() => {
  vi.restoreAllMocks();
});

function makeTarget(overrides: Partial<Target> = {}): Target {
  return {
    id: "target-observed",
    agent: "claude",
    profile: "home",
    path: "~/.claude/skills",
    ownership: "observed",
    skills: 1,
    lastSync: "now",
    ...overrides,
  };
}

describe("OverviewPage observed import", () => {
  it("imports observed targets from the first managed-skill step", async () => {
    const importObserved = vi.spyOn(api, "skillImportObserved").mockResolvedValue({
      ok: true,
      cmd: "skill.import_observed",
      request_id: "req-import",
    });
    const onMutation = vi.fn();

    render(
      <OverviewPage
        skills={[]}
        targets={[makeTarget()]}
        ops={[]}
        projections={[]}
        vizMode="loom"
        setVizMode={() => {}}
        selectedSkill={null}
        selectedTarget={null}
        onSelectSkill={() => {}}
        onSelectTarget={() => {}}
        registryRoot={null}
        onMutation={onMutation}
        onNewTarget={() => {}}
        onNewBinding={() => {}}
        onOpenSkills={() => {}}
        onViewActivity={() => {}}
        onOpenSync={() => {}}
        readOnly={false}
      />,
    );

    expect(screen.getByText(/Import creates managed skills/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Import observed skills/ }));

    await waitFor(() => {
      expect(importObserved).toHaveBeenCalledWith();
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });
});
