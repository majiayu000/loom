import { describe, expect, it } from "vitest";
import type { RegistryProjection } from "../generated/RegistryProjection";
import type { PanelLiveData } from "./api/usePanelData";
import {
  selectOperationViewModel,
  selectPanelViewModel,
  selectProjectionLinks,
  selectProjectionViewModel,
  selectTargetViewModel,
} from "./panel_view_model";
import type { Op, Target } from "./types";

function liveData(overrides: Partial<PanelLiveData> = {}): PanelLiveData {
  return {
    live: true,
    apiReachable: true,
    loading: false,
    error: null,
    mode: "live",
    setupRequired: false,
    lastUpdated: "2026-06-11T12:00:00Z",
    registryRoot: "/tmp/loom",
    remote: { sync_state: "CLEAN" },
    warnings: [],
    health: { ok: true },
    counts: { skills: 1, targets: 1, bindings: 0, projections: 0, operations: 0 },
    skills: [],
    targets: [],
    bindings: [],
    ops: [],
    projections: [],
    queuedWriteCount: 0,
    refetch: () => {},
    ...overrides,
  };
}

describe("panel view-model selectors", () => {
  it("renders unknown enum values as unknown instead of coercing them", () => {
    const projection = {
      instance_id: "projection-1",
      skill_id: "typed-api-client",
      target_id: "target-1",
      materialized_path: "/tmp/target/skill",
      method: "teleport",
      last_applied_rev: "abcdef123",
      health: "healthy",
    } as RegistryProjection;
    const target = {
      id: "target-1",
      agent: "future-agent",
      profile: "home",
      path: "/tmp/target",
      ownership: "delegated",
      skills: 0,
      observedSkills: 0,
      projectedSkills: 0,
      lastSync: "now",
    } as unknown as Target;
    const op = {
      id: "op-1",
      status: "blocked",
      kind: "project",
      skill: "typed-api-client",
      target: "target-1",
      method: "teleport",
      time: "now",
    } as unknown as Op;

    expect(selectProjectionViewModel(projection).method).toMatchObject({
      state: "available",
      label: "unknown",
      raw: "teleport",
    });
    expect(selectProjectionLinks([projection])[0].method).toBe("unknown");
    expect(selectTargetViewModel(target).ownership).toMatchObject({
      state: "available",
      label: "unknown",
      raw: "delegated",
    });
    expect(selectOperationViewModel(op).status).toMatchObject({
      state: "available",
      label: "unknown",
      raw: "blocked",
    });
  });

  it("marks missing backend fields as explicitly unavailable", () => {
    const projection = {
      instance_id: "projection-1",
      skill_id: "typed-api-client",
      target_id: "target-1",
      materialized_path: "/tmp/target/skill",
      method: "symlink",
      last_applied_rev: "abcdef123",
      health: "healthy",
    } as RegistryProjection;
    const vm = selectProjectionViewModel(projection);

    expect(vm.binding).toMatchObject({ state: "unavailable", label: "unavailable" });
    expect(vm.updatedAt).toMatchObject({ state: "unavailable", label: "unavailable" });
  });

  it("does not fabricate graph methods when projection method is missing", () => {
    const projection = {
      instance_id: "projection-1",
      skill_id: "typed-api-client",
      target_id: "target-1",
      materialized_path: "/tmp/target/skill",
      last_applied_rev: "abcdef123",
      health: "healthy",
    } as RegistryProjection;

    expect(selectProjectionViewModel(projection).method).toMatchObject({
      state: "unavailable",
      label: "unavailable",
    });
    expect(selectProjectionLinks([projection])[0].method).toBe("unknown");
  });

  it("covers shell counts and does not invent missing backend counts", () => {
    const vm = selectPanelViewModel(liveData({ counts: {}, queuedWriteCount: 2 }), {
      page: "overview",
      readOnly: false,
    });

    expect(vm.shell.counts.skills).toMatchObject({ value: 0, display: "0", state: "available" });
    expect(vm.shell.counts.queuedWrites).toMatchObject({ value: 2, display: "2", state: "available" });
    expect(vm.shell.counts.backend.skills).toMatchObject({
      value: null,
      display: "unavailable",
      state: "unavailable",
    });
  });

  it("disables every mutation action while the panel is read-only", () => {
    const vm = selectPanelViewModel(
      liveData({
        mode: "offline-empty",
        live: false,
        queuedWriteCount: 2,
      }),
      { page: "overview", readOnly: true },
    );

    expect(Object.values(vm.actions)).toHaveLength(10);
    for (const action of Object.values(vm.actions)) {
      expect(action.mutation).toBe(true);
      expect(action.enabled).toBe(false);
      expect(action.disabledReason).toBe("registry offline");
    }
  });

  it("uses the live data mode as the read-only action reason", () => {
    const vm = selectPanelViewModel(
      liveData({
        mode: "first-run",
        setupRequired: true,
        live: true,
      }),
      { page: "overview", readOnly: true },
    );

    expect(vm.shell.readOnlyReason).toBe("registry setup required");
    expect(vm.actions.addSkill.disabledReason).toBe("registry setup required");
  });
});
