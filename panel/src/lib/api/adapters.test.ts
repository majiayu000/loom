import { describe, expect, it } from "vitest";
import { adaptBinding, adaptPendingOp, adaptRegistryOperation, adaptTarget, buildAdapterIndex } from "./adapters";
import type { RegistryOperationRecord } from "./client";

function operation(overrides: Partial<RegistryOperationRecord> = {}): RegistryOperationRecord {
  return {
    op_id: "op-1",
    intent: "skill.save",
    status: "succeeded",
    ack: false,
    created_at: "2026-05-29T00:00:00Z",
    updated_at: "2026-05-29T00:00:00Z",
    ...overrides,
  };
}

describe("adaptRegistryOperation", () => {
  it("treats succeeded registry rows as complete even before sync ack", () => {
    expect(adaptRegistryOperation(operation()).status).toBe("ok");
  });

  it("keeps queued rows pending and failed rows errored", () => {
    expect(adaptRegistryOperation(operation({ status: "pending" })).status).toBe("pending");
    expect(adaptRegistryOperation(operation({ status: "enqueued" })).status).toBe("pending");
    expect(adaptRegistryOperation(operation({ status: "in_flight" })).status).toBe("pending");
    expect(adaptRegistryOperation(operation({ status: "retrying" })).status).toBe("pending");
    expect(
      adaptRegistryOperation(
        operation({ status: "succeeded", last_error: { code: "IO_ERROR", message: "failed" } }),
      ).status,
    ).toBe("err");
  });

  it("preserves raw timestamps for real activity aggregation", () => {
    const adapted = adaptRegistryOperation(
      operation({ created_at: "2026-05-28T12:00:00Z", updated_at: "2026-05-29T13:00:00Z" }),
    );

    expect(adapted.createdAt).toBe("2026-05-28T12:00:00Z");
    expect(adapted.updatedAt).toBe("2026-05-29T13:00:00Z");
  });
});

describe("api adapters enum handling", () => {
  it("surfaces unknown target ownership instead of coercing it to external", () => {
    const index = buildAdapterIndex([], []);
    const target = adaptTarget(
      {
        target_id: "target-1",
        agent: "claude",
        path: "/tmp/skills",
        ownership: "delegated",
        capabilities: { symlink: false, copy: false, watch: false },
      },
      index,
    );

    expect(target.ownership).toBe("unknown");
  });

  it("surfaces unknown projection methods instead of coercing them to symlink", () => {
    const binding = adaptBinding(
      {
        binding_id: "binding-1",
        agent: "claude",
        profile_id: "default",
        workspace_matcher: { kind: "path_prefix", value: "/repo" },
        default_target_id: "target-1",
        policy_profile: "manual",
        active: true,
      },
      [
        {
          binding_id: "binding-1",
          skill_id: "demo",
          target_id: "target-1",
          method: "teleport",
          watch_policy: "manual",
        },
      ],
    );

    expect(binding.method).toBe("unknown");
    expect(
      adaptPendingOp(
        {
          request_id: "req-1",
          command: "project",
          created_at: "2026-05-29T00:00:00Z",
          details: { method: "teleport" },
        },
        0,
      ).method,
    ).toBe("unknown");
    expect(adaptRegistryOperation(operation({ method: "teleport" })).method).toBe("unknown");
  });

  it("labels multi-rule bindings without fabricating a single projectable skill", () => {
    const binding = adaptBinding(
      {
        binding_id: "binding-1",
        agent: "claude",
        profile_id: "default",
        workspace_matcher: { kind: "path_prefix", value: "/repo" },
        default_target_id: "target-1",
        policy_profile: "auto",
        active: true,
      },
      [
        {
          binding_id: "binding-1",
          skill_id: "skill-a",
          target_id: "target-1",
          method: "copy",
          watch_policy: "auto",
        },
        {
          binding_id: "binding-1",
          skill_id: "skill-b",
          target_id: "target-1",
          method: "symlink",
          watch_policy: "auto",
        },
      ],
    );

    expect(binding.skill).toBe("multi");
    expect(binding.method).toBe("unknown");
    expect(binding.ruleCount).toBe(2);
    expect(binding.skillCount).toBe(2);
  });

  it("keeps a single-rule skill literally named multi projectable", () => {
    const binding = adaptBinding(
      {
        binding_id: "binding-1",
        agent: "claude",
        profile_id: "default",
        workspace_matcher: { kind: "path_prefix", value: "/repo" },
        default_target_id: "target-1",
        policy_profile: "auto",
        active: true,
      },
      [
        {
          binding_id: "binding-1",
          skill_id: "multi",
          target_id: "target-1",
          method: "copy",
          watch_policy: "auto",
        },
      ],
    );

    expect(binding.skill).toBe("multi");
    expect(binding.method).toBe("copy");
    expect(binding.ruleCount).toBe(1);
    expect(binding.skillCount).toBe(1);
  });
});
