import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api } from "../lib/api/client";
import { OperationLogRow } from "./OperationLogRow";
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
  window.localStorage.clear();
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
          {
            op_id: "hist-3",
            audit_id: "audit-3",
            request_id: "req-3",
            source: "registry",
            intent: "sync.replay",
            status: "enqueued",
            ack: false,
            skill: null,
            target: null,
            binding: null,
            method: null,
            created_at: "2026-06-12T09:04:00Z",
            updated_at: "2026-06-12T09:05:00Z",
          },
          {
            op_id: "hist-4",
            audit_id: "audit-4",
            request_id: "req-4",
            source: "registry",
            intent: "sync.push",
            status: "succeeded",
            ack: true,
            skill: null,
            target: null,
            binding: null,
            method: null,
            last_error: { code: "sync_failed", message: "remote rejected push" },
            created_at: "2026-06-12T09:06:00Z",
            updated_at: "2026-06-12T09:07:00Z",
          },
        ],
      },
    });

    const { container } = render(<SkillMPanel />);

    expect(screen.getByText("推送远端同步")).toBeTruthy();
    expect(screen.getByText("扫描观测目录")).toBeTruthy();
    expect(screen.getByText("4 个 skill")).toBeTruthy();
    expect(screen.queryByText("aiproxy-workflow-auth-debug, ask-claude, ask-gemini, code-review")).toBeNull();
    expect(screen.queryByText("skill.save")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: /审计历史/ }));

    expect(await screen.findByText("release-notes skill release done")).toBeTruthy();
    expect(screen.getByText("5 skills observed skill monitor done")).toBeTruthy();
    expect(screen.getByText("批量 5")).toBeTruthy();
    expect([...container.querySelectorAll(".op-row-pending .op-pill")].some((node) => node.textContent === "待处理")).toBe(true);
    expect([...container.querySelectorAll(".op-row-failed .op-pill")].some((node) => node.textContent === "失败")).toBe(true);
    expect(screen.queryByText(auditSkillList)).toBeNull();
    expect(ops.mock.calls[0]?.[0]).toEqual({ limit: 100, offset: 0 });
    expect(new URL(window.location.href).searchParams.get("view")).toBe("history");
  });

  it("keeps every skill name available when expanded bulk rows exceed the summary height", () => {
    const names = Array.from({ length: 90 }, (_, index) => `skill-${index + 1}`);

    render(
      <OperationLogRow
        op={{
          id: "bulk-all",
          kind: "skill.monitor_observed",
          skill: names.join(", "),
          target: "target_codex_home",
          status: "pending",
          time: "now",
          method: "—",
        }}
      />,
    );

    expect(screen.getByText("skill-1")).toBeTruthy();
    expect(screen.getByText("skill-90")).toBeTruthy();
    expect(screen.queryByText(/\+10 more/)).toBeNull();
  });

  it("keeps skill details visible while browsing many skill cards", async () => {
    panelData.current = {
      ...panelData.liveOps,
      ops: [],
      skills: [
        {
          id: "alpha",
          name: "alpha-skill",
          description: "Alpha description",
          tag: "workflow",
          sourceStatus: "present",
          releaseTags: [],
          snapshotTags: [],
          latestRev: "rev-alpha",
          ruleCount: 0,
          bindingCount: 1,
          projectionCount: 2,
          changed: "1h ago",
          targets: [],
        },
        {
          id: "beta",
          name: "beta-skill",
          description: "Beta description",
          tag: "debug",
          sourceStatus: "missing",
          releaseTags: [],
          snapshotTags: [],
          latestRev: "rev-beta",
          ruleCount: 0,
          bindingCount: 0,
          projectionCount: 0,
          changed: "2h ago",
          targets: [],
        },
      ],
      targets: [],
    };
    window.history.replaceState(null, "", "/?view=skills");

    render(<SkillMPanel />);

    expect(screen.getByLabelText("alpha-skill detail")).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "查看 beta-skill 详情" }));

    const detail = screen.getByLabelText("beta-skill detail");
    expect(detail).toBeTruthy();
    expect(within(detail).getByText("Beta description")).toBeTruthy();
  });

  it("uses the current panel host instead of a hard-coded dev port", () => {
    panelData.current = panelData.liveOps;
    window.history.replaceState(null, "", "/?view=targets");

    render(<SkillMPanel />);

    expect(screen.getByTitle("当前 Panel 地址")).toBeTruthy();
    expect(screen.queryByText("localhost:5173")).toBeNull();
  });

  it("discloses projection graph truncation and scopes the table to the same page", async () => {
    const projections = Array.from({ length: 14 }, (_, index) => ({
      instance_id: `projection-${index + 1}`,
      skill_id: `skill-${index + 1}`,
      binding_id: `binding-${index + 1}`,
      target_id: `target_${index + 1}`,
      materialized_path: `/tmp/target-${index + 1}/skill-${index + 1}`,
      method: "copy",
      last_applied_rev: `rev-${index + 1}`.padEnd(8, "x"),
      health: "ok",
      observed_drift: false,
    }));
    panelData.current = {
      ...panelData.liveOps,
      ops: [],
      skills: projections.map((projection) => ({
        id: projection.skill_id,
        name: projection.skill_id,
        description: "Projection fixture",
        tag: "workflow",
        sourceStatus: "present",
        releaseTags: [],
        snapshotTags: [],
        latestRev: projection.last_applied_rev,
        ruleCount: 0,
        bindingCount: 1,
        projectionCount: 1,
        changed: "now",
        targets: [projection.target_id],
      })),
      targets: projections.map((projection, index) => ({
        id: projection.target_id,
        agent: "codex",
        path: `/tmp/target-${index + 1}`,
        profile: "default",
        ownership: "managed",
        skills: 1,
        projectedSkills: 1,
        lastSync: "now",
      })),
      projections,
    };
    window.history.replaceState(null, "", "/?view=projections");

    render(<SkillMPanel />);

    const table = screen.getByRole("table");
    expect(screen.getByText("displaying 12 of 14 skills")).toBeTruthy();
    expect(screen.getByText("displaying 12 of 14 targets")).toBeTruthy();
    expect(screen.getByText("displaying 12 of 14 projections")).toBeTruthy();
    expect(within(table).queryByText("skill-14")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Next projection page" }));

    expect(screen.getByText("displaying 2 of 14 skills")).toBeTruthy();
    expect(screen.getByText("displaying 2 of 14 targets")).toBeTruthy();
    expect(screen.getByText("displaying 2 of 14 projections")).toBeTruthy();
    expect(within(table).getByText("skill-14")).toBeTruthy();
  });

  it("summarizes Git sync events without exposing raw bulk skill lists", () => {
    panelData.current = panelData.liveOps;
    window.history.replaceState(null, "", "/?view=sync");

    render(<SkillMPanel />);

    expect(screen.getByText("推送远端同步")).toBeTruthy();
    expect(screen.queryByText("扫描观测目录")).toBeNull();
    expect(screen.queryByText("4 个 skill")).toBeNull();
    expect(screen.queryByText("aiproxy-workflow-auth-debug, ask-claude, ask-gemini, code-review")).toBeNull();
    expect(screen.queryByText("skill.monitor_observed")).toBeNull();
  });

  it("routes the footer sync control to Git sync instead of replaying immediately", async () => {
    panelData.current = panelData.liveOps;
    window.history.replaceState(null, "", "/");
    const syncReplay = vi.spyOn(api, "syncReplay");

    render(<SkillMPanel />);

    await userEvent.click(screen.getByRole("button", { name: /local/ }));

    expect(screen.getByRole("heading", { name: "注册表同步" })).toBeTruthy();
    expect(syncReplay).not.toHaveBeenCalled();
  });

  it("does not underreport queued operations when the API count exceeds visible rows", () => {
    panelData.current = { ...panelData.liveOps, queuedWriteCount: 5 };
    window.history.replaceState(null, "", "/?view=ops");

    const { container } = render(<SkillMPanel />);
    const pendingStat = [...container.querySelectorAll(".pstat")].find((node) => node.textContent?.includes("待处理"));

    expect(pendingStat?.textContent).toContain("5");
    expect(screen.getByText(/5 queued/)).toBeTruthy();
  });

  it("makes non-wired controls read as status and gives overlays real controls", async () => {
    panelData.current = {
      ...panelData.liveOps,
      skills: [
        {
          id: "alpha",
          name: "alpha-skill",
          description: "Alpha description",
          tag: "workflow",
          sourceStatus: "present",
          releaseTags: [],
          snapshotTags: [],
          latestRev: "rev-alpha",
          ruleCount: 0,
          bindingCount: 1,
          projectionCount: 2,
          changed: "1h ago",
          targets: [],
        },
        {
          id: "beta",
          name: "beta-skill",
          description: "Beta description",
          tag: "debug",
          sourceStatus: "present",
          releaseTags: [],
          snapshotTags: [],
          latestRev: "rev-beta",
          ruleCount: 0,
          bindingCount: 0,
          projectionCount: 0,
          changed: "2h ago",
          targets: [],
        },
      ],
      targets: [{ id: "target_codex", agent: "codex", path: "~/.codex/skills", profile: "default", ownership: "observed", projectedSkills: 2 }],
      bindings: [{ id: "bind-alpha", skill: "alpha-skill", policy: "codex", matcher: "tag:workflow", target: "target_codex", method: "copy" }],
    };
    window.history.replaceState(null, "", "/?view=targets");

    render(<SkillMPanel />);

    expect(screen.queryByRole("button", { name: /target add/i })).toBeNull();
    expect(screen.getByText("target 新增未接入")).toBeTruthy();
    expect(screen.getByText("verify 未接入")).toBeTruthy();

    await userEvent.keyboard("{Control>}k{/Control}");
    const search = await screen.findByRole("textbox", { name: "搜索命令" });
    await userEvent.type(search, "beta");

    expect(screen.getByText("Open beta-skill")).toBeTruthy();
    expect(screen.queryByText("Open alpha-skill")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "关闭命令面板" }));
    await userEvent.click(screen.getByRole("button", { name: /Settings/ }));

    expect(screen.getByRole("switch", { name: "切换深色模式" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "选择配色 1" })).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "tweaks" }));

    expect(screen.getByRole("button", { name: "关闭 Tweaks" })).toBeTruthy();
  });

  it("opens an explicit Ops purge confirmation before dispatching", async () => {
    panelData.current = panelData.liveOps;
    window.history.replaceState(null, "", "/?view=ops");
    const purge = vi.spyOn(api, "opsPurge").mockResolvedValue({ ok: true, cmd: "ops.purge", request_id: "req-purge" });
    const nativeConfirm = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<SkillMPanel />);

    await userEvent.click(screen.getByRole("button", { name: /purge/ }));

    expect(nativeConfirm).not.toHaveBeenCalled();
    expect(purge).not.toHaveBeenCalled();

    const dialog = screen.getByRole("dialog", { name: "清理 Ops 队列？" });
    expect(within(dialog).getByText("Affected scope")).toBeTruthy();
    expect(within(dialog).getByText(/不可自动撤销/)).toBeTruthy();
    expect(within(dialog).getByText(/Ops purge API/)).toBeTruthy();

    await userEvent.click(within(dialog).getByRole("button", { name: "确认清理" }));

    await waitFor(() => expect(purge).toHaveBeenCalledTimes(1));
    expect(panelData.refetch).toHaveBeenCalledTimes(1);
  });

  it("shows queued count and defers Ops replay until the sheet confirm action", async () => {
    panelData.current = { ...panelData.liveOps, queuedWriteCount: 5 };
    window.history.replaceState(null, "", "/?view=ops");
    const retry = vi.spyOn(api, "opsRetry").mockResolvedValue({ ok: true, cmd: "ops.retry", request_id: "req-retry" });

    render(<SkillMPanel />);

    await userEvent.click(screen.getByRole("button", { name: /replay 队列/ }));

    expect(retry).not.toHaveBeenCalled();

    const dialog = screen.getByRole("dialog", { name: "重放 Ops 队列？" });
    expect(within(dialog).getByText("Queued count")).toBeTruthy();
    expect(within(dialog).getByText("5")).toBeTruthy();
    expect(within(dialog).getByText(/重试 pending\/failed 操作/)).toBeTruthy();

    await userEvent.click(within(dialog).getByRole("button", { name: "确认重放" }));

    await waitFor(() => expect(retry).toHaveBeenCalledTimes(1));
  });

  it("marks Market and Forge as preview before navigation", async () => {
    panelData.current = panelData.liveOps;
    render(<SkillMPanel />);

    expect(screen.getByRole("button", { name: /Market Preview/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Forge Preview/ })).toBeTruthy();

    await userEvent.keyboard("{Control>}k{/Control}");

    expect(await screen.findByRole("button", { name: /Go to Market Preview not connected/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Go to Forge Preview not connected/ })).toBeTruthy();
  });

  it("explains Market and Forge placeholders without fake install or create controls", async () => {
    panelData.current = panelData.liveOps;
    render(<SkillMPanel />);

    await userEvent.click(screen.getByRole("button", { name: /Market Preview/ }));

    expect(screen.getByRole("heading", { name: "市场" })).toBeTruthy();
    expect(screen.getByText("Preview · not connected")).toBeTruthy();
    expect(screen.getByText(/只读查看本地 registry/)).toBeTruthy();
    expect(screen.getByText(/不展示安装按钮/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /install|安装/i })).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: /Forge Preview/ }));

    expect(screen.getByRole("heading", { name: "Forge" })).toBeTruthy();
    expect(screen.getByText(/只读参考本地 registry/)).toBeTruthy();
    expect(screen.getByText(/不展示创建按钮/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /create|创建/i })).toBeNull();
  });
});
