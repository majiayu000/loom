import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import { api } from "../../lib/api/client";
import type { Binding, Op, Target } from "../../lib/types";
import { OverviewPage } from "./OverviewPage";
import { ZERO_OPERATION_COUNTS } from "../../types";

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

function renderOverview(overrides: Partial<React.ComponentProps<typeof OverviewPage>> = {}) {
  return render(
    <OverviewPage
      skills={[]}
      targets={[makeTarget()]}
      bindings={[]}
      ops={[]}
      projections={[]}
      registryProjections={[]}
      remoteState="CLEAN"
      queuedWriteCount={0}
      operationCounts={ZERO_OPERATION_COUNTS}
      vizMode="loom"
      setVizMode={() => {}}
      selectedSkill={null}
      selectedTarget={null}
      onSelectSkill={() => {}}
      onSelectTarget={() => {}}
      registryRoot={null}
      onMutation={() => {}}
      onNewTarget={() => {}}
      onNewBinding={() => {}}
      onOpenSkills={() => {}}
      onViewActivity={() => {}}
      onOpenSync={() => {}}
      readOnly={false}
      {...overrides}
    />,
  );
}

function controlCard(label: string): HTMLElement {
  const card = screen.getByText(label).closest(".kpi");
  expect(card).toBeTruthy();
  return card as HTMLElement;
}

describe("OverviewPage observed import", () => {
  it("renders API-backed control-room summary cards", () => {
    const binding: Binding = {
      id: "binding-1",
      skill: "skill.writer",
      target: "target-managed",
      matcher: "path_prefix:/repo",
      method: "copy",
      policy: "auto",
    };
    const op: Op = {
      id: "op-1",
      status: "err",
      kind: "sync.push",
      skill: "skill.writer",
      target: "target-managed",
      method: "copy",
      time: "now",
      reason: "push rejected",
    };
    const registryProjections: RegistryProjection[] = [
      {
        instance_id: "projection-1",
        skill_id: "skill.writer",
        target_id: "target-managed",
        materialized_path: "/tmp/target/skill.writer",
        method: "copy",
        last_applied_rev: "abc1234",
        health: "healthy",
      },
      {
        instance_id: "projection-2",
        skill_id: "skill.writer",
        target_id: "target-observed",
        materialized_path: "/tmp/target/skill.reader",
        method: "symlink",
        last_applied_rev: "def5678",
        health: "drifted",
      },
    ];

    renderOverview({
      skills: [],
      targets: [
        makeTarget({ id: "target-managed", ownership: "managed" }),
        makeTarget({ id: "target-observed", ownership: "observed" }),
        makeTarget({ id: "target-external", ownership: "external" }),
      ],
      bindings: [binding],
      ops: [op],
      projections: [],
      registryProjections,
      remoteState: "PENDING_PUSH",
      queuedWriteCount: 3,
      vizMode: "loom",
      setVizMode: () => {},
      selectedSkill: null,
      selectedTarget: null,
      onSelectSkill: () => {},
      onSelectTarget: () => {},
      registryRoot: "/tmp/loom-registry",
      onMutation: () => {},
      onNewTarget: () => {},
      onNewBinding: () => {},
      onOpenSkills: () => {},
      onViewActivity: () => {},
      onOpenSync: () => {},
      readOnly: false,
    });

    const summary = screen.getByText("Registry root").closest(".kpi-row");
    expect(summary).toBeTruthy();
    expect(within(summary as HTMLElement).getByText("/tmp/loom-registry")).toBeInTheDocument();
    expect(screen.getByText("pending push")).toBeInTheDocument();
    expect(screen.getByText("3 queued writes")).toBeInTheDocument();
    expect(screen.getByText("Actionable operations")).toBeInTheDocument();
    expect(screen.getByText("Local journal events")).toBeInTheDocument();
    expect(screen.getByText("Unpushed history events")).toBeInTheDocument();
    expect(screen.getByText("Local-only history events")).toBeInTheDocument();
    expect(screen.getByText("managed 1 · observed 1 · external 1")).toBeInTheDocument();
    expect(screen.getByText("symlink 1 · copy 1")).toBeInTheDocument();
    expect(screen.getByText("drifted 1 · healthy 1")).toBeInTheDocument();
    expect(screen.getByText("sync.push")).toBeInTheDocument();
    expect(screen.getByText(/err · skill\.writer \/ target-managed \/ copy/)).toBeInTheDocument();
  });

  it("imports observed targets from the first managed-skill step", async () => {
    const importObserved = vi.spyOn(api, "skillImportObserved").mockResolvedValue({
      ok: true,
      cmd: "skill.import_observed",
      request_id: "req-import",
    });
    const onMutation = vi.fn();

    renderOverview({
      skills: [],
      targets: [makeTarget()],
      bindings: [],
      ops: [],
      projections: [],
      registryProjections: [],
      remoteState: "CLEAN",
      queuedWriteCount: 0,
      vizMode: "loom",
      setVizMode: () => {},
      selectedSkill: null,
      selectedTarget: null,
      onSelectSkill: () => {},
      onSelectTarget: () => {},
      registryRoot: null,
      onMutation,
      onNewTarget: () => {},
      onNewBinding: () => {},
      onOpenSkills: () => {},
      onViewActivity: () => {},
      onOpenSync: () => {},
      readOnly: false,
    });

    expect(screen.getByText(/Import creates managed skills/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Import observed skills/ }));

    await waitFor(() => {
      expect(importObserved).toHaveBeenCalledWith();
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });

  it("marks divergent sync and non-healthy projections as attention states", () => {
    renderOverview({
      remoteState: "DIVERGED",
      registryProjections: [
        {
          instance_id: "projection-missing",
          skill_id: "skill.writer",
          target_id: "target-observed",
          materialized_path: "/tmp/target/skill.writer",
          method: "copy",
          last_applied_rev: "abc1234",
          health: "missing",
        },
      ],
    });

    expect(controlCard("Registry transport")).toHaveAttribute("data-tone", "err");
    expect(controlCard("Projection health")).toHaveAttribute("data-tone", "warn");
  });

  it("marks conflicted sync and projection conflicts as errors", () => {
    renderOverview({
      remoteState: "CONFLICTED",
      registryProjections: [
        {
          instance_id: "projection-conflict",
          skill_id: "skill.writer",
          target_id: "target-observed",
          materialized_path: "/tmp/target/skill.writer",
          method: "copy",
          last_applied_rev: "abc1234",
          health: "conflict",
        },
      ],
    });

    expect(controlCard("Registry transport")).toHaveAttribute("data-tone", "err");
    expect(controlCard("Projection health")).toHaveAttribute("data-tone", "err");
  });
});
