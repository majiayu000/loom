import { afterEach, describe, expect, it, vi } from "vitest";
import { request, requestOtp, saveConfig, signOut, verifyOtp } from "./client";

afterEach(async () => {
  await signOut();
  vi.unstubAllGlobals();
});
const config = {
  cloud_api_url: "https://cloud.example.test",
  auth_url: "https://auth.example.test",
  auth_public_key: "public-test-key",
};

describe("team cloud trust boundary", () => {
  it("preserves retry keys and revision conditions through native IPC", async () => {
    const invoke = vi
      .fn()
      .mockRejectedValueOnce(new Error("connection interrupted"))
      .mockResolvedValue({ data: { skill: { revision: 8 } } });
    vi.stubGlobal("__TAURI_INTERNALS__", { invoke });
    const options = {
      method: "POST",
      body: { name: "Team" },
      idempotencyKey: "same-attempt",
      ifMatch: 7,
    };
    await expect(request("/v1/teams", options)).rejects.toThrow(
      "connection interrupted",
    );
    await expect(request("/v1/teams", options)).resolves.toEqual({
      skill: { revision: 8 },
    });
    expect(invoke).toHaveBeenCalledTimes(2);
    for (const call of invoke.mock.calls)
      expect(call).toEqual([
        "cloud_request",
        {
          method: "POST",
          path: "/v1/teams",
          body: { name: "Team" },
          idempotencyKey: "same-attempt",
          ifMatch: "7",
        },
        undefined,
      ]);
    await request("/v1/teams/t/skills/s", {
      method: "PATCH",
      body: { title: "Updated" },
      ifMatch: 8,
    });
    expect(invoke).toHaveBeenLastCalledWith(
      "cloud_request",
      {
        method: "PATCH",
        path: "/v1/teams/t/skills/s",
        body: { title: "Updated" },
        ifMatch: "8",
        idempotencyKey: null,
      },
      undefined,
    );
  });
  it("does not request private data before authentication", async () => {
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    await expect(request("/v1/me/teams")).rejects.toThrow("登录");
    expect(fetch).not.toHaveBeenCalled();
  });
  it("rejects insecure remote origins", async () => {
    await expect(
      saveConfig({ ...config, cloud_api_url: "http://remote.example.test" }),
    ).rejects.toThrow("HTTPS");
  });
  it("uses verified login result only in memory and clears it on sign out", async () => {
    await saveConfig(config);
    const storage = vi.spyOn(localStorage, "setItem");
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            access_token: "test-session",
            user: { id: "user-a" },
          }),
        ),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ data: { teams: [] } })),
      );
    vi.stubGlobal("fetch", fetch);
    expect(await verifyOtp("alice@example.test", "123456")).toEqual({
      id: "user-a",
    });
    expect(await request("/v1/me/teams")).toEqual({ teams: [] });
    expect(fetch.mock.calls[1][1].headers.Authorization).toBe(
      "Bearer test-session",
    );
    expect(storage).not.toHaveBeenCalled();
    await signOut();
    await expect(request("/v1/me/teams")).rejects.toThrow("登录");
    storage.mockRestore();
  });
  it("shows service errors instead of silently returning an empty catalog", async () => {
    await saveConfig(config);
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ msg: "Email rate limit exceeded" }), {
          status: 429,
        }),
      ),
    );
    await expect(requestOtp("alice@example.test")).rejects.toThrow(
      "Email rate limit exceeded",
    );
  });
});
