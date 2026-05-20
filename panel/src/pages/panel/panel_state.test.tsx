import React from "react";
import { afterAll, expect, test } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { LiveDataBanner } from "../PanelApp";
import { BindingsPage } from "./BindingsPage";
import { HistoryPage, bucket } from "./HistoryPage";
import { TargetsPage } from "./TargetsPage";
import { SettingsPage } from "./SettingsPage";
import { OverviewPage } from "./OverviewPage";
import { DoctorPage } from "./DoctorPage";
import { FirstRunPage } from "./FirstRunPage";
import { ProjectionsPage } from "./ProjectionsPage";
import { BindingAddForm } from "../../components/panel/forms/BindingAddForm";
import { api, type BindingShowPayload, type CommandEnvelope, type DoctorPayload, type OpsPayload, type TargetShowPayload, type RegistryOperationRecord } from "../../lib/api/client";
import type { Binding, Skill, Target } from "../../lib/types";
import type { RegistryProjection } from "../../generated/RegistryProjection";

const originalWindow = (globalThis as { window?: unknown }).window;
const originalLocalStorage = (globalThis as { localStorage?: unknown }).localStorage;
const localStorageStub = {
  getItem: (_key: string) => null,
  setItem: (_key: string, _value: string) => {},
  removeItem: (_key: string) => {},
};
(globalThis as { window?: unknown }).window = {
  setTimeout,
  clearTimeout,
  confirm: () => true,
  location: { reload: () => {} },
  localStorage: localStorageStub,
} as unknown;
(globalThis as { localStorage?: unknown }).localStorage = localStorageStub;

afterAll(() => {
  (globalThis as { window?: unknown }).window = originalWindow;
  (globalThis as { localStorage?: unknown }).localStorage = originalLocalStorage;
});

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

