import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, api } from "../lib/api/client";
import { OperationLogRow } from "./OperationLogRow";
import { SkillMPanel } from "./SkillMPanel";

const panelData = vi.hoisted(() => ({
  refetch: vi.fn(),
  current: null as null | {
    live: boolean;
    apiReachable: boolean;
    loading: boolean;
    error: string | null;
    mode: "loading" | "live" | "first-run" | "offline-empty" | "offline-stale";
    setupRequired: boolean;
    lastUpdated: string | null;
    registryRoot: string | null;
    agentDirs: unknown[];
    remote: null | {
      configured: boolean;
      url: string | null;
      remote?: string | null;
      sync_state?: string;
      operation_backlog?: number;
    };
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
      actionable?: boolean;
      time: string;
      reason?: string;
      method?: string;
    }>;
    projections: unknown[];
    operationCounts: null | {
      actionable_operations: number;
      local_journal_events: number;
      unpushed_history_events: number;
      local_only_history_events: number;
    };
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
    operationCounts: null,
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
        kind: "skill.commit",
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
        actionable: true,
        time: "2026-06-12 09:05",
        reason: "queued",
      },
      {
        id: "op-bulk",
        kind: "skill.monitor_observed",
        skill: "aiproxy-workflow-auth-debug, ask-claude, ask-gemini, code-review",
        target: "target_codex_home",
        status: "pending" as const,
        actionable: true,
        time: "2026-06-12 09:06",
        method: "—",
      },
    ],
    projections: [],
    operationCounts: {
      actionable_operations: 2,
      local_journal_events: 3,
      unpushed_history_events: 4,
      local_only_history_events: 5,
    },
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
    expect(screen.queryByText("skill.commit")).toBeNull();
    expect(screen.getByText("可执行操作")).toBeTruthy();
    expect(screen.getByText("本地 journal")).toBeTruthy();
    expect(screen.getByText("待推送 history")).toBeTruthy();
    expect(screen.getByText("仅本地 history")).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: /审计历史/ }));

    expect(await screen.findByText("release-notes skill release pending")).toBeTruthy();
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

  it("starts in truthful loading state before the first registry response", () => {
    panelData.current = {
      ...panelData.liveOps,
      live: false,
      apiReachable: false,
      loading: true,
      mode: "loading",
      lastUpdated: null,
      skills: [],
    };

    render(<SkillMPanel />);

    expect(screen.getByText(/正在连接注册表/)).toBeTruthy();
    expect(screen.queryByText("API offline")).toBeNull();
  });

  it("bounds the initial skill card render while filtering the full inventory", async () => {
    const skills = Array.from({ length: 120 }, (_, index) => {
      const name = `skill-${String(index + 1).padStart(3, "0")}`;
      return {
        id: name,
        name,
        description: `Description for ${name}`,
        tag: index % 2 ? "workflow" : "debug",
        sourceStatus: "present" as const,
        releaseTags: [],
        snapshotTags: [],
        latestRev: `rev-${index}`,
        ruleCount: 0,
        bindingCount: 0,
        projectionCount: 0,
        changed: "now",
        targets: [],
      };
    });
    panelData.current = { ...panelData.liveOps, ops: [], skills, targets: [] };
    window.history.replaceState(null, "", "/?view=skills");

    render(<SkillMPanel />);

    expect(document.querySelectorAll(".skill-card").length).toBeLessThanOrEqual(24);
    expect(screen.getByText("Page 1 of 5 · showing 1-24 of 120")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Next skill page" }));
    expect(screen.getByRole("button", { name: "查看 skill-025 详情" })).toBeTruthy();

    const search = screen.getByRole("searchbox", { name: "Search skills" });
    await userEvent.clear(search);
    await userEvent.type(search, "skill-120");
    expect(screen.getByRole("button", { name: "查看 skill-120 详情" })).toBeTruthy();
    expect(screen.queryByText("Page 1 of 1 · showing 1-1 of 1")).toBeNull();
  });

  it("keeps the selected skill detail stable when a filter hides its card", async () => {
    const skills = ["skill-001", "skill-002", "skill-120"].map((name) => ({
      id: name,
      name,
      description: `Description for ${name}`,
      tag: "workflow",
      sourceStatus: "present" as const,
      releaseTags: [],
      snapshotTags: [],
      latestRev: "rev",
      ruleCount: 0,
      bindingCount: 0,
      projectionCount: 0,
      changed: "now",
      targets: [],
    }));
    panelData.current = { ...panelData.liveOps, ops: [], skills, targets: [] };
    window.history.replaceState(null, "", "/?view=skills");

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "查看 skill-002 详情" }));
    expect(screen.getByLabelText("skill-002 detail")).toBeTruthy();

    await userEvent.type(screen.getByRole("searchbox", { name: "Search skills" }), "skill-120");

    expect(screen.queryByRole("button", { name: "查看 skill-002 详情" })).toBeNull();
    expect(screen.getByLabelText("skill-002 detail")).toBeTruthy();
  });

  it("uses observed-only copy for imported inventory rows", () => {
    const skill = {
      id: "observed-skill",
      name: "observed-skill",
      description: null,
      tag: "observed",
      sourceStatus: "missing" as const,
      observedImported: true,
      releaseTags: [],
      snapshotTags: [],
      latestRev: "unknown",
      ruleCount: 0,
      bindingCount: 0,
      projectionCount: 0,
      changed: "observed now",
      targets: [],
    };
    panelData.current = { ...panelData.liveOps, ops: [], skills: [skill], targets: [] };
    window.history.replaceState(null, "", "/?view=skills");

    render(<SkillMPanel />);

    const card = screen.getByRole("button", { name: "查看 observed-skill 详情" });
    expect(within(card).getByText("observed-only")).toBeTruthy();
    expect(within(card).queryByText("missing")).toBeNull();
    expect(within(card).getByText("No description observed in the registry.")).toBeTruthy();
  });

  it("imports a skill from the keyboard-accessible form and focuses the selected row", async () => {
    const importedName = "zz-imported-skill";
    const skills = [...Array.from({ length: 25 }, (_, index) => `skill-${String(index + 1).padStart(3, "0")}`), importedName].map((name) => ({
      id: name,
      name,
      description: `${name} description`,
      tag: "workflow",
      sourceStatus: "present" as const,
      releaseTags: [],
      snapshotTags: [],
      latestRev: "rev",
      ruleCount: 0,
      bindingCount: 0,
      projectionCount: 0,
      changed: "now",
      targets: [],
    }));
    panelData.current = { ...panelData.liveOps, ops: [], skills, targets: [] };
    window.history.replaceState(null, "", "/?view=skills");
    const skillAdd = vi.spyOn(api, "skillAdd").mockResolvedValue({ ok: true, cmd: "skill.add", request_id: "req-import" });

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "Import skill" }));
    await userEvent.type(screen.getByLabelText("Source"), "/tmp/zz-imported-skill");
    await userEvent.type(screen.getByLabelText("Skill name"), importedName);
    await userEvent.click(within(screen.getByRole("form", { name: "Import skill" })).getByRole("button", { name: "Import skill" }));

    await waitFor(() => expect(skillAdd).toHaveBeenCalledWith({ source: "/tmp/zz-imported-skill", name: importedName }));
    await waitFor(() => expect(screen.getByLabelText(`${importedName} detail`)).toBeTruthy());
    expect(screen.getByText("Page 2 of 2 · showing 25-26 of 26")).toBeTruthy();
    expect(screen.getByRole("button", { name: `查看 ${importedName} 详情` })).toHaveFocus();
    expect(panelData.refetch).toHaveBeenCalled();
  });

  it("matches the backend skill-name contract instead of rejecting valid leading punctuation", async () => {
    panelData.current = { ...panelData.liveOps, ops: [], skills: [], targets: [] };
    window.history.replaceState(null, "", "/?view=skills");
    const skillAdd = vi.spyOn(api, "skillAdd").mockResolvedValue({ ok: true, cmd: "skill.add", request_id: "req-import" });

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "Import skill" }));
    await userEvent.type(screen.getByLabelText("Source"), "/tmp/private-skill");
    const name = screen.getByLabelText("Skill name");
    await userEvent.type(name, ".");
    await userEvent.click(within(screen.getByRole("form", { name: "Import skill" })).getByRole("button", { name: "Import skill" }));

    expect(screen.getByText("Skill name cannot be '.' or '..'.")).toBeTruthy();
    expect(skillAdd).not.toHaveBeenCalled();

    await userEvent.clear(name);
    await userEvent.type(name, "_private-skill");
    await userEvent.click(within(screen.getByRole("form", { name: "Import skill" })).getByRole("button", { name: "Import skill" }));

    await waitFor(() => expect(skillAdd).toHaveBeenCalledWith({ source: "/tmp/private-skill", name: "_private-skill" }));
  });

  it("reveals and focuses an imported skill even when the active filters would hide it", async () => {
    const skills = [
      {
        id: "alpha-skill",
        name: "alpha-skill",
        description: "Alpha description",
        tag: "workflow",
        sourceStatus: "missing" as const,
        releaseTags: [],
        snapshotTags: [],
        latestRev: "rev-alpha",
        ruleCount: 0,
        bindingCount: 0,
        projectionCount: 0,
        changed: "now",
        targets: [],
      },
      {
        id: "imported-skill",
        name: "imported-skill",
        description: "Imported description",
        tag: "debug",
        sourceStatus: "present" as const,
        releaseTags: [],
        snapshotTags: [],
        latestRev: "rev-imported",
        ruleCount: 0,
        bindingCount: 0,
        projectionCount: 0,
        changed: "now",
        targets: [],
      },
    ];
    panelData.current = { ...panelData.liveOps, ops: [], skills, targets: [] };
    window.history.replaceState(null, "", "/?view=skills");
    vi.spyOn(api, "skillAdd").mockResolvedValue({ ok: true, cmd: "skill.add", request_id: "req-import" });

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "missing" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Search skills" }), "alpha");
    await userEvent.click(screen.getByRole("button", { name: "Import skill" }));
    await userEvent.type(screen.getByLabelText("Source"), "/tmp/imported-skill");
    await userEvent.type(screen.getByLabelText("Skill name"), "imported-skill");
    await userEvent.click(within(screen.getByRole("form", { name: "Import skill" })).getByRole("button", { name: "Import skill" }));

    await waitFor(() => expect(screen.getByRole("searchbox", { name: "Search skills" })).toHaveValue(""));
    expect(screen.getByRole("button", { name: "全部来源" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "查看 imported-skill 详情" })).toHaveFocus();
  });

  it("keeps import fields and renders backend next actions after a failed import", async () => {
    panelData.current = {
      ...panelData.liveOps,
      ops: [],
      skills: [],
      targets: [],
    };
    window.history.replaceState(null, "", "/?view=skills");
    vi.spyOn(api, "skillAdd").mockRejectedValue(
      new ApiError("/api/v1/skills", 409, "source rejected", [{ cmd: "loom skill inspect demo", reason: "inspect source" }]),
    );

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "Import skill" }));
    const source = screen.getByLabelText("Source");
    const name = screen.getByLabelText("Skill name");
    await userEvent.type(source, "github:owner/repo//skills/demo");
    await userEvent.type(name, "demo");
    await userEvent.click(within(screen.getByRole("form", { name: "Import skill" })).getByRole("button", { name: "Import skill" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("source rejected");
    expect(alert).toHaveTextContent("Try: loom skill inspect demo - inspect source");
    expect(source).toHaveValue("github:owner/repo//skills/demo");
    expect(name).toHaveValue("demo");
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
    expect(screen.getByText("可执行操作")).toBeTruthy();
    expect(screen.getByText("本地 journal")).toBeTruthy();
    expect(screen.getByText("待推送 history")).toBeTruthy();
    expect(screen.getByText("仅本地 history")).toBeTruthy();
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

  it("uses canonical actionable operations instead of a conflicting legacy alias", () => {
    panelData.current = { ...panelData.liveOps, queuedWriteCount: 5 };
    window.history.replaceState(null, "", "/?view=ops");

    const { container } = render(<SkillMPanel />);
    const pendingStat = [...container.querySelectorAll(".pstat")].find((node) => node.textContent?.includes("可执行操作"));

    expect(pendingStat?.textContent).toContain("2");
    expect(screen.getByText(/2 queued/)).toBeTruthy();
  });

  it("keeps a succeeded actionable row in the canonical Ops queue", () => {
    panelData.current = {
      ...panelData.liveOps,
      ops: [{ ...panelData.liveOps.ops[0], id: "op-succeeded-unacked", status: "ok", actionable: true }],
      operationCounts: { actionable_operations: 1, local_journal_events: 0, unpushed_history_events: 0, local_only_history_events: 0 },
    };
    window.history.replaceState(null, "", "/?view=ops");

    render(<SkillMPanel />);

    expect(screen.getByText("op-succeeded-unacked")).toBeTruthy();
    expect(screen.queryByText(/队列已清空/)).toBeNull();
  });

  it("removes dead control-plane affordances and gives overlays real controls", async () => {
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

    expect(screen.getByRole("button", { name: "New target" })).toBeTruthy();
    expect(screen.queryByText("target 新增未接入")).toBeNull();
    expect(screen.queryByText("verify 未接入")).toBeNull();
    expect(screen.getByText("2 个投影")).toBeTruthy();
    expect(screen.getByText("同步状态未验证")).toBeTruthy();

    await userEvent.keyboard("{Control>}k{/Control}");
    const search = await screen.findByRole("textbox", { name: "搜索命令" });
    expect(search).toHaveFocus();
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

  it("creates a target from the production shell and preserves the form after a backend failure", async () => {
    panelData.current = { ...panelData.liveOps, ops: [], targets: [] };
    window.history.replaceState(null, "", "/?view=targets");
    const targetAdd = vi.spyOn(api, "targetAdd")
      .mockRejectedValueOnce(new ApiError("/api/v1/targets", 409, "target already exists", [{ cmd: "loom target list", reason: "inspect targets" }]))
      .mockResolvedValueOnce({ ok: true, cmd: "target.add", request_id: "req-target" });

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "New target" }));
    const form = screen.getByRole("form", { name: "Add target" });
    const path = within(form).getByLabelText("path");
    await userEvent.type(path, "/tmp/agent-skills");
    await userEvent.selectOptions(within(form).getByLabelText("ownership"), "managed");
    await userEvent.click(within(form).getByRole("button", { name: "target add" }));

    expect(await within(form).findByRole("alert")).toHaveTextContent("target already exists");
    expect(path).toHaveValue("/tmp/agent-skills");

    await userEvent.click(within(form).getByRole("button", { name: "target add" }));
    await waitFor(() => expect(targetAdd).toHaveBeenLastCalledWith({ agent: "claude", path: "/tmp/agent-skills", ownership: "managed" }));
    await waitFor(() => expect(screen.queryByRole("form", { name: "Add target" })).toBeNull());
    expect(panelData.refetch).toHaveBeenCalledTimes(1);
  });

  it("creates a binding and runs a reviewed projection through the real APIs", async () => {
    const target = { id: "target_codex", agent: "codex", path: "/tmp/codex-skills", profile: "default", ownership: "managed", projectedSkills: 0 };
    const skill = {
      id: "demo",
      name: "demo",
      description: "Demo skill",
      tag: "workflow",
      sourceStatus: "present" as const,
      releaseTags: [],
      snapshotTags: [],
      latestRev: "rev-demo",
      ruleCount: 0,
      bindingCount: 1,
      projectionCount: 0,
      changed: "now",
      targets: [],
    };
    const binding = { id: "binding-demo", skill: "demo", policy: "auto", matcher: "path_prefix:/tmp/work", target: "target_codex", method: "copy" };
    panelData.current = { ...panelData.liveOps, ops: [], skills: [skill], targets: [target], bindings: [binding] };
    window.history.replaceState(null, "", "/?view=bindings");
    const bindingAdd = vi.spyOn(api, "bindingAdd").mockResolvedValue({ ok: true, cmd: "binding.add", request_id: "req-binding" });
    const project = vi.spyOn(api, "project").mockResolvedValue({ ok: true, cmd: "skill.project", request_id: "req-project" });

    render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "New binding" }));
    const bindingForm = screen.getByRole("form", { name: "Add binding" });
    await userEvent.type(within(bindingForm).getByLabelText("matcher value"), "/tmp/work");
    await userEvent.click(within(bindingForm).getByRole("button", { name: "binding add" }));

    await waitFor(() => expect(bindingAdd).toHaveBeenCalledWith({
      agent: "claude",
      profile: "home",
      matcher_kind: "path_prefix",
      matcher_value: "/tmp/work",
      target: "target_codex",
    }));

    await userEvent.click(screen.getByRole("button", { name: "Projections" }));
    await userEvent.click(screen.getByRole("button", { name: "Project skill" }));
    const projectForm = screen.getByRole("form", { name: "Project skill" });
    await userEvent.selectOptions(within(projectForm).getByLabelText("Projection method"), "copy");
    await userEvent.click(within(projectForm).getByRole("button", { name: "Review projection" }));

    expect(project).not.toHaveBeenCalled();
    expect(within(projectForm).getByRole("status")).toHaveTextContent("demo");
    await userEvent.click(within(projectForm).getByRole("button", { name: "Confirm projection" }));

    await waitFor(() => expect(project).toHaveBeenCalledWith({ skill: "demo", binding: "binding-demo", target: "target_codex", method: "copy" }));
    expect(panelData.refetch).toHaveBeenCalledTimes(2);
  });

  it("confirms pull and push for a configured remote and explains unavailable sync", async () => {
    panelData.current = {
      ...panelData.liveOps,
      remote: { configured: true, url: "git@github.com:team/skills.git", sync_state: "clean" },
    };
    window.history.replaceState(null, "", "/?view=sync");
    const pull = vi.spyOn(api, "syncPull").mockResolvedValue({ ok: true, cmd: "sync.pull", request_id: "req-pull" });
    const push = vi.spyOn(api, "syncPush").mockResolvedValue({ ok: true, cmd: "sync.push", request_id: "req-push" });

    const { unmount } = render(<SkillMPanel />);
    await userEvent.click(screen.getByRole("button", { name: "pull" }));
    expect(pull).not.toHaveBeenCalled();
    await userEvent.click(within(screen.getByRole("dialog", { name: "拉取远端注册表？" })).getByRole("button", { name: "确认拉取" }));
    await waitFor(() => expect(pull).toHaveBeenCalledTimes(1));

    await userEvent.click(screen.getByRole("button", { name: "push" }));
    expect(push).not.toHaveBeenCalled();
    await userEvent.click(within(screen.getByRole("dialog", { name: "推送本地注册表？" })).getByRole("button", { name: "确认推送" }));
    await waitFor(() => expect(push).toHaveBeenCalledTimes(1));
    unmount();

    panelData.current = { ...panelData.liveOps, remote: null };
    render(<SkillMPanel />);
    const unavailablePull = screen.getByRole("button", { name: "pull" });
    expect(unavailablePull).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Configure a Git remote before pulling or pushing.");
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

  it("shows canonical actionable count and defers Ops replay until confirmation", async () => {
    panelData.current = { ...panelData.liveOps, queuedWriteCount: 5 };
    window.history.replaceState(null, "", "/?view=ops");
    const retry = vi.spyOn(api, "opsRetry").mockResolvedValue({ ok: true, cmd: "ops.retry", request_id: "req-retry" });

    render(<SkillMPanel />);

    const replay = screen.getByRole("button", { name: /replay 队列/ });
    await userEvent.click(replay);

    expect(retry).not.toHaveBeenCalled();

    const dialog = screen.getByRole("dialog", { name: "重放 Ops 队列？" });
    expect(within(dialog).getByText("Queued count")).toBeTruthy();
    expect(within(dialog).getByText("2")).toBeTruthy();
    expect(within(dialog).getByText(/重试 pending\/failed 操作/)).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "取消" })).toHaveFocus();

    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "重放 Ops 队列？" })).toBeNull();
    expect(replay).toHaveFocus();

    await userEvent.click(replay);
    const reopenedDialog = screen.getByRole("dialog", { name: "重放 Ops 队列？" });

    await userEvent.click(within(reopenedDialog).getByRole("button", { name: "确认重放" }));

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
