import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api } from "../lib/api/client";
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
      {
        id: "op-bulk",
        kind: "skill.monitor_observed",
        skill: "aiproxy-workflow-auth-debug, ask-claude, ask-gemini, code-review",
        target: "target_codex_home",
        status: "pending" as const,
        time: "2026-06-12 09:06",
        method: "—",
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
  vi.restoreAllMocks();
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
    const auditSkillList = "agentsmd-audit, ai-slop-cleaner, aiproxy-workflow-auth-debug, app-sizzle, app-store-screens";
    const ops = vi.spyOn(api, "ops").mockResolvedValue({
      ok: true,
      data: {
        count: 40,
        loaded_count: 2,
        offset: 0,
        limit: 100,
        has_more: false,
        operations: [
          {
            op_id: "hist-1",
            audit_id: "audit-1",
            request_id: "req-1",
            source: "panel",
            intent: "skill.release",
            status: "succeeded",
            ack: false,
            skill: "release-notes",
            target: "codex",
            binding: null,
            method: "copy",
            created_at: "2026-06-12T09:00:00Z",
            updated_at: "2026-06-12T09:01:00Z",
          },
          {
            op_id: "hist-2",
            audit_id: "audit-2",
            request_id: "req-2",
            source: "registry",
            intent: "skill.monitor_observed",
            status: "succeeded",
            ack: true,
            skill: auditSkillList,
            target: null,
            binding: null,
            method: null,
            created_at: "2026-06-12T09:02:00Z",
            updated_at: "2026-06-12T09:03:00Z",
          },
        ],
      },
    });

    render(<SkillMPanel />);

    expect(screen.getByText("推送远端同步")).toBeTruthy();
    expect(screen.getByText("扫描观测目录")).toBeTruthy();
    expect(screen.getByText("4 个 skill")).toBeTruthy();
    expect(screen.queryByText("aiproxy-workflow-auth-debug, ask-claude, ask-gemini, code-review")).toBeNull();
    expect(screen.queryByText("skill.save")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: /审计历史/ }));

    expect(await screen.findByText("发布版本标签")).toBeTruthy();
    expect(screen.getByText("release-notes")).toBeTruthy();
    expect(screen.getByText("5 个 skill")).toBeTruthy();
    expect(screen.getByText(/本次批量操作包含 5 个 skill/)).toBeTruthy();
    expect(screen.queryByText(auditSkillList)).toBeNull();
    expect(ops.mock.calls[0]?.[0]).toEqual({ limit: 100, offset: 0 });
    expect(new URL(window.location.href).searchParams.get("view")).toBe("history");
  });
});