function clickableRows(renderer: ReactTestRenderer): ReactTestInstance[] {
  return renderer.root.findAll((node: ReactTestInstance) => node.type === "tr" && typeof node.props.onClick === "function");
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

function makeTarget(overrides: Partial<Target> = {}): Target {
  return {
    id: "target-1",
    agent: "claude",
    profile: "home",
    path: "~/.claude/skills",
    ownership: "managed",
    skills: 0,
    lastSync: "now",
    ...overrides,
  };
}

function makeBinding(): Binding {
  return {
    id: "binding-1",
    skill: "skill.writer",
    target: "target-1",
    matcher: "path_prefix:/repo",
    method: "symlink",
    policy: "auto",
  };
}

function makeSkill(): Skill {
  return {
    id: "s-skill.writer",
    name: "skill.writer",
    tag: "v1",
    sourceStatus: "present",
    releaseTags: ["v1"],
    snapshotTags: [],
    latestRev: "abc12345",
    ruleCount: 1,
    bindingCount: 1,
    projectionCount: 1,
    changed: "now",
    targets: ["target-1"],
  };
}

function makeOrphanProjection(): RegistryProjection {
  return {
    instance_id: "inst-orphan",
    skill_id: "skill.writer",
    target_id: "target-1",
    materialized_path: "/tmp/inst-orphan",
    method: "copy",
    last_applied_rev: "deadbeefcafebabe",
    health: "orphaned",
  };
}

function bindingPayload(projectionCount: number): BindingShowPayload {
  return {
    ok: true,
    data: {
      state_model: "registry",
      binding: {
        binding_id: "binding-1",
        agent: "claude",
        profile_id: "home",
        workspace_matcher: { kind: "path_prefix", value: "/repo" },
        default_target_id: "target-1",
        policy_profile: "auto",
        active: true,
        created_at: "2026-04-09T10:05:00Z",
      },
      default_target: {
        target_id: "target-1",
        agent: "claude",
        path: "~/.claude/skills",
        ownership: "managed",
        capabilities: { symlink: true, copy: true, watch: true },
        created_at: "2026-04-09T10:05:00Z",
      },
      rules: [
        {
          binding_id: "binding-1",
          skill_id: "skill.writer",
          target_id: "target-1",
          method: "symlink",
          watch_policy: "auto",
          created_at: "2026-04-09T10:05:00Z",
        },
      ],
      projections:
        projectionCount === 0
          ? []
          : [
              {
                instance_id: "proj-1",
                skill_id: "skill.writer",
                binding_id: "binding-1",
                target_id: "target-1",
                materialized_path: "/tmp/proj-1",
                method: "symlink",
                last_applied_rev: "deadbeefcafebabe",
                health: "ok",
                updated_at: "2026-04-09T10:05:00Z",
              },
            ],
    },
  };
}

function targetPayload(projectionCount = 0): TargetShowPayload {
  return {
    ok: true,
    data: {
      state_model: "registry",
      target: {
        target_id: "target-1",
        agent: "claude",
        path: "~/.claude/skills",
        ownership: "managed",
        capabilities: { symlink: true, copy: true, watch: true },
        created_at: "2026-04-09T10:05:00Z",
      },
      bindings: [],
      projections:
        projectionCount === 0
          ? []
          : [
              {
                instance_id: "target-proj-1",
                skill_id: "skill.writer",
                binding_id: "binding-1",
                target_id: "target-1",
                materialized_path: "/tmp/target-proj-1",
                method: "symlink",
                last_applied_rev: "deadbeefcafebabe",
                health: "ok",
                updated_at: "2026-04-09T10:05:00Z",
              },
            ],
      rules: [],
    },
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

function doctorPayload(): DoctorPayload {
  return {
    healthy: false,
    checks_v1: [
      {
        section: "git",
        id: "git_fsck",
        ok: true,
        severity: "ok",
        message: "git object database is healthy",
        next_action: null,
        details: {},
      },
      {
        section: "pending_queue",
        id: "pending_queue_warnings",
        ok: false,
        severity: "warning",
        message: "pending queue has malformed or ignored entries",
        next_action: "inspect state/pending_ops.jsonl and repair or purge malformed queue entries",
        details: { warning_count: 1 },
      },
      {
        section: "agents",
        id: "agent_skill_inventory",
        ok: true,
        severity: "info",
        message: "detected 1 of 2 known agent skill directories",
        next_action: null,
        details: {
          agents: [
            {
              agent: "claude",
              default_path: "/tmp/home/.claude/skills",
              present: true,
              registered_target_count: 1,
              registered_targets: [
                {
                  target_id: "target_claude_claude_skills",
                  ownership: "observed",
                },
              ],
            },
            {
              agent: "codex",
              default_path: "/tmp/home/.codex/skills",
              present: false,
              registered_target_count: 0,
              registered_targets: [],
            },
          ],
        },
      },
    ],
  };
}

test("HistoryPage treats succeeded operations as successful", () => {
  expect(bucket(makeOperation("succeeded", false))).toBe("ok");
});

test("LiveDataBanner renders nothing during live refetch loading", () => {
  const html = renderToStaticMarkup(<LiveDataBanner error={null} loading={true} mode="live" />);
  expect(html).toBe("");
});

test("LiveDataBanner renders nothing in first-run mode", () => {
  const html = renderToStaticMarkup(<LiveDataBanner error={null} loading={false} mode="first-run" />);
  expect(html).toBe("");
});

test("FirstRunPage initializes the registry with scan enabled", async () => {
  const originalInit = api.workspaceInit;
  const calls: Array<{ scan_existing?: boolean }> = [];
  const envelope: CommandEnvelope = {
    ok: true,
    cmd: "workspace.init",
    request_id: "req-1",
    data: {
      initialized: true,
      scanned: true,
      imported: [{ target_id: "target-1" }],
      skipped: [{ agent: "codex" }, { agent: "cursor" }],
    },
    error: undefined,
    meta: { warnings: [] },
  };
  api.workspaceInit = async (body) => {
    calls.push(body);
    return envelope;
  };

  try {
    let ready = 0;
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FirstRunPage registryRoot="/tmp/loom" onReady={() => ready += 1} />);
    });

    await act(async () => {
      buttonByLabel(renderer!, "Initialize").props.onClick();
    });
    await flush();

    expect(calls).toEqual([{ scan_existing: true }]);
    expect(ready).toBe(1);
    expect(markup(renderer!).includes("1 observed targets imported")).toBe(true);
  } finally {
    api.workspaceInit = originalInit;
  }
});

