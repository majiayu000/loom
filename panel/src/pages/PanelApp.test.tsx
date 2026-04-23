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

  it("shows registry error state and mock banner when /api/v3/status fails", async () => {
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
          return Promise.resolve({
            ok: false,
            status: 503,
            statusText: "Service Unavailable",
            json: vi.fn().mockRejectedValue(new SyntaxError("Unexpected token < in JSON at position 0")),
          } as unknown as Response);
        case "/api/remote/status":
          return Promise.resolve(jsonResponse({ remote: { sync_state: "CLEAN" } }));
        case "/api/pending":
          return Promise.resolve(jsonResponse({ count: 0, ops: [] }));
        default:
          return Promise.reject(new Error(`unexpected fetch ${url}`));
      }
    });

    render(<PanelApp />);

    expect(screen.getByText(/Fetching live registry state from/i)).toBeTruthy();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /registry error/i })).toBeTruthy();
    });

    expect(screen.getByText(/Registry state unavailable — GET \/api\/v3\/status returned 503\./i)).toBeTruthy();
    expect(screen.getByText(/mock data/i)).toBeTruthy();
    expect(screen.getByText(/projected across 6 agent targets\./i)).toBeTruthy();

    const replayButton = screen.getByRole("button", { name: /^Replay$/i }) as HTMLButtonElement;
    expect(replayButton.disabled).toBe(true);
  });
});
