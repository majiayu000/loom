import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api, type RegistryOperationRecord } from "../../lib/api/client";
import type { Op } from "../../lib/types";
import { HistoryPage } from "./HistoryPage";
import { OpsPage } from "./OpsPage";
import { SyncPage } from "./SyncPage";
import { ZERO_OPERATION_COUNTS } from "../../types";

function activity(status: Op["status"], id: string): Op {
  return {
    id,
    status,
    kind: status === "pending" ? "sync.replay" : "skill.project",
    skill: id,
    target: "target-1",
    method: "copy",
    time: "now",
    reason: status === "err" ? "failed to write" : undefined,
  };
}

function operation(overrides: Partial<RegistryOperationRecord> = {}): RegistryOperationRecord {
  return {
    op_id: "op-1",
    audit_id: "audit-1",
    request_id: "req-1",
    source: "panel",
    intent: "skill.project",
    status: "succeeded",
    ack: false,
    skill: "skill.writer",
    target: "target-1",
    binding: "binding-1",
    method: "copy",
    payload: { skill: "skill.writer", target: "target-1" },
    effects: { projection: "projection-1" },
    created_at: "2026-04-09T10:05:00Z",
    updated_at: "2026-04-09T10:05:00Z",
    ...overrides,
  };
}

function diagnosePayload(conflictCount = 0) {
  return {
    ok: true,
    data: {
      local_branch: true,
      remote_tracking: true,
      ahead: 0,
      behind: 0,
      local_segments: 1,
      local_archives: 0,
      remote_segments: 1,
      remote_archives: 0,
      local_snapshot: true,
      remote_snapshot: true,
      compact_after_segments: 8,
      retain_recent_segments: 4,
      retain_archives: 4,
      conflicts: Array.from({ length: conflictCount }, (_, index) => ({
        scope: "segment",
        path: `segments/${index}.jsonl`,
        local_blob: "local",
        remote_blob: "remote",
        local_rename_path: `segments/${index}.local.jsonl`,
        remote_rename_path: `segments/${index}.remote.jsonl`,
      })),
    },
  };
}

