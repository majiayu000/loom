import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SkillMPanel } from "./SkillMPanel";

const panelData = vi.hoisted(() => ({
  refetch: vi.fn(),
  current: null as null | {
    live: boolean;
    apiReachable: boolean;
    loading: boolean;
    error: string | null;
    mode: "live" | "first-run" | "offline-empty" | "offline-stale";
    setupRequired: boolean;
    lastUpdated: string | null;
    registryRoot: string | null;
    agentDirs: unknown[];
    remote: null;
    warnings: string[];
    health: { service: string };
    counts: Record<string, never>;
    skills: unknown[];
    targets: unknown[];
    bindings: unknown[];
    ops: Array<{
      id: string;
      kind: string;
      skill: string;
      target: string;
      status: "ok" | "err" | "pending";
      time: string;
      reason?: string;
      method?: string;
    }>;
    projections: unknown[];
    queuedWriteCount: number;
  },
  firstRun: {
    live: true,
    apiReachable: true,
    loading: false,
    error: null,
    mode: "first-run" as const,
    setupRequired: true,
    lastUpdated: "2026-06-12T00:00:00.000Z",
    registryRoot: "/tmp/loom-registry",
    agentDirs: [],
    remote: null,
    warnings: [],
    health: { service: "loom-panel" },
    counts: {},
    skills: [],
    targets: [],
    bindings: [],
    ops: [],
    projections: [],
    queuedWriteCount: 0,
  },
  liveOps: {
    live: true,
    apiReachable: true,
    loading: false,
    error: null,
    mode: "live" as const,
    setupRequired: false,
    lastUpdated: "2026-06-12T00:00:00.000Z",
    registryRoot: "/tmp/loom-registry",
    agentDirs: [],
    remote: null,
    warnings: [],
    health: { service: "loom-panel" },
    counts: {},
    skills: [],
    targets: [],
    bindings: [],
    ops: [
      {
        id: "op-ok",
        kind: "skill.save",
        skill: "docs",
        target: "codex",
        status: "ok" as const,
        time: "2026-06-12 09:00",
        method: "copy",
      },
      {
        id: "op-pending",
        kind: "sync.push",
        skill: "deploy",
        target: "claude",
        status: "pending" as const,
        time: "2026-06-12 09:05",
        reason: "queued",
      },
    ],
    projections: [],
    queuedWriteCount: 0,
  },
}));

vi.mock("../lib/api/usePanelData", () => ({
  usePanelData: () => ({
    ...(panelData.current ?? panelData.firstRun),
    refetch: panelData.refetch,
  }),
}));

afterEach(() => {
  cleanup();
  panelData.refetch.mockReset();
  panelData.current = null;
  window.history.replaceState(null, "", "/");
});

describe("SkillMPanel", () => {
  it("shows the real first-run initialization flow when registry state is missing", async () => {
    panelData.current = panelData.firstRun;
    render(<SkillMPanel />);

    expect(await screen.findByRole("heading", { name: "Initialize Registry" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Initialize" })).toBeTruthy();
    expect(screen.queryByText("Skill 真实统计")).toBeNull();
  });

  it("switches between queued ops and audit history tabs", async () => {
    panelData.current = panelData.liveOps;
    window.history.replaceState(null, "", "/?view=ops");

    render(<SkillMPanel />);

    expect(screen.getByText("sync.push")).toBeTruthy();
    expect(screen.queryByText("skill.save")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: /审计历史/ }));

    expect(screen.getByText("skill.save")).toBeTruthy();
    expect(new URL(window.location.href).searchParams.get("view")).toBe("history");
  });
});