test("OverviewPage disables add binding until a target exists", async () => {
  let renderer: ReactTestRenderer;
  await act(async () => {
    renderer = create(
      <OverviewPage
        skills={[]}
        targets={[]}
        ops={[]}
        projections={[]}
        vizMode="loom"
        setVizMode={() => {}}
        selectedSkill={null}
        selectedTarget={null}
        onSelectSkill={() => {}}
        onSelectTarget={() => {}}
        registryRoot={null}
        onMutation={() => {}}
        onNewTarget={() => {}}
        onNewBinding={() => {}}
        onOpenSkills={() => {}}
        onViewActivity={() => {}}
        onOpenSync={() => {}}
        readOnly={false}
      />,
    );
  });

  const addBinding = buttonByLabel(renderer!, "Add binding");
  expect(addBinding.props.disabled).toBe(true);
  expect(addBinding.props.title).toBe("add a target first");
});

test("BindingAddForm submits the canonical matcher kind", async () => {
  const originalBindingAdd = api.bindingAdd;
  const submissions: Array<Parameters<typeof api.bindingAdd>[0]> = [];
  let successCount = 0;

  api.bindingAdd = async (body) => {
    submissions.push(body);
    return { ok: true, cmd: "binding.add", request_id: "req-1" };
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BindingAddForm
          targets={[makeTarget()]}
          onCancel={() => {}}
          onSuccess={() => {
            successCount += 1;
          }}
        />,
      );
    });

    const matcherValue = renderer!.root
      .findAll((node: ReactTestInstance) => node.type === "input")
      .find((node) => node.props.placeholder === "/Users/me/work");
    if (!matcherValue) throw new Error("matcher value input not found");

    await act(async () => {
      matcherValue.props.onChange({ target: { value: "/repo" } });
    });

    await act(async () => {
      renderer!.root.findByType("form").props.onSubmit({ preventDefault: () => {} });
      await Promise.resolve();
    });

    expect(submissions[0]?.matcher_kind).toBe("path_prefix");
    expect(submissions[0]?.matcher_value).toBe("/repo");
    expect(submissions[0]?.target).toBe("target-1");
    expect(successCount).toBe(1);
  } finally {
    api.bindingAdd = originalBindingAdd;
  }
});

test("BindingsPage refetches selected binding details after a successful project", async () => {
  const target = makeTarget();
  const binding = makeBinding();
  const originalBindingShow = api.bindingShow;
  const originalProject = api.project;
  const bindingShowCalls: string[] = [];
  let detailRevision = 0;

  api.bindingShow = async (id: string) => {
    bindingShowCalls.push(id);
    return bindingPayload(detailRevision);
  };
  api.project = async () => {
    detailRevision = 1;
    return { ok: true, cmd: "project", request_id: "req-1" };
  };

  try {
    function Harness() {
      const [mutationVersion, setMutationVersion] = React.useState(0);
      return (
        <BindingsPage
          bindings={[binding]}
          targets={[target]}
          readOnly={false}
          mutationVersion={mutationVersion}
          onMutation={() => setMutationVersion((cur) => cur + 1)}
        />
      );
    }

    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<Harness />);
    });
    await act(async () => {
      clickableRows(renderer!)[0]?.props.onClick();
    });
    await flush();

    expect(bindingShowCalls.length).toBe(1);
    expect(markup(renderer!).includes("No projections realized yet for this binding.")).toBe(true);

    await act(async () => {
      buttonByLabel(renderer!, "Project now").props.onClick();
      await Promise.resolve();
      await Promise.resolve();
    });
    await flush();

    expect(bindingShowCalls.length).toBe(2);
    expect(markup(renderer!).includes("No projections realized yet for this binding.")).toBe(false);
    expect(markup(renderer!).includes("deadbeef")).toBe(true);
  } finally {
    api.bindingShow = originalBindingShow;
    api.project = originalProject;
  }
});

test("BindingsPage exposes orphan cleanup from live projection data", async () => {
  const originalOrphanClean = api.orphanClean;
  const calls: Array<{ delete_live_paths?: boolean }> = [];
  let mutations = 0;

  api.orphanClean = async (body) => {
    calls.push(body);
    return {
      ok: true,
      cmd: "skill.orphan.clean",
      request_id: "req-orphan",
      data: { cleaned_count: 1 },
    };
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BindingsPage
          bindings={[makeBinding()]}
          targets={[makeTarget()]}
          projections={[makeOrphanProjection()]}
          readOnly={false}
          mutationVersion={0}
          onMutation={() => {
            mutations += 1;
          }}
        />,
      );
    });

    expect(markup(renderer!).includes("inst-orphan")).toBe(true);
    expect(markup(renderer!).includes("orphaned projection")).toBe(true);

    await act(async () => {
      buttonByLabel(renderer!, "Clean metadata").props.onClick();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(calls).toEqual([{ delete_live_paths: false }]);
    expect(mutations).toBe(1);
  } finally {
    api.orphanClean = originalOrphanClean;
  }
});

