import { describe, expect, it } from "vitest";
import {
  describeActivityOperation,
  describeRegistryOperation,
  registryOperationDisplayId,
} from "./operation_labels";
import type { RegistryOperationRecord } from "./api/client";
import type { Op } from "./types";

function registryOperation(overrides: Partial<RegistryOperationRecord> = {}): RegistryOperationRecord {
  return {
    op_id: "op_123",
    intent: "target.add",
    status: "pending",
    ack: false,
    created_at: "2026-04-09T10:05:00Z",
    updated_at: "2026-04-09T10:05:00Z",
    ...overrides,
  };
}

describe("operation labels", () => {
  it("puts the operator-facing action before raw target ids in Activity rows", () => {
    const op: Op = {
      id: "op_123",
      status: "pending",
      kind: "target.add",
      skill: "target.add",
      target: "target_claude_proj_a",
      method: "—",
      time: "now",
    };

    const label = describeActivityOperation(op);

    expect(label.category).toBe("Target");
    expect(label.title).toBe("Claude target registration pending");
    expect(label.title).not.toContain("op_123");
    expect(label.details).toContain("id op_123");
    expect(label.details).toContain("target target_claude_proj_a");
  });

  it("keeps audit ids accessible without making them the History row title", () => {
    const label = describeRegistryOperation(
      registryOperation({
        target: "target_claude_proj_a",
        status: "succeeded",
        ack: true,
      }),
    );

    expect(label.title).toBe("Claude target registration done");
    expect(label.title).not.toContain("op_123");
    expect(label.details).toContain("intent target.add");
    expect(label.details).toContain("id op_123");
    expect(label.details).toContain("synced");
  });

  it("uses the action phrase directly when a target id has no agent hint", () => {
    const label = describeActivityOperation({
      id: "op_456",
      status: "pending",
      kind: "target.add",
      skill: "target.add",
      target: "target-1",
      method: "—",
      time: "now",
    });

    expect(label.title).toBe("Target registration pending");
  });

  it("falls back to audit or request ids when registry operation ids are absent", () => {
    expect(
      registryOperationDisplayId(
        registryOperation({
          op_id: null,
          audit_id: "audit_1",
          request_id: "req_1",
        }),
      ),
    ).toBe("audit_1");
  });
});
