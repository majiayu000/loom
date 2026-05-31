import { describe, expect, it } from "vitest";
import { adaptRegistryOperation } from "./adapters";
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
    expect(
      adaptRegistryOperation(
        operation({ status: "succeeded", last_error: { code: "IO_ERROR", message: "failed" } }),
      ).status,
    ).toBe("err");
  });
});