test("ProjectionsPage can capture and re-project a selected projection", async () => {
  const originalCapture = api.capture;
  const originalProject = api.project;
  const captureCalls: Array<{ instance?: string }> = [];
  const projectCalls: Array<{ skill: string; binding: string; target?: string; method?: string }> = [];
  let mutations = 0;

  api.capture = async (body) => {
    captureCalls.push(body);
    return { ok: true, cmd: "skill.capture", request_id: "req-capture", data: {} };
  };
  api.project = async (body) => {
    projectCalls.push(body);
    return { ok: true, cmd: "skill.project", request_id: "req-project", data: {} };
  };

  try {
    const projection: RegistryProjection = {
      instance_id: "inst-demo",
      skill_id: "skill.writer",
      binding_id: "binding-1",
      target_id: "target-1",
      materialized_path: "/tmp/target-1/skill.writer",
      method: "copy",
      last_applied_rev: "deadbeefcafebabe",
      health: "healthy",
    };
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ProjectionsPage
          projections={[projection]}
          targets={[makeTarget()]}
          bindings={[makeBinding()]}
          readOnly={false}
          onMutation={() => {
            mutations += 1;
          }}
        />,
      );
    });

    await act(async () => {
      buttonByLabel(renderer!, "Capture").props.onClick();
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      buttonByLabel(renderer!, "Re-project").props.onClick();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(captureCalls).toEqual([{ instance: "inst-demo" }]);
    expect(projectCalls).toEqual([
      { skill: "skill.writer", binding: "binding-1", target: "target-1", method: "copy" },
    ]);
    expect(mutations).toBe(2);
  } finally {
    api.capture = originalCapture;
    api.project = originalProject;
  }
});

test("ProjectionsPage cleans orphaned projection metadata", async () => {
  const originalOrphanClean = api.orphanClean;
  const calls: Array<{ delete_live_paths?: boolean }> = [];
  api.orphanClean = async (body) => {
    calls.push(body);
    return { ok: true, cmd: "skill.orphan.clean", request_id: "req-clean", data: {} };
  };

  try {
    let mutations = 0;
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ProjectionsPage
          projections={[makeOrphanProjection()]}
          targets={[makeTarget()]}
          bindings={[makeBinding()]}
          readOnly={false}
          onMutation={() => {
            mutations += 1;
          }}
        />,
      );
    });

    await act(async () => {
      buttonByLabel(renderer!, "Clean orphan").props.onClick();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(calls).toEqual([{ delete_live_paths: false }]);
    expect(mutations).toBe(1);
  } finally {
    api.orphanClean = originalOrphanClean;
  }
});

test("HistoryPage refetches when a panel mutation completes", async () => {
  const originalOps = api.ops;
  const seen: string[] = [];
  let response = opsPayload(makeOperation("pending", false, "op-old", "sync.replay"));
  api.ops = async () => {
    seen.push(response.data?.operations[0]?.op_id ?? "none");
    return response;
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HistoryPage live={true} mode="live" mutationVersion={0} />);
    });
    await flush();

    expect(seen.length).toBe(1);
    expect(markup(renderer!).includes("op-old")).toBe(true);

    response = opsPayload(makeOperation("succeeded", true, "op-new", "sync.replay"));
    await act(async () => {
      renderer!.update(<HistoryPage live={true} mode="live" mutationVersion={1} />);
    });
    await flush();

    expect(seen.length).toBe(2);
    expect(markup(renderer!).includes("op-new")).toBe(true);
    expect(markup(renderer!).includes("op-old")).toBe(false);
  } finally {
    api.ops = originalOps;
  }
});

