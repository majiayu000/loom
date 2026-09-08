import { afterEach, describe, expect, it, vi } from "vitest";
import { request, requestOtp, saveConfig, signOut, verifyOtp } from "./client";

afterEach(async () => { await signOut(); vi.unstubAllGlobals(); });
const config = { cloud_api_url: "https://cloud.example.test", auth_url: "https://auth.example.test", auth_public_key: "public-test-key" };

describe("team cloud trust boundary", () => {
  it("does not request private data before authentication", async () => {
    const fetch = vi.fn(); vi.stubGlobal("fetch", fetch);
    await expect(request("/v1/me/teams")).rejects.toThrow("登录");
    expect(fetch).not.toHaveBeenCalled();
  });
  it("rejects insecure remote origins", async () => {
    await expect(saveConfig({ ...config, cloud_api_url: "http://remote.example.test" })).rejects.toThrow("HTTPS");
  });
  it("uses verified login result only in memory and clears it on sign out", async () => {
    await saveConfig(config);
    const storage = vi.spyOn(localStorage, "setItem");
    const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ access_token: "test-session", user: { id: "user-a" } })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: { teams: [] } })));
    vi.stubGlobal("fetch", fetch);
    expect(await verifyOtp("alice@example.test", "123456")).toEqual({ id: "user-a" });
    expect(await request("/v1/me/teams")).toEqual({ teams: [] });
    expect(fetch.mock.calls[1][1].headers.Authorization).toBe("Bearer test-session");
    expect(storage).not.toHaveBeenCalled();
    await signOut();
    await expect(request("/v1/me/teams")).rejects.toThrow("登录");
    storage.mockRestore();
  });
  it("shows service errors instead of silently returning an empty catalog", async () => {
    await saveConfig(config);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ msg: "Email rate limit exceeded" }), { status: 429 })));
    await expect(requestOtp("alice@example.test")).rejects.toThrow("Email rate limit exceeded");
  });
});
