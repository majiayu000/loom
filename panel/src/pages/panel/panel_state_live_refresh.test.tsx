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
import { SkillsPage } from "./SkillsPage";
import { BindingAddForm } from "../../components/panel/forms/BindingAddForm";
import { api, type BindingShowPayload, type CommandEnvelope, type DoctorPayload, type OpsPayload, type TargetShowPayload, type RegistryOperationRecord } from "../../lib/api/client";
import type { Binding, Skill, Target } from "../../lib/types";
import type { RegistryProjection } from "../../generated/RegistryProjection";

import { bindingPayload, buttonByLabel, clickableRows, doctorPayload, flush, makeBinding, makeOperation, makeOrphanProjection, makeSkill, makeTarget, markup, opsPayload, targetPayload, textOf } from "./panel_state_test_utils";
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
      renderer = create(<DoctorPage apiReachable={true} mode="live" refreshKey="tick-1" />);
    });
    await flush();

    expect(calls).toBe(1);
    expect(markup(renderer!).includes("operation_journal_warnings")).toBe(true);
    expect(markup(renderer!).includes("operation journal has malformed or ignored entries")).toBe(true);
    expect(markup(renderer!).includes("inspect state/registry/ops/operations.jsonl")).toBe(true);
    expect(markup(renderer!).includes("agent_skill_inventory")).toBe(true);
    expect(markup(renderer!).includes("/tmp/home/.claude/skills")).toBe(true);
    expect(markup(renderer!).includes("present")).toBe(true);
    expect(markup(renderer!).includes("missing")).toBe(true);
    expect(markup(renderer!).includes("observed")).toBe(true);
    expect(markup(renderer!).includes("target_claude_claude_skills")).toBe(true);

    await act(async () => {
      renderer!.update(<DoctorPage apiReachable={true} mode="live" refreshKey="tick-2" />);
    });
    await flush();

    expect(calls).toBe(2);
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("DoctorPage skips doctor fetches when the panel API is unreachable", async () => {
  const originalDoctor = api.workspaceDoctor;
  let calls = 0;
  api.workspaceDoctor = async () => {
    calls += 1;
    return doctorPayload();
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DoctorPage apiReachable={false} mode="offline-empty" refreshKey={null} />);
    });
    await flush();

    expect(calls).toBe(0);
    expect(markup(renderer!).includes("Doctor needs the live panel API.")).toBe(true);
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("DoctorPage still fetches doctor diagnostics when registry data is degraded", async () => {
  const originalDoctor = api.workspaceDoctor;
  let calls = 0;
  api.workspaceDoctor = async () => {
    calls += 1;
    return doctorPayload();
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DoctorPage apiReachable={true} mode="offline-empty" refreshKey={null} />);
    });
    await flush();

    expect(calls).toBe(1);
    expect(markup(renderer!).includes("Doctor needs the live panel API.")).toBe(false);
    expect(markup(renderer!).includes("operation_journal_warnings")).toBe(true);
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("DoctorPage refreshes diagnostics when registry data degrades later", async () => {
  const originalDoctor = api.workspaceDoctor;
  let calls = 0;
  api.workspaceDoctor = async () => {
    calls += 1;
    return doctorPayload();
  };

  try {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DoctorPage apiReachable={true} mode="live" refreshKey="tick-1" />);
    });
    await flush();

    expect(calls).toBe(1);

    await act(async () => {
      renderer!.update(<DoctorPage apiReachable={true} mode="offline-stale" refreshKey="tick-1" />);
    });
    await flush();

    expect(calls).toBe(2);
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
