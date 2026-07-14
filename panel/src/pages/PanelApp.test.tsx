import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { PanelApp } from "./PanelApp";
import { errorResponse, jsonResponse } from "./test_utils";
import { ZERO_OPERATION_COUNTS, type OperationCounts } from "../types";
import type { RegistryOperationRecord } from "../types";

const fetchMock = vi.fn<typeof fetch>();

interface FetchMockOptions {
  skillsWarnings?: string[];
  pendingCount?: number;
  pendingWarnings?: string[];
  opsWarnings?: string[];
  diagnoseConflict?: boolean;
  operationCounts?: OperationCounts;
  pendingOps?: RegistryOperationRecord[];
  omitOperationCounts?: boolean;
}

function installFetchMock(failingPath: string | null = null, failingResponse?: Response, options: FetchMockOptions = {}) {
  const operationCounts = options.operationCounts ?? {
    ...ZERO_OPERATION_COUNTS,
    actionable_operations: options.pendingCount ?? 0,
  };
  fetchMock.mockImplementation((input: RequestInfo | URL) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    const failedResponse = url === failingPath ? failingResponse : undefined;
    switch (url) {
      case "/api/v1/health":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "panel.health",
            request_id: "req-health",
            data: { service: "loom-panel" },
            error: null,
            meta: { warnings: [] },
          }),
        );
      case "/api/v1/workspace/info":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "panel.info",
            request_id: "req-info",
            data: { root: "/tmp/loom-registry" },
            error: null,
            meta: { warnings: [] },
          }),
        );
      case "/api/v1/workspace/status":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "workspace.status",
            request_id: "req-status",
            data: { registry: { counts: {} } },
            error: null,
            meta: { warnings: [] },
          }),
        );
      case "/api/v1/skills":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "registry.skills",
            request_id: "req-skills",
            data: {
              skills: [
                {
                  skill_id: "typed-api-client",
                  source_status: "present",
                  bindings_count: 0,
                  projections_count: 0,
                  target_ids: [],
                  release_tags: [],
                  snapshot_tags: [],
                },
              ],
            },
            error: null,
            meta: { warnings: options.skillsWarnings ?? [] },
          }),
        );
      case "/api/v1/skills/typed-api-client/history":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            data: { skill: "typed-api-client", count: 0, events: [] },
          }),
        );
      case "/api/v1/skills/typed-api-client/inspect":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "skill.inspect",
            request_id: "req-inspect",
            data: {
              skill: "typed-api-client",
              source: {
                path: "/tmp/loom-registry/skills/typed-api-client",
                exists: true,
                entrypoint: "SKILL.md",
                entrypoint_exists: true,
                working_tree_drift: false,
                head_tree_oid: "tree123",
                last_source_commit: "abc12345",
                drifted_paths: [],
              },
              spec: { portable: "pass", codex: "pass", claude: "pass", findings: [] },
              provenance: {},
              runtime: {},
              dependencies: null,
              quality: {
                last_eval: null,
                trigger_precision: null,
                trigger_recall: null,
                baseline_delta: null,
              },
              safety: {
                trust: "unknown",
                policy: "unknown",
                scripts_present: null,
                network_requested: null,
                quarantined: false,
                reason: null,
                updated_at: null,
              },
              next_actions: ["loom skill eval typed-api-client"],
            },
            error: null,
            meta: { warnings: [] },
          }),
        );
      case "/api/v1/registry/status":
        return Promise.resolve(
          failedResponse
            ? failedResponse
            : jsonResponse({ ok: true, data: { counts: {}, projections: [], rules: [], targets: [], bindings: [] } }),
        );
      case "/api/v1/sync/status":
        return Promise.resolve(
          failedResponse
            ? failedResponse
            : jsonResponse({
                ok: true,
                cmd: "sync.status",
                request_id: "req-sync",
                data: { remote: { sync_state: "CLEAN", operation_counts: operationCounts }, warnings: [] },
                error: null,
                meta: { warnings: [] },
              }),
        );
      case "/api/v1/ops/pending":
        return Promise.resolve(
          failedResponse
            ? failedResponse
            : jsonResponse({
                ok: true,
                cmd: "pending.list",
                request_id: "req-pending",
                data: {
                  count: options.pendingCount ?? 0,
                  ops: options.pendingOps ?? [],
                  ...(!options.omitOperationCounts && { operation_counts: operationCounts }),
                  warnings: options.pendingWarnings ?? [],
                },
                error: null,
                meta: { warnings: [] },
              }),
        );
      case "/api/v1/ops/diagnose":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            data: {
              local_branch: true,
              remote_tracking: true,
              ahead: 0,
              behind: 0,
              local_segments: 1,
              local_archives: 0,
              remote_segments: 1,
              remote_archives: 0,
              local_snapshot: true,
              remote_snapshot: true,
              compact_after_segments: 8,
              retain_recent_segments: 4,
              retain_archives: 4,
              conflicts: options.diagnoseConflict
                ? [
                    {
                      scope: "segment",
                      path: "registry_ops_history/conflict.jsonl",
                      local_blob: "local",
                      remote_blob: "remote",
                      local_rename_path: "registry_ops_history/conflict-local.jsonl",
                      remote_rename_path: "registry_ops_history/conflict-remote.jsonl",
                    },
                  ]
                : [],
            },
          }),
        );
      case "/api/v1/ops?limit=30":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "registry.ops",
            request_id: "req-ops",
            data: { count: 0, loaded_count: 0, offset: 0, limit: 30, has_more: false, operations: [] },
            error: null,
            meta: { warnings: options.opsWarnings ?? [] },
          }),
        );
      case "/api/v1/ops?limit=100&offset=0":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "registry.ops",
            request_id: "req-history",
            data: { count: 0, loaded_count: 0, offset: 0, limit: 100, has_more: false, operations: [] },
            error: null,
            meta: { warnings: [] },
          }),
        );
      case "/api/v1/sync/replay":
        return Promise.resolve(
          jsonResponse({
            ok: true,
            cmd: "sync.replay",
            request_id: "req-replay",
            data: { replayed: options.pendingCount ?? 0 },
            error: null,
            meta: { warnings: [] },
          }),
        );
      default:
        return Promise.reject(new Error(`unexpected fetch ${url}`));
    }
  });
}

