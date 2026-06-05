import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, it, expect, vi } from "vitest";
import { TargetsPage } from "./TargetsPage";
import type { Ownership, Target } from "../../lib/types";
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
});
