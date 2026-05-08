import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { TargetsPage } from "./TargetsPage";
import type { Ownership, Target } from "../../lib/types";

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
});