function installSuccessfulFetchMock() {
  installFetchMock();
}

describe("PanelApp status failure UI", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
    fetchMock.mockReset();
    localStorage.clear();
    window.history.replaceState(null, "", "/");
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("shows registry error state and offline banner when /api/v1/registry/status fails", async () => {
    installFetchMock(
      "/api/v1/registry/status",
      {
        ok: false,
        status: 503,
        statusText: "Service Unavailable",
        json: vi.fn().mockRejectedValue(new SyntaxError("Unexpected token < in JSON at position 0")),
      } as unknown as Response,
    );

    render(<PanelApp />);

    expect(screen.getByText(/Fetching live registry state from/i)).toBeTruthy();

    await waitFor(() => {
      expect(screen.getByText(/registry error/i)).toBeTruthy();
    });

    expect(screen.getByText(/GET \/api\/v1\/registry\/status returned 503/i)).toBeTruthy();
  });

  it("shows registry error state when /api/v1/sync/status returns a structured backend failure", async () => {
    installFetchMock(
      "/api/v1/sync/status",
      errorResponse(500, {
        ok: false,
        error: { code: "IO_ERROR", message: "failed to read operations.jsonl" },
      }),
    );

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/registry error/i)).toBeTruthy();
    });

    expect(screen.getByText(/failed to read operations\.jsonl/i)).toBeTruthy();
  });

  it("shows registry error state when /api/v1/ops/pending returns a structured backend failure", async () => {
    installFetchMock(
      "/api/v1/ops/pending",
      errorResponse(500, {
        ok: false,
        error: { code: "IO_ERROR", message: "failed to read operation backlog" },
      }),
    );

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/registry error/i)).toBeTruthy();
    });

    expect(screen.getByText(/failed to read operation backlog/i)).toBeTruthy();
  });

  it("fails visibly when the pending response omits canonical operation counts", async () => {
    installFetchMock(null, undefined, { omitOperationCounts: true });

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/registry error/i)).toBeTruthy();
    });
    expect(screen.getByText(/missing operation_counts/i)).toBeTruthy();
  });

  it("keeps a succeeded actionable pending row visible and replayable", async () => {
    localStorage.setItem("loom.page", "ops");
    installFetchMock(null, undefined, {
      pendingCount: 1,
      pendingOps: [{
        op_id: "op-succeeded-unacked",
        intent: "skill.commit",
        status: "succeeded",
        ack: false,
        skill: "demo",
        created_at: "2026-07-14T00:00:00Z",
        updated_at: "2026-07-14T00:00:00Z",
      }],
    });

    render(<PanelApp />);

    const retry = (await screen.findByRole("button", { name: /Retry replayable \(1\)/i })) as HTMLButtonElement;
    expect(retry.disabled).toBe(false);
    expect(screen.getByTitle("source command: skill.commit")).toBeInTheDocument();
    expect(screen.getAllByText(/pending/i).length).toBeGreaterThan(0);
  });

  it("shows backend warnings returned by panel read paths", async () => {
    installFetchMock(undefined, undefined, {
      skillsWarnings: ["skipped malformed skill metadata"],
      pendingWarnings: ["operation backlog had parse warnings"],
      opsWarnings: ["ignored malformed operation audit row"],
    });

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/Backend warnings/i)).toBeTruthy();
    });

    expect(screen.getByText(/skipped malformed skill metadata/i)).toBeTruthy();
    expect(screen.getByText(/operation backlog had parse warnings/i)).toBeTruthy();
    expect(screen.getByText(/ignored malformed operation audit row/i)).toBeTruthy();
  });

  it("disables history repair while queued writes exist", async () => {
    localStorage.setItem("loom.page", "history");
    installFetchMock(null, undefined, {
      pendingCount: 2,
      diagnoseConflict: true,
    });

    render(<PanelApp />);
    fireEvent.click(await screen.findByRole("button", { name: /Audit log/i }));

    const repairLocal = (await screen.findByRole("button", {
      name: /Repair from local/i,
    })) as HTMLButtonElement;
    const repairRemote = (await screen.findByRole("button", {
      name: /Repair from remote/i,
    })) as HTMLButtonElement;
    await waitFor(() => {
      expect(repairLocal.disabled).toBe(true);
      expect(repairRemote.disabled).toBe(true);
    });
    expect(repairLocal.title).toBe("operation backlog must be replayed or purged first");
    expect(repairRemote.title).toBe("operation backlog must be replayed or purged first");
  });

  it("shows first-run mode when workspace status reports missing registry state", async () => {
    fetchMock.mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
      switch (url) {
        case "/api/v1/health":
          return Promise.resolve(
            jsonResponse({
              ok: true,
              cmd: "panel.health",
              request_id: "req-health",
              data: { service: "loom-panel" },
              error: null,
              meta: { warnings: [] },
            }),
          );
        case "/api/v1/workspace/info":
          return Promise.resolve(
            jsonResponse({
              ok: true,
              cmd: "panel.info",
              request_id: "req-info",
              data: { root: "/tmp/loom-registry" },
              error: null,
              meta: { warnings: [] },
            }),
          );
        case "/api/v1/workspace/status":
          return Promise.resolve(
            jsonResponse({
              ok: true,
              cmd: "workspace.status",
              request_id: "req-status",
              data: {
                registry: {
                  available: false,
                  error: { code: "ARG_INVALID", message: "registry state not initialized" },
                },
              },
              error: null,
              meta: { warnings: [] },
            }),
          );
        default:
          return Promise.reject(new Error(`unexpected fetch ${url}`));
      }
    });

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/Initialize Registry/i)).toBeTruthy();
    });
    expect(screen.getByText(/Scan existing agent skill directories/i)).toBeTruthy();
  });

  it("opens the command palette with Ctrl+K and navigates to pages and skills", async () => {
    installSuccessfulFetchMock();

    render(<PanelApp />);

    await screen.findByRole("heading", { name: "Overview" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    let dialog = await screen.findByRole("dialog", { name: /Command palette/i });
    const skillsPageOption = within(dialog).getAllByRole("option").find((option) => {
      const text = option.textContent ?? "";
      return text.includes("Pages") && text.includes("Skillsskills");
    });
    expect(skillsPageOption).toBeTruthy();
    fireEvent.click(skillsPageOption as HTMLButtonElement);

    await screen.findByRole("heading", { name: "Skills" });
    expect(localStorage.getItem("loom.page")).toBe("skills");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    dialog = await screen.findByRole("dialog", { name: /Command palette/i });
    fireEvent.change(within(dialog).getByRole("searchbox"), { target: { value: "typed-api" } });
    fireEvent.click(within(dialog).getByText("typed-api-client").closest("button") as HTMLButtonElement);

    await screen.findByRole("heading", { name: "Skills" });
    expect(localStorage.getItem("loom.page")).toBe("skills");
    expect(window.location.hash).toBe("#/skills/typed-api-client");
  });

  it("restores the skills detail route from the URL hash", async () => {
    window.history.replaceState(null, "", "#/skills/typed-api-client");
    installSuccessfulFetchMock();

    render(<PanelApp />);

    await screen.findByRole("heading", { name: "Skills" });
    expect(localStorage.getItem("loom.page")).toBe("skills");
    expect(window.location.hash).toBe("#/skills/typed-api-client");
    expect(await screen.findByText("Runtime visibility")).toBeInTheDocument();
  });

  it("replays queued writes from the status bar through the existing sync API", async () => {
    installFetchMock(null, undefined, { pendingCount: 2 });

    render(<PanelApp />);

    const replay = (await screen.findByRole("button", { name: /queued 2/i })) as HTMLButtonElement;
    expect(replay.disabled).toBe(false);

    fireEvent.click(replay);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith("/api/v1/sync/replay", expect.anything());
    });
    expect(await screen.findByText(/Queued writes replayed/i)).toBeTruthy();
  });
});

