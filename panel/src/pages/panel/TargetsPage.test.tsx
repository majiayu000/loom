import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, it, expect, vi } from "vitest";
import { TargetsPage } from "./TargetsPage";
import type { Ownership, Skill, Target } from "../../lib/types";
import { api } from "../../lib/api/client";

afterEach(() => {
  vi.restoreAllMocks();
});

function makeTarget(id: string, ownership: Ownership): Target {
  return {
    id,
    agent: "claude",
    profile: "home",
    path: `~/.${ownership}/skills`,
    ownership,
    skills: 0,
    lastSync: "now",
  };
}

function makeObservedSkill(name: string, targetId: string): Skill {
  return {
    id: `s-${name}`,
    name,
    description: null,
    tag: "skill",
    sourceStatus: "present",
    observedImported: true,
    sources: ["observed", "source"],
    releaseTags: [],
    snapshotTags: [],
    latestRev: "—",
    ruleCount: 0,
    bindingCount: 0,
    projectionCount: 0,
    changed: "—",
    targets: [],
    observedTargetIds: [targetId],
  };
}

describe("TargetsPage — ownership tier tooltip", () => {
  it("attaches an explanatory title to each ownership chip", () => {
    const targets: Target[] = [
      makeTarget("target-managed", "managed"),
      makeTarget("target-observed", "observed"),
      makeTarget("target-external", "external"),
    ];

    const { container } = render(
      <TargetsPage
        targets={targets}
        skills={[]}
        selectedTarget={null}
        onSelectTarget={() => {}}
        onRemoveTarget={() => {}}
        onMutation={() => {}}
        readOnly={false}
        mutationVersion={0}
      />,
    );

    const chips = container.querySelectorAll<HTMLSpanElement>("span.chip");
    const byOwnership = new Map<string, string>();
    chips.forEach((chip) => {
      for (const tier of ["managed", "observed", "external"] as const) {
        if (chip.classList.contains(tier) && chip.title) {
          byOwnership.set(tier, chip.title);
        }
      }
    });

    expect(byOwnership.get("managed")).toMatch(/Loom owns this directory/);
    expect(byOwnership.get("observed")).toMatch(/only reads/);
    expect(byOwnership.get("external")).toMatch(/hands-off/);
  });

  it("shows the agent adapter source for each target", () => {
    render(
      <TargetsPage
        targets={[{ ...makeTarget("target-external-agent", "observed"), agent: "fixture-agent", agentSource: "external" }]}
        skills={[]}
        selectedTarget="target-external-agent"
        onSelectTarget={() => {}}
        onRemoveTarget={() => {}}
        onMutation={() => {}}
        readOnly={false}
        mutationVersion={0}
      />,
    );

    expect(screen.getAllByText("external").length).toBeGreaterThan(0);
  });

  it("surfaces explicit import for observed targets when no managed skills exist", async () => {
    const importObserved = vi.spyOn(api, "skillImportObserved").mockResolvedValue({
      ok: true,
      cmd: "skill.import_observed",
      request_id: "req-import",
    });
    const onMutation = vi.fn();

    render(
      <TargetsPage
        targets={[makeTarget("target-observed", "observed")]}
        skills={[]}
        selectedTarget={null}
        onSelectTarget={() => {}}
        onRemoveTarget={() => {}}
        onMutation={onMutation}
        readOnly={false}
        mutationVersion={0}
      />,
    );

    expect(screen.getByText(/Import creates managed registry skills/)).toBeInTheDocument();
    expect(importObserved).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /Import observed skills/ }));

    await waitFor(() => {
      expect(importObserved).toHaveBeenCalledWith();
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });

  it("shows observed target inventory separately from projections", () => {
    vi.spyOn(api, "targetShow").mockResolvedValue({
      ok: true,
      data: {
        target: {
          target_id: "target-observed",
          agent: "claude",
          path: "/tmp/skills",
          ownership: "observed",
          capabilities: { symlink: false, copy: false, watch: true },
        },
        bindings: [],
        projections: [],
      },
    });

    render(
      <TargetsPage
        targets={[
          {
            ...makeTarget("target-observed", "observed"),
            observedSkills: 2,
            projectedSkills: 0,
            skills: 2,
          },
        ]}
        skills={[
          makeObservedSkill("alpha", "target-observed"),
          makeObservedSkill("beta", "target-observed"),
        ]}
        selectedTarget="target-observed"
        onSelectTarget={() => {}}
        onRemoveTarget={() => {}}
        onMutation={() => {}}
        readOnly={false}
        mutationVersion={0}
      />,
    );

    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("observed skills")).toBeInTheDocument();
    expect(screen.getAllByText("0").length).toBeGreaterThan(0);
    expect(screen.getByText("projected")).toBeInTheDocument();
    expect(screen.getByText("Observed skills in this target")).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("beta")).toBeInTheDocument();
  });
});
