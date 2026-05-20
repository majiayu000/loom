import { expect, test } from "vitest";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { OverviewPage } from "./OverviewPage";
import { HistoryPage } from "./HistoryPage";
import { SyncPage } from "./SyncPage";
import { api, type OpsHistoryDiagnosePayload, type OpsPayload, type RegistryOperationRecord } from "../../lib/api/client";
import type { Op, Target } from "../../lib/types";

function textOf(value: unknown): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map((item) => textOf(item)).join("");
  if (value && typeof value === "object" && "props" in value) {
    return textOf((value as { props?: { children?: unknown } }).props?.children);
  }
  return "";
}

function markup(renderer: ReactTestRenderer): string {
  return JSON.stringify(renderer.toJSON());
}

function buttonByLabel(renderer: ReactTestRenderer, label: string): ReactTestInstance {
  const button = renderer.root.findAll(
    (node: ReactTestInstance) => node.type === "button" && textOf(node.props.children).includes(label),
  )[0];
  if (!button) throw new Error(`button ${label} not found`);
  return button;
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function makeTarget(): Target {
  return {
    id: "target-1",
    agent: "claude",
    profile: "home",
    path: "~/.claude/skills",
    ownership: "managed",
    skills: 0,
    lastSync: "now",
  };
}

function makePanelOp(): Op {
  return {
    id: "op-pending",
    status: "pending",
    kind: "sync-replay",
    skill: "-",
    target: "-",
    method: "copy",
    time: "now",
  };
}

function makeOperation(status: string, ack = false, opId = "op_123", intent = "skill.project"): RegistryOperationRecord {
  return {
    op_id: opId,
    intent,
    status,
    ack,
    payload: {},
    effects: {},
    created_at: "2026-04-09T10:05:00Z",
    updated_at: "2026-04-09T10:05:00Z",
  };
}

function opsPayload(operation: RegistryOperationRecord): OpsPayload {
  return {
    ok: true,
    data: {
      count: 1,
      loaded_count: 1,
      offset: 0,
      limit: 100,
      has_more: false,
      operations: [operation],
      checkpoint: { last_scanned_op_id: operation.op_id ?? undefined },
    },
  };
}

function historyDiagnosePayload(conflictCount = 0): OpsHistoryDiagnosePayload {
  return {
    ok: true,
    data: {
      local_branch: true,
      remote_tracking: true,
      ahead: 0,
      behind: 0,
      local_segments: 2,
      local_archives: 0,
      remote_segments: 2,
      remote_archives: 0,
      local_snapshot: true,
      remote_snapshot: true,
      compact_after_segments: 64,
      retain_recent_segments: 16,
      retain_archives: 8,
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

test("OverviewPage shows actionable next steps for a partial registry", async () => {
  let openedSkills = 0;
  let openedSync = 0;
  let renderer: ReactTestRenderer;
  await act(async () => {
    renderer = create(
      <OverviewPage
        skills={[]}
        targets={[makeTarget()]}
        ops={[makePanelOp()]}
        projections={[]}
        vizMode="loom"
        setVizMode={() => {}}
        selectedSkill={null}
        selectedTarget={null}
        onSelectSkill={() => {}}
        onSelectTarget={() => {}}
        registryRoot="/tmp/loom"
        onMutation={() => {}}
        onNewTarget={() => {}}
        onNewBinding={() => {}}
        onOpenSkills={() => {
          openedSkills += 1;
        }}
        onViewActivity={() => {}}
        onOpenSync={() => {
          openedSync += 1;
        }}
        readOnly={false}
      />,
    );
  });

  expect(markup(renderer!).includes("Next steps")).toBe(true);
  expect(markup(renderer!).includes("No tracked skills yet.")).toBe(true);
  buttonByLabel(renderer!, "Open Skills").props.onClick();
  buttonByLabel(renderer!, "Replay pending").props.onClick();
  expect(openedSkills).toBe(1);
  expect(openedSync).toBe(1);
});

test("HistoryPage repairs diagnosed history conflicts from the panel", async () => {
  const originalOps = api.ops;
  const originalDiagnose = api.opsHistoryDiagnose;
  const originalRepair = api.opsHistoryRepair;
  const repairs: Array<{ strategy: "local" | "remote" }> = [];
  let mutations = 0;
  let diagnoseCalls = 0;
  api.ops = async () => {
    const operation = {
      ...makeOperation("failed", false, "op-failed", "sync.pull"),
      last_error: { code: "history_conflict", message: "loom-history path conflict" },
    };
    return opsPayload(operation);
  };
  api.opsHistoryDiagnose = async () => {
    diagnoseCalls += 1;
    return historyDiagnosePayload(diagnoseCalls === 1 ? 1 : 0);
  };
  api.opsHistoryRepair = async (body) => {
    repairs.push(body);
    return { ok: true, cmd: "ops.history.repair", request_id: "req-repair", data: { repaired_conflicts: 1 } };
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <HistoryPage
          live={true}
          mode="live"
          mutationVersion={0}
          onMutation={() => {
            mutations += 1;
          }}
        />,
      );
    });
    await flush();

    expect(markup(renderer!).includes("loom-history path conflict")).toBe(true);
    expect(markup(renderer!).includes("segments/0.jsonl")).toBe(true);

    await act(async () => {
      buttonByLabel(renderer!, "Repair from remote").props.onClick();
    });
    await flush();

    expect(repairs).toEqual([{ strategy: "remote" }]);
    expect(mutations).toBe(1);
  } finally {
    api.ops = originalOps;
    api.opsHistoryDiagnose = originalDiagnose;
    api.opsHistoryRepair = originalRepair;
  }
});

test("SyncPage surfaces history repair actions", async () => {
  const originalDiagnose = api.opsHistoryDiagnose;
  const originalRepair = api.opsHistoryRepair;
  const repairs: Array<{ strategy: "local" | "remote" }> = [];
  let mutations = 0;
  api.opsHistoryDiagnose = async () => historyDiagnosePayload(1);
  api.opsHistoryRepair = async (body) => {
    repairs.push(body);
    return { ok: true, cmd: "ops.history.repair", request_id: "req-repair", data: { repaired_conflicts: 1 } };
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SyncPage
          remote={{ configured: true, url: "git@example.com:loom.git", ahead: 0, behind: 0, sync_state: "clean" }}
          pendingCount={0}
          registryRoot="/tmp/loom"
          readOnly={false}
          onMutation={() => {
            mutations += 1;
          }}
        />,
      );
    });
    await flush();

    expect(markup(renderer!).includes("History repair")).toBe(true);
    expect(markup(renderer!).includes("segments/0.jsonl")).toBe(true);

    await act(async () => {
      buttonByLabel(renderer!, "Repair from local").props.onClick();
    });
    await flush();

    expect(repairs).toEqual([{ strategy: "local" }]);
    expect(mutations).toBe(1);
  } finally {
    api.opsHistoryDiagnose = originalDiagnose;
    api.opsHistoryRepair = originalRepair;
  }
});