describe("PanelApp theme initialization", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
    fetchMock.mockReset();
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.style.removeProperty("--accent");
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.style.removeProperty("--accent");
  });

  it("uses the restored GitHub theme accent when tweaks were reset", async () => {
    localStorage.setItem("loom.theme", "github");
    installSuccessfulFetchMock();

    render(<PanelApp />);

    await waitFor(() => {
      expect(document.documentElement.getAttribute("data-theme")).toBe("github");
      expect(document.documentElement.style.getPropertyValue("--accent")).toBe("#0969da");
    });
    expect(JSON.parse(localStorage.getItem("loom.tweaks") ?? "{}")).toMatchObject({ accent: "#0969da" });
  });

  it("fills a missing stored accent from the restored Warm theme", async () => {
    localStorage.setItem("loom.theme", "light");
    localStorage.setItem(
      "loom.tweaks",
      JSON.stringify({
        vizMode: "force",
        density: "dense",
        compact: true,
        hero: "graph",
        displayFont: "Inter",
      }),
    );
    installSuccessfulFetchMock();

    render(<PanelApp />);

    await waitFor(() => {
      expect(document.documentElement.getAttribute("data-theme")).toBe("light");
      expect(document.documentElement.style.getPropertyValue("--accent")).toBe("#c05f23");
    });
    expect(JSON.parse(localStorage.getItem("loom.tweaks") ?? "{}")).toMatchObject({
      accent: "#c05f23",
      displayFont: "Inter",
      vizMode: "force",
    });
  });
});