test("HistoryPage refetches when the shared live refresh key changes", async () => {
  const originalOps = api.ops;
  const seen: string[] = [];
  let response = opsPayload(makeOperation("pending", false, "op-old", "sync.replay"));
  api.ops = async () => {
    seen.push(response.data?.operations[0]?.op_id ?? "none");
    return response;
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HistoryPage live={true} mode="live" mutationVersion={0} refreshKey="tick-1" />);
    });
    await flush();

    response = opsPayload(makeOperation("succeeded", true, "op-new", "sync.replay"));
    await act(async () => {
      renderer!.update(<HistoryPage live={true} mode="live" mutationVersion={0} refreshKey="tick-2" />);
    });
    await flush();

    expect(seen.length).toBe(2);
    expect(markup(renderer!).includes("op-new")).toBe(true);
    expect(markup(renderer!).includes("op-old")).toBe(false);
  } finally {
    api.ops = originalOps;
  }
});

test("DoctorPage renders structured workspace doctor checks", async () => {
  const originalDoctor = api.workspaceDoctor;
  let calls = 0;
  api.workspaceDoctor = async () => {
    calls += 1;
    return doctorPayload();
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DoctorPage live={true} mode="live" refreshKey="tick-1" />);
    });
    await flush();

    expect(calls).toBe(1);
    expect(markup(renderer!).includes("pending_queue_warnings")).toBe(true);
    expect(markup(renderer!).includes("pending queue has malformed or ignored entries")).toBe(true);
    expect(markup(renderer!).includes("inspect state/pending_ops.jsonl")).toBe(true);
    expect(markup(renderer!).includes("agent_skill_inventory")).toBe(true);
    expect(markup(renderer!).includes("/tmp/home/.claude/skills")).toBe(true);
    expect(markup(renderer!).includes("present")).toBe(true);
    expect(markup(renderer!).includes("missing")).toBe(true);
    expect(markup(renderer!).includes("observed")).toBe(true);
    expect(markup(renderer!).includes("target_claude_claude_skills")).toBe(true);

    await act(async () => {
      renderer!.update(<DoctorPage live={true} mode="live" refreshKey="tick-2" />);
    });
    await flush();

    expect(calls).toBe(2);
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("DoctorPage skips live fetches while offline", async () => {
  const originalDoctor = api.workspaceDoctor;
  let calls = 0;
  api.workspaceDoctor = async () => {
    calls += 1;
    return doctorPayload();
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DoctorPage live={false} mode="offline-empty" refreshKey={null} />);
    });
    await flush();

    expect(calls).toBe(0);
    expect(markup(renderer!).includes("Doctor needs the live panel API.")).toBe(true);
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("TargetsPage refetches selected target details when a panel mutation completes", async () => {
  const originalTargetShow = api.targetShow;
  const targetShowCalls: string[] = [];
  let detailRevision = 0;

  api.targetShow = async (id: string) => {
    targetShowCalls.push(id);
    return targetPayload(detailRevision);
  };

  try {
    const props = {
      targets: [makeTarget()],
      skills: [makeSkill()],
      selectedTarget: "target-1",
      onSelectTarget: () => {},
      onRemoveTarget: () => {},
      onMutation: () => {},
      readOnly: false,
    };

    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<TargetsPage {...props} mutationVersion={0} />);
    });
    await flush();

    expect(targetShowCalls.length).toBe(1);
    expect(markup(renderer!).includes("No projections realized yet.")).toBe(true);

    detailRevision = 1;
    await act(async () => {
      renderer!.update(<TargetsPage {...props} mutationVersion={1} />);
    });
    await flush();

    expect(targetShowCalls.length).toBe(2);
    expect(markup(renderer!).includes("No projections realized yet.")).toBe(false);
    expect(markup(renderer!).includes("deadbeef")).toBe(true);
  } finally {
    api.targetShow = originalTargetShow;
  }
});

