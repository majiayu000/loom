import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { SkillMAuditHistory } from "./SkillMAuditHistory";
import { api, type RegistryOperationRecord } from "../lib/api/client";

function registryOperation(overrides: Partial<RegistryOperationRecord> = {}): RegistryOperationRecord {
  return {
    op_id: "op_123",
    audit_id: "audit_123",
    request_id: "req_123",
    source: "registry",
    intent: "target.add",
    status: "succeeded",
    ack: true,
    created_at: "2026-04-09T10:05:00Z",
    updated_at: "2026-04-09T10:06:00Z",
    ...overrides,
  };
}

function opsResponse(operations: RegistryOperationRecord[], hasMore = false, offset = 0) {
  return {
    ok: true,
    data: {
      count: operations.length,
      loaded_count: operations.length,
      offset,
      limit: 100,
      has_more: hasMore,
      operations,
    },
  };
}

describe("SkillMAuditHistory", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("keeps raw audit fields out of collapsed rows until the row is expanded", async () => {
    const rawOperation = registryOperation({
      op_id: "op_raw_123",
      audit_id: "audit_raw_123",
      request_id: "req_raw_123",
      source: "daemon-source",
      intent: "target.add",
      status: "succeeded",
      target: "target_claude_proj_a",
      created_at: "2026-04-09T10:05:00Z",
      updated_at: "2026-04-09T10:06:00Z",
    });
    vi.spyOn(api, "ops").mockResolvedValue(opsResponse([rawOperation]));

    render(<SkillMAuditHistory live refreshKey={null} />);

    const title = await screen.findByText("Claude target registration done");
    const row = title.closest("details");
    expect(row).not.toBeNull();

    expect(within(row!).queryByText("target.add")).toBeNull();
    expect(within(row!).queryByText("succeeded")).toBeNull();
    expect(within(row!).queryByText("daemon-source")).toBeNull();
    expect(within(row!).queryByText("op_raw_123")).toBeNull();
    expect(within(row!).queryByText(/target_claude_proj_a/)).toBeNull();
    expect(within(row!).queryByText("2026-04-09T10:06:00Z")).toBeNull();

    fireEvent.click(within(row!).getByText("详情"));

    expect(await within(row!).findByText("target.add")).toBeTruthy();
    expect(within(row!).getByText("succeeded")).toBeTruthy();
    expect(within(row!).getByText("daemon-source")).toBeTruthy();
    expect(within(row!).getByText("op_raw_123")).toBeTruthy();
    expect(within(row!).getByText(/target_claude_proj_a/)).toBeTruthy();
    expect(within(row!).getByText("2026-04-09T10:06:00Z")).toBeTruthy();
  });

  it("filters loaded audit rows by text, status bucket, and operation category while preserving pagination", async () => {
    const targetOp = registryOperation({
      op_id: "op_target",
      intent: "target.add",
      target: "target_claude_proj_a",
    });
    const importOp = registryOperation({
      op_id: "op_import",
      intent: "skill.import_observed",
      skill: "writer",
    });
    const failedSyncOp = registryOperation({
      op_id: "op_sync",
      intent: "sync.push",
      status: "failed",
      ack: false,
      last_error: { code: "push_failed", message: "remote rejected update" },
    });
    const opsSpy = vi
      .spyOn(api, "ops")
      .mockResolvedValueOnce(opsResponse([targetOp, importOp, failedSyncOp], true))
      .mockResolvedValueOnce(opsResponse([], false, 100));

    render(<SkillMAuditHistory live refreshKey={null} />);

    expect(await screen.findByText("3 loaded audit changes.")).toBeTruthy();
    expect(screen.getByText("Claude target registration done")).toBeTruthy();
    expect(screen.getByText("writer observed skill import done")).toBeTruthy();
    expect(screen.getByText("Remote sync push failed")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Audit text filter"), { target: { value: "writer" } });
    expect(screen.getByText("writer observed skill import done")).toBeTruthy();
    expect(screen.queryByText("Claude target registration done")).toBeNull();
    expect(screen.queryByText("Remote sync push failed")).toBeNull();

    fireEvent.change(screen.getByLabelText("Audit text filter"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("Audit status filter"), { target: { value: "err" } });
    expect(screen.getByText("Remote sync push failed")).toBeTruthy();
    expect(screen.queryByText("writer observed skill import done")).toBeNull();

    fireEvent.change(screen.getByLabelText("Audit status filter"), { target: { value: "all" } });
    fireEvent.change(screen.getByLabelText("Audit operation type filter"), { target: { value: "Target" } });
    expect(screen.getByText("Claude target registration done")).toBeTruthy();
    expect(screen.queryByText("writer observed skill import done")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "older" }));

    await waitFor(() => expect(opsSpy).toHaveBeenCalledTimes(2));
    expect(opsSpy.mock.calls[1][0]).toEqual({ limit: 100, offset: 100 });
  });

  it("can transition from offline to live without changing hook order", async () => {
    const opsSpy = vi
      .spyOn(api, "ops")
      .mockResolvedValue(opsResponse([registryOperation({ target: "target_claude_proj_a" })]));

    const { rerender } = render(<SkillMAuditHistory live={false} refreshKey={null} />);

    expect(screen.getByText("Audit history needs the live panel API.")).toBeTruthy();

    rerender(<SkillMAuditHistory live refreshKey="ready" />);

    expect(await screen.findByText("Claude target registration done")).toBeTruthy();
    expect(opsSpy).toHaveBeenCalledWith({ limit: 100, offset: 0 }, expect.any(AbortSignal));
  });
});
