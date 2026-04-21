import { afterEach, describe, expect, it, mock } from "bun:test";
import { ApiError } from "./src/lib/api/client";

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe("api POST failure messages", () => {
  it("prefers HTTP status text over JSON parse errors for non-JSON failure bodies", async () => {
    globalThis.fetch = mock(async () =>
      new Response("<html>bad gateway</html>", {
        status: 502,
        statusText: "Bad Gateway",
        headers: { "Content-Type": "text/html" },
      }),
    ) as typeof fetch;

    const { api } = await import("./src/lib/api/client");

    await expect(api.syncPull()).rejects.toEqual(
      expect.objectContaining<ApiError>({
        name: "ApiError",
        path: "/api/sync/pull",
        status: 502,
        message: "POST /api/sync/pull returned 502 Bad Gateway",
      }),
    );
  });
});
