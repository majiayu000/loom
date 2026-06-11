import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { PanelApp } from "./PanelApp";

const fetchMock = vi.fn<typeof fetch>();

function jsonResponse(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: vi.fn().mockResolvedValue(body),
  } as unknown as Response;
}

function errorResponse(status: number, body: unknown): Response {
  return {
    ok: false,
    status,
    statusText: "Service Unavailable",
    json: vi.fn().mockResolvedValue(body),
  } as unknown as Response;
}

function installFetchMock(failingPath: string | null = null, failingResponse?: Response) {
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
            meta: { warnings: [] },
          }),
        );
      case "/api/v1/registry/status":
        return Promise.resolve(
          url === failingPath && failingResponse
            ? failingResponse
            : jsonResponse({ ok: true, data: { counts: {}, projections: [], rules: [], targets: [], bindings: [] } }),
        );
      case "/api/v1/sync/status":
        return Promise.resolve(
          url === failingPath && failingResponse
            ? failingResponse
            : jsonResponse({
                ok: true,
                cmd: "sync.status",
                request_id: "req-sync",
                data: { remote: { sync_state: "CLEAN" }, warnings: [] },
                error: null,
                meta: { warnings: [] },
              }),
        );
      case "/api/v1/ops/pending":
        return Promise.resolve(
          url === failingPath && failingResponse
            ? failingResponse
            : jsonResponse({
                ok: true,
                cmd: "pending.list",
                request_id: "req-pending",
                data: { count: 0, ops: [] },
                error: null,
                meta: { warnings: [] },
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
        error: { code: "IO_ERROR", message: "failed to read pending_ops.jsonl" },
      }),
    );

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/registry error/i)).toBeTruthy();
    });

    expect(screen.getByText(/failed to read pending_ops\.jsonl/i)).toBeTruthy();
  });

  it("shows registry error state when /api/v1/ops/pending returns a structured backend failure", async () => {
    installFetchMock(
      "/api/v1/ops/pending",
      errorResponse(500, {
        ok: false,
        error: { code: "IO_ERROR", message: "failed to read pending queue" },
      }),
    );

    render(<PanelApp />);

    await waitFor(() => {
      expect(screen.getByText(/registry error/i)).toBeTruthy();
    });

    expect(screen.getByText(/failed to read pending queue/i)).toBeTruthy();
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
