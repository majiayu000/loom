import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError } from "./client";

describe("api.registryStatus", () => {
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

    await expect(api.registryStatus()).rejects.toEqual(
      expect.objectContaining<ApiError>({
        name: "ApiError",
        path: "/api/v1/registry/status",
        status: 502,
        message: "GET /api/v1/registry/status returned 502",
      }),
    );
  });

  it("preserves AbortError when response parsing is canceled", async () => {
    const abortError = new DOMException("The operation was aborted.", "AbortError");
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockRejectedValue(abortError),
    } as unknown as Response);

    await expect(api.registryStatus()).rejects.toBe(abortError);
  });
});

describe("api v1 routes", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses v1 endpoints for panel mutations", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({ ok: true, cmd: "ok", request_id: "req-1", data: {} }),
    } as unknown as Response);

    await api.targetAdd({ agent: "claude", path: "/tmp/skills", ownership: "managed" });
    await api.targetRemove("target-1");
    await api.bindingAdd({
      agent: "claude",
      profile: "home",
      matcher_kind: "path_prefix",
      matcher_value: "/repo",
      target: "target-1",
      policy_profile: "safe-capture",
    });
    await api.bindingRemove("binding-1");
    await api.skillAdd({ source: "/tmp/source", name: "demo" });
    await api.skillImportObserved();
    await api.skillSave("demo", { message: "save demo" });
    await api.skillSnapshot("demo");
    await api.skillRelease("demo", { version: "v1" });
    await api.skillRollback("demo", { to: "HEAD~1" });
    await api.skillUse("demo", { agents: ["claude"], apply: false });
    await api.project({ skill: "demo", binding: "binding-1", target: "target-1", method: "symlink" });
    await api.capture({ instance: "inst-1" });
    await api.orphanClean({ delete_live_paths: false });
    await api.opsRetry();
    await api.opsPurge();
    await api.remoteSet({ url: "https://example.com/repo.git" });
    await api.syncPush();
    await api.syncPull();
    await api.syncReplay();
    await api.opsHistoryRepair({ strategy: "local" });

    const paths = fetchSpy.mock.calls.map((call) => call[0]);
    expect(paths).toEqual([
      "/api/v1/targets",
      "/api/v1/targets/target-1/remove",
      "/api/v1/bindings",
      "/api/v1/bindings/binding-1/remove",
      "/api/v1/skills",
      "/api/v1/skills/import-observed",
      "/api/v1/skills/demo/save",
      "/api/v1/skills/demo/snapshot",
      "/api/v1/skills/demo/release",
      "/api/v1/skills/demo/rollback",
      "/api/v1/skills/demo/use",
      "/api/v1/projections/project",
      "/api/v1/projections/capture",
      "/api/v1/orphans/clean",
      "/api/v1/ops/retry",
      "/api/v1/ops/purge",
      "/api/v1/workspace/remote",
      "/api/v1/sync/push",
      "/api/v1/sync/pull",
      "/api/v1/sync/replay",
      "/api/v1/ops/history/repair",
    ]);
  });

  it("uses the v1 endpoint for the skills read model", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        ok: true,
        cmd: "registry.skills",
        request_id: "req-1",
        data: { skills: [] },
        error: null,
        meta: { warnings: [] },
      }),
    } as unknown as Response);

    await api.skills();

    expect(fetchSpy).toHaveBeenCalledWith("/api/v1/skills", { signal: undefined });
  });

  it("uses v1 endpoints for panel bootstrap reads", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        ok: true,
        cmd: "ok",
        request_id: "req-1",
        data: {},
        error: null,
        meta: { warnings: [] },
      }),
    } as unknown as Response);

    await api.health();
    await api.info();
    await api.pending();

    const paths = fetchSpy.mock.calls.map((call) => call[0]);
    expect(paths).toEqual([
      "/api/v1/health",
      "/api/v1/workspace/info",
      "/api/v1/ops/pending",
    ]);
  });

  it("preserves envelope warnings for the skills read model", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        ok: true,
        cmd: "registry.skills",
        request_id: "req-1",
        data: { skills: [] },
        error: null,
        meta: { warnings: ["skipped malformed skill metadata"] },
      }),
    } as unknown as Response);

    await expect(api.skillsWithWarnings()).resolves.toEqual({
      data: { skills: [] },
      warnings: ["skipped malformed skill metadata"],
    });
  });

  it("preserves envelope warnings for registry payload reads", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        ok: true,
        cmd: "registry.status",
        request_id: "req-1",
        data: { counts: {}, projections: [], rules: [], targets: [], bindings: [] },
        error: null,
        meta: { warnings: ["ignored malformed operation audit row"] },
      }),
    } as unknown as Response);

    const result = await api.registryStatusWithWarnings();

    expect(result.warnings).toEqual(["ignored malformed operation audit row"]);
    expect(result.data.data?.counts).toEqual({});
  });

  it("combines sync status envelope and payload warnings", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        ok: true,
        cmd: "sync.status",
        request_id: "req-1",
        data: {
          remote: { sync_state: "CLEAN" },
          warnings: ["pending queue had parse warnings"],
        },
        error: null,
        meta: { warnings: ["failed to read remote tracking ref"] },
      }),
    } as unknown as Response);

    await expect(api.remoteStatusWithWarnings()).resolves.toEqual({
      data: {
        remote: { sync_state: "CLEAN" },
        warnings: ["pending queue had parse warnings"],
      },
      warnings: ["failed to read remote tracking ref", "pending queue had parse warnings"],
    });
  });

  it("rejects non-envelope payloads for v1 bootstrap reads", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({ root: "/tmp/loom" }),
    } as unknown as Response);

    await expect(api.info()).rejects.toEqual(
      expect.objectContaining<ApiError>({
        name: "ApiError",
        path: "/api/v1/workspace/info",
        status: 200,
        message: "GET /api/v1/workspace/info returned non-envelope payload",
      }),
    );
  });

  it("uses v1 endpoints for registry read routes", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        ok: true,
        cmd: "ok",
        request_id: "req-1",
        data: {},
        error: null,
        meta: { warnings: [] },
      }),
    } as unknown as Response);

    await api.registryStatus();
    await api.opsHistoryDiagnose();
    await api.bindingShow("binding 1");
    await api.targetShow("target 1");
    await api.skillHistory("demo skill");
    await api.skillInspect("demo skill");
    await api.skillDiff("demo skill", "old", "new");
    await api.telemetryReport();

    const paths = fetchSpy.mock.calls.map((call) => call[0]);
    expect(paths).toEqual([
      "/api/v1/registry/status",
      "/api/v1/ops/diagnose",
      "/api/v1/bindings/binding%201",
      "/api/v1/targets/target%201",
      "/api/v1/skills/demo%20skill/history",
      "/api/v1/skills/demo%20skill/inspect",
      "/api/v1/skills/demo%20skill/diff?rev_a=old&rev_b=new",
      "/api/v1/telemetry/report",
    ]);
  });
});