test("TargetsPage keeps a newer selection when a previous target delete completes", async () => {
  const originalTargetShow = api.targetShow;
  const originalTargetRemove = api.targetRemove;
  let resolveRemove: ((value: { ok: true; cmd: string; request_id: string }) => void) | null = null;

  api.targetShow = async () => targetPayload();
  api.targetRemove = async () =>
    new Promise((resolve) => {
      resolveRemove = resolve;
    });

  try {
    function Harness() {
      const [selectedTarget, setSelectedTarget] = React.useState<string | null>("target-1");
      return (
        <TargetsPage
          targets={[
            makeTarget(),
            makeTarget({
              id: "target-2",
              agent: "codex",
              profile: "work",
              path: "~/.codex/skills",
            }),
          ]}
          skills={[]}
          selectedTarget={selectedTarget}
          onSelectTarget={(id) => {
            setSelectedTarget((cur) => (cur === id ? null : id));
          }}
          onRemoveTarget={(id) => {
            setSelectedTarget((cur) => (cur === id ? null : cur));
          }}
          onMutation={() => {}}
          readOnly={false}
          mutationVersion={0}
        />
      );
    }

    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<Harness />);
    });
    await flush();

    await act(async () => {
      buttonByLabel(renderer!, "Delete target").props.onClick();
      await Promise.resolve();
    });

    const targetTwoCard = renderer!.root.findAll(
      (node: ReactTestInstance) =>
        node.type === "div" &&
        typeof node.props.onClick === "function" &&
        textOf(node.props.children).includes("codex"),
    )[0];
    await act(async () => {
      targetTwoCard.props.onClick();
    });

    await act(async () => {
      resolveRemove?.({ ok: true, cmd: "target remove", request_id: "req-1" });
      await Promise.resolve();
      await Promise.resolve();
    });

    const selectedCards = renderer!.root.findAll(
      (node: ReactTestInstance) => node.type === "div" && node.props.style?.borderColor === "var(--accent)",
    );
    expect(selectedCards.length).toBe(1);
    expect(textOf(selectedCards[0].props.children).includes("codex")).toBe(true);
  } finally {
    api.targetShow = originalTargetShow;
    api.targetRemove = originalTargetRemove;
  }
});

test("BindingsPage keeps a newer selection when a previous binding delete completes", async () => {
  const originalBindingShow = api.bindingShow;
  const originalBindingRemove = api.bindingRemove;
  let resolveRemove: ((value: { ok: true; cmd: string; request_id: string }) => void) | null = null;

  api.bindingShow = async () => bindingPayload(0);
  api.bindingRemove = async () =>
    new Promise((resolve) => {
      resolveRemove = resolve;
    });

  try {
    function Harness() {
      return (
        <BindingsPage
          bindings={[
            makeBinding(),
            {
              id: "binding-2",
              skill: "skill.reader",
              target: "target-1",
              matcher: "path_prefix:/other",
              method: "copy",
              policy: "manual",
            },
          ]}
          targets={[makeTarget()]}
          readOnly={false}
          mutationVersion={0}
          onMutation={() => {}}
        />
      );
    }

    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<Harness />);
    });
    await act(async () => {
      clickableRows(renderer!)[0]?.props.onClick();
    });
    await flush();

    await act(async () => {
      buttonByLabel(renderer!, "Delete binding").props.onClick();
      await Promise.resolve();
    });

    await act(async () => {
      clickableRows(renderer!)[1]?.props.onClick();
    });
    await flush();

    await act(async () => {
      resolveRemove?.({ ok: true, cmd: "binding remove", request_id: "req-1" });
      await Promise.resolve();
      await Promise.resolve();
    });

    const dpathDivs = renderer!.root.findAll(
      (node: ReactTestInstance) => node.type === "div" && node.props.className === "dpath",
    );
    expect(dpathDivs.length).toBe(1);
    expect(textOf(dpathDivs[0].props.children).includes("skill.reader → target-1")).toBe(true);
    expect(() => buttonByLabel(renderer!, "Delete binding")).not.toThrow();
    expect(markup(renderer!).includes("Select a binding to inspect")).toBe(false);
  } finally {
    api.bindingShow = originalBindingShow;
    api.bindingRemove = originalBindingRemove;
  }
});

test("BindingsPage skips live detail fetches in read-only mode", async () => {
  const originalBindingShow = api.bindingShow;
  let calls = 0;
  api.bindingShow = async () => {
    calls += 1;
    return bindingPayload(0);
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BindingsPage
          bindings={[makeBinding()]}
          targets={[makeTarget()]}
          readOnly={true}
          mutationVersion={0}
          onMutation={() => {}}
        />,
      );
    });
    await act(async () => {
      clickableRows(renderer!)[0]?.props.onClick();
    });
    await flush();

    expect(calls).toBe(0);
    expect(markup(renderer!).includes("Registry offline. Start with")).toBe(true);
  } finally {
    api.bindingShow = originalBindingShow;
  }
});