describe("Ops, History, and Sync pages", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("orders Activity rows with replayable and failed work first", () => {
    const { container } = render(
      <OpsPage
        ops={[activity("ok", "done-skill"), activity("pending", "queued-skill"), activity("err", "failed-skill")]}
        operationCounts={ZERO_OPERATION_COUNTS}
        onMutation={() => {}}
        readOnly={false}
      />,
    );

    const rows = Array.from(container.querySelectorAll(".op-row")).map((row) => row.textContent ?? "");
    expect(rows[0]).toContain("queued-skill");
    expect(rows[1]).toContain("failed-skill");
    expect(rows[2]).toContain("done-skill");
    expect(screen.getByText("Actionable operations")).toBeInTheDocument();
    expect(screen.getByText("Local journal events")).toBeInTheDocument();
    expect(screen.getByText("Unpushed history events")).toBeInTheDocument();
    expect(screen.getByText("Local-only history events")).toBeInTheDocument();
  });

  it("enables queue actions from canonical actionable counts for succeeded and failed rows", async () => {
    const retry = vi.spyOn(api, "opsRetry").mockResolvedValue({ ok: true, cmd: "ops.retry", request_id: "req-retry" });
    render(
      <OpsPage
        ops={[{ ...activity("ok", "succeeded-unacked"), actionable: true }, { ...activity("err", "failed"), actionable: true }]}
        operationCounts={{ ...ZERO_OPERATION_COUNTS, actionable_operations: 2 }}
        onMutation={() => {}}
        readOnly={false}
      />,
    );

    const retryButton = screen.getByRole("button", { name: /Retry replayable \(2\)/i }) as HTMLButtonElement;
    expect(retryButton.disabled).toBe(false);
    fireEvent.click(retryButton);
    await waitFor(() => expect(retry).toHaveBeenCalledTimes(1));
    expect((screen.getByRole("button", { name: /Clear replayable/i }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("filters Audit History by real fields and opens raw detail", async () => {
    vi.spyOn(api, "ops").mockResolvedValue({
      ok: true,
      data: {
        count: 1,
        loaded_count: 1,
        offset: 0,
        limit: 100,
        has_more: false,
        operations: [operation()],
      },
    });
    vi.spyOn(api, "opsHistoryDiagnose").mockResolvedValue(diagnosePayload());

    render(<HistoryPage live={true} mode="live" mutationVersion={0} />);

    expect(await screen.findByText(/skill.writer skill projection done/i)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Skill filter"), { target: { value: "writer" } });
    expect(screen.getByText(/skill.writer skill projection done/i)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Request/audit/op id filter"), { target: { value: "req-1" } });
    expect(screen.getByText(/skill.writer skill projection done/i)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Request/audit/op id filter"), { target: { value: "missing-id" } });
    expect(screen.getByText("No activity matches the current filter.")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Clear operation filters"));
    fireEvent.click(screen.getByText(/skill.writer skill projection done/i));

    expect(await screen.findByText("Audit detail")).toBeInTheDocument();
    expect(screen.getByText("op-1")).toBeInTheDocument();
    expect(screen.getByText("audit-1")).toBeInTheDocument();
    expect(screen.getByText("req-1")).toBeInTheDocument();
    expect(screen.getByText(/skill:skill.writer/)).toBeInTheDocument();
    expect(screen.getAllByText(/binding binding-1/).length).toBeGreaterThan(0);
  });

  it("truncates large primitive payload fields in Audit History detail", async () => {
    vi.spyOn(api, "ops").mockResolvedValue({
      ok: true,
      data: {
        count: 1,
        loaded_count: 1,
        offset: 0,
        limit: 100,
        has_more: false,
        operations: [operation({ payload: { note: "x".repeat(120) } })],
      },
    });
    vi.spyOn(api, "opsHistoryDiagnose").mockResolvedValue(diagnosePayload());

    render(<HistoryPage live={true} mode="live" mutationVersion={0} />);
    fireEvent.click(await screen.findByText(/skill.writer skill projection done/i));

    expect(screen.getByText(/^note:x{77}\.\.\.$/)).toBeInTheDocument();
    expect(screen.queryByText(`note:${"x".repeat(120)}`)).not.toBeInTheDocument();
  });

  it("exposes a manual Sync history diagnosis action", async () => {
    const diagnose = vi.spyOn(api, "opsHistoryDiagnose").mockResolvedValue(diagnosePayload());

    render(
      <SyncPage
        remote={{ configured: true, url: "git@example.com:loom.git", ahead: 0, behind: 0, sync_state: "clean", operation_counts: ZERO_OPERATION_COUNTS }}
        queuedWriteCount={0}
        operationCounts={ZERO_OPERATION_COUNTS}
        registryRoot="/tmp/loom"
        readOnly={false}
        onMutation={() => {}}
      />,
    );

    await waitFor(() => expect(diagnose).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Diagnose history" }));
    await waitFor(() => expect(diagnose).toHaveBeenCalledTimes(2));
  });

  it("renders the four non-overlapping operation counters", async () => {
    vi.spyOn(api, "opsHistoryDiagnose").mockResolvedValue(diagnosePayload());

    render(
      <SyncPage
        remote={{
          configured: false,
          sync_state: "LOCAL_ONLY",
          operation_backlog: 0,
          operation_counts: {
            actionable_operations: 0,
            local_journal_events: 3,
            unpushed_history_events: 0,
            local_only_history_events: 400,
          },
        }}
        queuedWriteCount={0}
        operationCounts={{
          actionable_operations: 0,
          local_journal_events: 3,
          unpushed_history_events: 0,
          local_only_history_events: 400,
        }}
        registryRoot="/tmp/loom"
        readOnly={false}
        onMutation={() => {}}
      />,
    );

    expect(await screen.findByText("Actionable operations")).toBeInTheDocument();
    expect(screen.getByText("Local journal events")).toBeInTheDocument();
    expect(screen.getByText("Unpushed history events")).toBeInTheDocument();
    expect(screen.getByText("Local-only history events")).toBeInTheDocument();
    expect(screen.getByText("400")).toBeInTheDocument();
  });
});
