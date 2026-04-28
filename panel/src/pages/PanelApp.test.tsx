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

function installFetchMock(failingPath: string, failingResponse: Response) {
  fetchMock.mockImplementation((input: RequestInfo | URL) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    switch (url) {
      case "/api/health":
        return Promise.resolve(jsonResponse({ ok: true }));
      case "/api/info":
        return Promise.resolve(jsonResponse({ root: "/tmp/loom-registry" }));
      case "/api/skills":
        return Promise.resolve(jsonResponse({ skills: ["typed-api-client"] }));
      case "/api/v3/status":
        return Promise.resolve(
          url === failingPath
            ? failingResponse
            : jsonResponse({ ok: true, data: { counts: {}, projections: [], rules: [], targets: [], bindings: [] } }),
        );
      case "/api/remote/status":
        return Promise.resolve(
          url === failingPath ? failingResponse : jsonResponse({ remote: { sync_state: "CLEAN" }, warnings: [] }),
        );
      case "/api/pending":
        return Promise.resolve(url === failingPath ? failingResponse : jsonResponse({ count: 0, ops: [] }));
      default:
        return Promise.reject(new Error(`unexpected fetch ${url}`));
    }
  });
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

  it("shows registry error state and offline banner when /api/v3/status fails", async () => {
    installFetchMock(
      "/api/v3/status",
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

    expect(screen.getByText(/GET \/api\/v3\/status returned 503/i)).toBeTruthy();
  });

  it("shows registry error state when /api/remote/status returns a structured backend failure", async () => {
    installFetchMock(
      "/api/remote/status",
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

  it("shows registry error state when /api/pending returns a structured backend failure", async () => {
    installFetchMock(
      "/api/pending",
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
});
