import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError } from "./client";

describe("api.v3Status", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("surfaces the HTTP status when a failed GET returns non-JSON", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      status: 502,
      statusText: "Bad Gateway",
      json: vi.fn().mockRejectedValue(new SyntaxError("Unexpected token < in JSON at position 0")),
    } as unknown as Response);

    await expect(api.v3Status()).rejects.toEqual(
      expect.objectContaining<ApiError>({
        name: "ApiError",
        path: "/api/v3/status",
        status: 502,
        message: "GET /api/v3/status returned 502",
      }),
    );
  });
});
