import React from "react";
import { afterAll, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { LiveDataBanner } from "../PanelApp";
import { BindingsPage } from "./BindingsPage";
import { HistoryPage, bucket } from "./HistoryPage";
import { TargetsPage } from "./TargetsPage";
import { SettingsPage } from "./SettingsPage";
import { OverviewPage } from "./OverviewPage";
import { api, type BindingShowPayload, type OpsPayload, type TargetShowPayload, type V3OperationRecord } from "../../lib/api/client";
import type { Binding, Skill, Target } from "../../lib/types";

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

function makeOperation(status: string, ack = false, opId = "op_123", intent = "skill.project"): V3OperationRecord {
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
    matcher: "path-prefix:/repo",
    method: "symlink",
    policy: "auto",
  };
}

function makeSkill(): Skill {
  return {
    id: "s-skill.writer",
    name: "skill.writer",
    tag: "v1",
    latestRev: "abc12345",
    ruleCount: 1,
    changed: "now",
    targets: ["target-1"],
  };
}

function bindingPayload(projectionCount: number): BindingShowPayload {
  return {
    ok: true,
    data: {
      state_model: "v3",
      binding: {
        binding_id: "binding-1",
        agent: "claude",
        profile_id: "home",
        workspace_matcher: { kind: "path-prefix", value: "/repo" },
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
      state_model: "v3",
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

function opsPayload(operation: V3OperationRecord): OpsPayload {
  return {
    ok: true,
    data: {
      count: 1,
      operations: [operation],
      checkpoint: { last_scanned_op_id: operation.op_id },
    },
  };
}

test("HistoryPage treats succeeded operations as successful", () => {
  expect(bucket(makeOperation("succeeded", false))).toBe("ok");
});

test("LiveDataBanner renders nothing during live refetch loading", () => {
  const html = renderToStaticMarkup(<LiveDataBanner error={null} loading={true} mode="live" />);
  expect(html).toBe("");
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
      v3_targets_file: "/tmp/loom/.loom/v3/targets.json",
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
