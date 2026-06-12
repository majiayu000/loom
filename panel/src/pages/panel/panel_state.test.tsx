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

test("table-heavy panel pages expose mobile card row labels", () => {
  const skillHtml = renderToStaticMarkup(
    <SkillsPage
      skills={[makeSkill()]}
      targets={[makeTarget()]}
      bindings={[makeBinding()]}
      selectedSkill="s-skill.writer"
      onSelectSkill={() => {}}
      onMutation={() => {}}
      readOnly={false}
    />,
  );
  expect(skillHtml).toContain('class="tbl mobile-cards"');
  expect(skillHtml).toContain('data-label="Latest rev"');
  expect(skillHtml).toContain('data-label="Bindings"');
  expect(skillHtml).toContain('data-label="Tags"');

  const bindingHtml = renderToStaticMarkup(
    <BindingsPage
      bindings={[makeBinding()]}
      targets={[makeTarget()]}
      readOnly={false}
      mutationVersion={0}
      onMutation={() => {}}
    />,
  );
  expect(bindingHtml).toContain('class="tbl mobile-cards"');
  expect(bindingHtml).toContain('data-label="Matcher"');
  expect(bindingHtml).toContain('data-label="Policy"');

  const projectionHtml = renderToStaticMarkup(
    <ProjectionsPage
      projections={[makeOrphanProjection()]}
      targets={[makeTarget()]}
      bindings={[makeBinding()]}
      readOnly={false}
      onMutation={() => {}}
    />,
  );
  expect(projectionHtml).toContain('class="tbl mobile-cards"');
  expect(projectionHtml).toContain('data-label="Instance"');
  expect(projectionHtml).toContain('data-label="Health"');

  const settingsHtml = renderToStaticMarkup(
    <SettingsPage live={false} mode="offline-empty" registryRoot="/tmp/loom" />,
  );
  expect(settingsHtml).toContain('class="tbl mobile-cards"');
  expect(settingsHtml).toContain('data-label="Setting"');
  expect(settingsHtml).toContain('data-label="Value"');
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
        bindings={[]}
        ops={[]}
        projections={[]}
        registryProjections={[]}
        remoteState="CLEAN"
        queuedWriteCount={0}
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

test("OverviewPage distinguishes observed imports from live autosave", async () => {
  const observedSkill: Skill = {
    ...makeSkill(),
    id: "s-agentsmd-audit",
    name: "agentsmd-audit",
    tag: "skill",
    releaseTags: [],
    latestRev: "—",
    ruleCount: 0,
    bindingCount: 0,
    projectionCount: 0,
    changed: "—",
    targets: [],
    observedImported: true,
    sources: ["observed", "source"],
  };
  let renderer: ReactTestRenderer;
  await act(async () => {
    renderer = create(
      <OverviewPage
        skills={[
          observedSkill,
          {
            ...observedSkill,
            id: "s-ai-slop-cleaner",
            name: "ai-slop-cleaner",
          },
        ]}
        targets={[makeTarget({ ownership: "observed" })]}
        bindings={[]}
        ops={[]}
        projections={[]}
        registryProjections={[]}
        remoteState="CLEAN"
        queuedWriteCount={0}
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
        onOpenSkills={() => {}}
        onViewActivity={() => {}}
        onOpenSync={() => {}}
        readOnly={false}
      />,
    );
  });

  const html = markup(renderer!);
  expect(html.includes("2 skills in registry")).toBe(true);
  expect(html.includes("2 imported from observed targets")).toBe(true);
  expect(html.includes("imported · no bindings")).toBe(true);
  expect(html.includes("No tracked skills yet.")).toBe(false);
  expect(html.includes("Observed targets are read-only imports")).toBe(true);
  expect(html.includes("loom skill monitor-observed")).toBe(true);
  expect(html.includes("loom skill watch")).toBe(true);
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