test("TargetsPage skips live detail fetches in read-only mode", async () => {
  const originalTargetShow = api.targetShow;
  let calls = 0;
  api.targetShow = async () => {
    calls += 1;
    return targetPayload();
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <TargetsPage
          targets={[makeTarget()]}
          skills={[makeSkill()]}
          selectedTarget="target-1"
          onSelectTarget={() => {}}
          onRemoveTarget={() => {}}
          onMutation={() => {}}
          readOnly={true}
          mutationVersion={0}
        />,
      );
    });
    await flush();

    expect(calls).toBe(0);
    expect(markup(renderer!).includes("Registry offline. Start with")).toBe(true);
  } finally {
    api.targetShow = originalTargetShow;
  }
});

test("HistoryPage skips live activity fetches when offline", async () => {
  const originalOps = api.ops;
  let calls = 0;
  api.ops = async () => {
    calls += 1;
    return opsPayload(makeOperation("pending"));
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HistoryPage live={false} mode="offline-empty" mutationVersion={0} />);
    });
    await flush();

    expect(calls).toBe(0);
    expect(markup(renderer!).includes("Activity history needs the live panel API.")).toBe(true);
  } finally {
    api.ops = originalOps;
  }
});

test("SettingsPage skips live info fetches when offline", async () => {
  const originalInfo = api.info;
  let calls = 0;
  api.info = async () => {
    calls += 1;
    return {
      root: "/tmp/loom",
      state_dir: "/tmp/loom/.loom",
      registry_targets_file: "/tmp/loom/.loom/registry/targets.json",
      claude_dir: "/tmp/loom/.claude",
      codex_dir: "/tmp/loom/.codex",
      remote_url: "git@example.com:loom.git",
    };
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<SettingsPage live={false} mode="offline-empty" registryRoot={null} />);
    });
    await flush();

    expect(calls).toBe(0);
    expect(markup(renderer!).includes("Settings need the live panel API.")).toBe(true);
  } finally {
    api.info = originalInfo;
  }
});

test("SettingsPage renders all live agent directories", async () => {
  const originalInfo = api.info;
  api.info = async () => ({
    root: "/tmp/loom",
    state_dir: "/tmp/loom/.loom",
    registry_targets_file: "/tmp/loom/.loom/registry/targets.json",
    agent_dirs: [
      { agent: "claude", env_var: "CLAUDE_SKILLS_DIR", path: "/tmp/home/.claude/skills" },
      { agent: "codex", env_var: "CODEX_SKILLS_DIR", path: "/tmp/home/.codex/skills" },
      { agent: "cursor", env_var: "CURSOR_SKILLS_DIR", path: "/tmp/home/.cursor/skills" },
      { agent: "windsurf", env_var: "WINDSURF_SKILLS_DIR", path: "/tmp/home/.windsurf/skills" },
      { agent: "cline", env_var: "CLINE_SKILLS_DIR", path: "/tmp/home/.cline/skills" },
      { agent: "copilot", env_var: "COPILOT_SKILLS_DIR", path: "/tmp/home/.github/copilot/skills" },
      { agent: "aider", env_var: "AIDER_SKILLS_DIR", path: "/tmp/home/.aider/skills" },
      { agent: "opencode", env_var: "OPENCODE_SKILLS_DIR", path: "/tmp/home/.opencode/skills" },
      { agent: "gemini-cli", env_var: "GEMINI_CLI_SKILLS_DIR", path: "/tmp/home/.gemini/skills" },
      { agent: "goose", env_var: "GOOSE_SKILLS_DIR", path: "/tmp/home/.config/goose/skills" },
    ],
    remote_url: "git@example.com:loom.git",
  });

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<SettingsPage live={true} mode="live" registryRoot="/tmp/loom" />);
    });
    await flush();

    const html = markup(renderer!);
    expect(html.includes("Gemini CLI dir")).toBe(true);
    expect(html.includes("/tmp/home/.gemini/skills")).toBe(true);
    expect(html.includes("Goose dir")).toBe(true);
    expect(html.includes("/tmp/home/.config/goose/skills")).toBe(true);
  } finally {
    api.info = originalInfo;
  }
});
