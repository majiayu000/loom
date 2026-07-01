import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SkillsPage } from "./SkillsPage";
import type { Binding, Skill, Target } from "../../lib/types";
import type { RegistryProjection } from "../../generated/RegistryProjection";

vi.mock("../../lib/api/client", () => ({
  api: {
    skillHistory: vi.fn(),
    skillDiagnose: vi.fn(),
    skillInspect: vi.fn(),
    skillDiff: vi.fn(),
    capture: vi.fn(),
    skillSave: vi.fn(),
    skillSnapshot: vi.fn(),
    skillRelease: vi.fn(),
    skillRollback: vi.fn(),
    skillUse: vi.fn(),
    skillTrashList: vi.fn(),
    skillTrashAdd: vi.fn(),
    skillTrashRestore: vi.fn(),
    skillTrashPurge: vi.fn(),
  },
}));

// Import after mock registration so we get the mocked version.
// eslint-disable-next-line import/first
import { api } from "../../lib/api/client";

const mockSkill: Skill = {
  id: "skill-1",
  name: "my-skill",
  description: "Runs a focused workflow for skill registry changes.",
  tag: "latest",
  sourceStatus: "present",
  releaseTags: [],
  snapshotTags: [],
  latestRev: "abc12345",
  ruleCount: 2,
  bindingCount: 2,
  projectionCount: 0,
  changed: "1h ago",
  targets: [],
};

const mockBinding: Binding = {
  id: "binding-1",
  skill: "my-skill",
  target: "target-1",
  matcher: "path_prefix:/repo",
  method: "copy",
  policy: "auto",
};

type SkillsPageOverrides = {
  onMutation?: () => void;
  bindings?: Binding[];
  targets?: Target[];
  projections?: RegistryProjection[];
  readOnly?: boolean;
  onSelectSkill?: (id: string | null) => void;
};

function skillsPage(overrides: SkillsPageOverrides = {}) {
  (api.skillInspect as ReturnType<typeof vi.fn>).mockResolvedValue(makeInspect());
  return (
    <SkillsPage
      skills={[mockSkill]}
      targets={overrides.targets ?? []}
      bindings={overrides.bindings ?? []}
      projections={overrides.projections ?? []}
      selectedSkill="skill-1"
      onSelectSkill={overrides.onSelectSkill ?? (() => {})}
      onMutation={overrides.onMutation ?? (() => {})}
      readOnly={overrides.readOnly ?? false}
    />
  );
}

function renderPage(overrides: SkillsPageOverrides = {}) {
  return render(skillsPage(overrides));
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

function makeSkill(overrides: Partial<Skill> = {}): Skill {
  return {
    ...mockSkill,
    ...overrides,
  };
}

function makeDiagnose(overrides: Record<string, unknown> = {}) {
  return {
    skill: "my-skill",
    healthy: false,
    status: "blocked",
    summary: {
      source_status: "missing",
      binding_count: 0,
      target_count: 0,
      projection_count: 0,
      failed_check_count: 1,
      warning_check_count: 1,
      drifted_path_count: 0,
      recent_failed_op_count: 0,
    },
    checks: [
      {
        section: "source",
        id: "source_directory_exists",
        ok: false,
        severity: "error",
        message: "source skill directory is missing",
        next_action: "restore the source skill, re-add it, or clean orphaned references",
        details: { path: "/tmp/skills/my-skill" },
      },
      {
        section: "git",
        id: "source_drift",
        ok: false,
        severity: "warning",
        message: "source has unsaved drift",
        next_action: "run loom skill save my-skill or inspect loom skill diff",
        details: { drifted_paths: ["SKILL.md"] },
      },
      {
        section: "operations",
        id: "recent_failed_ops",
        ok: true,
        severity: "ok",
        message: "no recent failed operations for this skill",
        next_action: null,
        details: { operations: [] },
      },
    ],
    related: {},
    ...overrides,
  };
}

function makeInspect(overrides: Record<string, unknown> = {}) {
  return {
    skill: "my-skill",
    source: {
      path: "/tmp/registry/skills/my-skill",
      exists: true,
      entrypoint: "SKILL.md",
      entrypoint_exists: true,
      working_tree_drift: false,
      head_tree_oid: "tree123",
      last_source_commit: "abc12345",
      drifted_paths: [],
    },
    spec: {
      portable: "pass",
      codex: "pass",
      claude: "pass",
      findings: [],
    },
    provenance: {
      source: null,
      pinned_ref: null,
      verified: null,
      drift: null,
    },
    runtime: {
      claude: {
        installed_in_registry: true,
        active_rule_present: false,
        projected_to_target: false,
        materialized_path_exists: null,
        visible_to_agent: "not_checked",
        enabled_by_agent_config: "not_checked",
        restart_required: "not_checked",
        target_id: null,
        binding_id: null,
        target_path: null,
        materialized_path: null,
        health: null,
        truth_level: "source_only",
        findings: [],
      },
    },
    dependencies: null,
    quality: {
      last_eval: null,
      trigger_precision: null,
      trigger_recall: null,
      baseline_delta: null,
    },
    safety: {
      trust: "unknown",
      policy: "unknown",
      scripts_present: null,
      network_requested: null,
      quarantined: false,
      reason: null,
      updated_at: null,
    },
    next_actions: ["loom skill eval my-skill", "loom skill policy my-skill"],
    ...overrides,
  };
}

function makeTrashEntry(overrides: Record<string, unknown> = {}) {
  return {
    trash_id: "old-skill-20260604T010203Z-a1b2c3d4",
    skill: "old-skill",
    original_path: "skills/old-skill",
    trashed_at: "2026-06-04T01:02:03Z",
    source_commit: "abcdef1234567890",
    trash_path: "trash/old-skill-20260604T010203Z-a1b2c3d4",
    ...overrides,
  };
}

describe("SkillsPage — use flow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    (api.skillUse as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      cmd: "use",
      request_id: "req-use",
      data: { steps: [{}] },
    });
  });

  it("plans selected skill use from the projections tab", async () => {
    renderPage({
      targets: [makeTarget({ agent: "codex", profile: "work" })],
    });

    fireEvent.click(screen.getByRole("button", { name: "Projections (0)" }));
    fireEvent.change(screen.getByPlaceholderText("workspace path"), { target: { value: "/repo/a" } });
    fireEvent.click(screen.getByRole("button", { name: "Plan" }));

    await waitFor(() => {
      expect(api.skillUse).toHaveBeenCalledWith("my-skill", {
        agents: ["codex"],
        scope: "project",
        workspace: "/repo/a",
        profile: "default",
        method: "symlink",
        apply: false,
      });
    });
    expect(await screen.findByText("1 agent planned")).toBeInTheDocument();
  });

  it("applies selected skill use and refreshes panel data", async () => {
    const onMutation = vi.fn();
    renderPage({ onMutation });

    fireEvent.click(screen.getByRole("button", { name: "Projections (0)" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => {
      expect(api.skillUse).toHaveBeenCalledWith("my-skill", {
        agents: ["claude"],
        scope: "project",
        workspace: undefined,
        profile: "default",
        method: "symlink",
        apply: true,
      });
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });
});

describe("SkillsPage — capture action", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    (api.capture as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      cmd: "skill.capture",
      request_id: "req-capture",
    });
  });

  it("sends a binding selector for a skill with one projected binding", async () => {
    const onMutation = vi.fn();
    renderPage({ bindings: [mockBinding], onMutation });

    fireEvent.click(screen.getByRole("button", { name: "Capture" }));

    await waitFor(() => {
      expect(api.capture).toHaveBeenCalledWith({ skill: "my-skill", binding: "binding-1" });
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });

  it("lets users choose which binding to capture when a skill has multiple bindings", async () => {
    const onMutation = vi.fn();
    renderPage({
      targets: [
        makeTarget(),
        makeTarget({ id: "target-2", agent: "codex", profile: "work", path: "~/.codex/skills" }),
      ],
      bindings: [
        mockBinding,
        { ...mockBinding, id: "binding-2", target: "target-2", method: "symlink", policy: "manual" },
      ],
      onMutation,
    });

    fireEvent.change(screen.getByLabelText("Capture binding"), { target: { value: "binding-2" } });
    fireEvent.click(screen.getByRole("button", { name: "Capture" }));

    await waitFor(() => {
      expect(api.capture).toHaveBeenCalledWith({ skill: "my-skill", binding: "binding-2" });
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });

  it("disables capture when the skill has no projected binding", () => {
    renderPage();

    expect(screen.getByRole("button", { name: "Capture" })).toBeDisabled();
  });

  it("derives capture choices from projections when bindings are shared across skills", async () => {
    const onMutation = vi.fn();
    const projection: RegistryProjection = {
      instance_id: "inst-1",
      skill_id: "my-skill",
      binding_id: "shared-binding",
      target_id: "target-1",
      materialized_path: "/tmp/target-1/my-skill",
      method: "copy",
      last_applied_rev: "abc12345",
      health: "healthy",
    };
    renderPage({
      bindings: [{ ...mockBinding, id: "shared-binding", skill: "other-skill" }],
      projections: [projection],
      onMutation,
    });

    fireEvent.click(screen.getByRole("button", { name: "Capture" }));

    await waitFor(() => {
      expect(api.capture).toHaveBeenCalledWith({ skill: "my-skill", binding: "shared-binding" });
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });
});

describe("SkillsPage — descriptions", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
  });

  it("shows skill descriptions in the list and detail panel", async () => {
    renderPage();

    expect(screen.getAllByText("Runs a focused workflow for skill registry changes.").length).toBeGreaterThan(1);
  });

  it("filters skills by description text", async () => {
    renderPage();

    fireEvent.change(screen.getByPlaceholderText("Filter skills…"), { target: { value: "registry changes" } });

    expect(screen.getAllByText("my-skill").length).toBeGreaterThan(0);
  });
});

describe("SkillsPage — history tab", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    // skillDiff is only rendered when its tab is active; keep it pending so
    // it doesn't interfere with history tab assertions.
    (api.skillDiff as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));
    (api.skillInspect as ReturnType<typeof vi.fn>).mockResolvedValue(makeInspect());
  });

  it("shows loading indicator while fetch is in-flight", () => {
    (api.skillHistory as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));
    renderPage();
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("shows error message when the fetch rejects", async () => {
    (api.skillHistory as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error("server unavailable"),
    );
    renderPage();
    await waitFor(() => {
      expect(screen.getByText("server unavailable")).toBeInTheDocument();
    });
  });

  it("shows empty-state prompt when the API returns zero events", async () => {
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    renderPage();
    await waitFor(() => {
      expect(screen.getByText(/No lifecycle events yet/)).toBeInTheDocument();
    });
  });

  it("renders file_changed events as 'save' and health_changed events as 'snapshot'", async () => {
    const now = new Date().toISOString();
    const earlier = new Date(Date.now() - 60_000).toISOString();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: {
        skill: "my-skill",
        count: 2,
        events: [
          {
            event_id: "aabbccdd-0001",
            instance_id: "inst-aabbccdd",
            kind: "file_changed",
            path: "SKILL.md",
            observed_at: now,
          },
          {
            event_id: "aabbccdd-0002",
            instance_id: "inst-aabbccdd",
            kind: "health_changed",
            from: "healthy",
            to: "drifted",
            observed_at: earlier,
          },
        ],
      },
    });
    renderPage();
    await waitFor(() => {
      expect(screen.getByText("save")).toBeInTheDocument();
      expect(screen.getByText("snapshot")).toBeInTheDocument();
    });
  });

  it("refetches history when the selected skill revision changes", async () => {
    (api.skillHistory as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce({
        ok: true,
        data: { skill: "my-skill", count: 0, events: [] },
      })
      .mockResolvedValueOnce({
        ok: true,
        data: {
          skill: "my-skill",
          count: 1,
          events: [
            {
              event_id: "rev-2-event",
              instance_id: "inst-aabbccdd",
              kind: "captured",
              path: "SKILL.md",
              observed_at: new Date().toISOString(),
            },
          ],
        },
      });

    const { rerender } = render(
      <SkillsPage
        skills={[makeSkill({ latestRev: "abc12345" })]}
        targets={[]}
        selectedSkill="skill-1"
        onSelectSkill={() => {}}
        onMutation={() => {}}
        readOnly={false}
      />,
    );

    await waitFor(() => {
      expect(api.skillHistory).toHaveBeenCalledTimes(1);
    });

    rerender(
      <SkillsPage
        skills={[makeSkill({ latestRev: "def67890" })]}
        targets={[]}
        selectedSkill="skill-1"
        onSelectSkill={() => {}}
        onMutation={() => {}}
        readOnly={false}
      />,
    );

    await waitFor(() => {
      expect(api.skillHistory).toHaveBeenCalledTimes(2);
    });
  });
});

describe("SkillsPage — diagnose tab", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    (api.skillDiff as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));
    (api.skillDiagnose as ReturnType<typeof vi.fn>).mockResolvedValue(makeDiagnose());
    (api.skillSave as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      cmd: "skill.save",
      request_id: "req-save",
    });
  });

  it("does not fetch diagnose data while Lifecycle remains active", async () => {
    renderPage();

    await waitFor(() => {
      expect(api.skillHistory).toHaveBeenCalledTimes(1);
    });
    expect(api.skillDiagnose).not.toHaveBeenCalled();
  });

  it("fetches diagnose data when clicked and renders blocked checks with next action", async () => {
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "Diagnose" }));

    await waitFor(() => {
      expect(api.skillDiagnose).toHaveBeenCalledWith("my-skill", expect.any(AbortSignal));
      expect(screen.getByText("blocked")).toBeInTheDocument();
      expect(screen.getByText("source_directory_exists")).toBeInTheDocument();
      expect(
        screen.getByText(/restore the source skill, re-add it, or clean orphaned references/),
      ).toBeInTheDocument();
    });
  });

  it("shows loading state while diagnose data is in-flight", () => {
    (api.skillDiagnose as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "Diagnose" }));

    expect(screen.getByText("Loading...")).toBeInTheDocument();
  });

  it("shows diagnose errors when the fetch rejects", async () => {
    (api.skillDiagnose as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error("diagnose unavailable"),
    );
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "Diagnose" }));

    await waitFor(() => {
      expect(screen.getByText("diagnose unavailable")).toBeInTheDocument();
    });
  });

  it("refetches diagnose data after lifecycle mutations while Diagnose is active", async () => {
    (api.skillDiagnose as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(makeDiagnose({ status: "blocked" }))
      .mockResolvedValueOnce(
        makeDiagnose({
          healthy: true,
          status: "healthy",
          summary: {
            source_status: "present",
            binding_count: 0,
            target_count: 0,
            projection_count: 0,
            failed_check_count: 0,
            warning_check_count: 0,
            drifted_path_count: 0,
            recent_failed_op_count: 0,
          },
          checks: [],
        }),
      );
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "Diagnose" }));
    await waitFor(() => {
      expect(screen.getByText("blocked")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(api.skillSave).toHaveBeenCalledWith("my-skill");
      expect(api.skillDiagnose).toHaveBeenCalledTimes(2);
      expect(screen.getByText("healthy")).toBeInTheDocument();
    });
  });
});

describe("SkillsPage — empty registry", () => {
  it("guides first-run users to the add button and the loom skill add CLI", () => {
    render(
      <SkillsPage
        skills={[]}
        targets={[]}
        selectedSkill={null}
        onSelectSkill={() => {}}
        onMutation={() => {}}
        readOnly={false}
      />,
    );

    // The CTA references the in-panel button by its label so users notice it.
    expect(screen.getAllByText(/\+ skill add/).length).toBeGreaterThan(0);
    // And it surfaces the equivalent CLI invocation for terminal-first users.
    expect(
      screen.getAllByText(/loom skill add <source> --name <name>/).length,
    ).toBeGreaterThan(0);
  });
});

describe("SkillsPage — trash UI", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    (api.skillTrashList as ReturnType<typeof vi.fn>).mockResolvedValue({
      items: [makeTrashEntry()],
    });
    (api.skillTrashAdd as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      cmd: "skill.trash.add",
      request_id: "req-trash-add",
    });
    (api.skillTrashRestore as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      cmd: "skill.trash.restore",
      request_id: "req-trash-restore",
    });
    (api.skillTrashPurge as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      cmd: "skill.trash.purge",
      request_id: "req-trash-purge",
    });
  });

  function openTrashView() {
    fireEvent.click(
      within(screen.getByRole("group", { name: "Skill view" })).getByRole("button", {
        name: "Trash",
      }),
    );
  }

  function openTrashStateTab() {
    fireEvent.click(screen.getByRole("button", { name: "Trash state" }));
  }

  it("lists trash entries in the Trash view", async () => {
    renderPage();
    openTrashView();

    await waitFor(() => {
      expect(api.skillTrashList).toHaveBeenCalledTimes(1);
      expect(screen.getAllByText("old-skill").length).toBeGreaterThan(0);
      expect(screen.getByText("trash/old-skill-20260604T010203Z-a1b2c3d4")).toBeInTheDocument();
    });
  });

  it("moves the selected skill to trash after confirmation", async () => {
    const onMutation = vi.fn();
    const onSelectSkill = vi.fn();
    renderPage({ onMutation, onSelectSkill });

    openTrashStateTab();
    fireEvent.click(screen.getByRole("button", { name: "Trash my-skill" }));
    fireEvent.click(screen.getByRole("button", { name: "Move to trash" }));

    await waitFor(() => {
      expect(api.skillTrashAdd).toHaveBeenCalledWith("my-skill");
      expect(onSelectSkill).toHaveBeenCalledWith(null);
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });

  it("keeps an open trash confirmation read-only safe", async () => {
    const view = renderPage();

    openTrashStateTab();
    fireEvent.click(screen.getByRole("button", { name: "Trash my-skill" }));
    expect(screen.getByRole("button", { name: "Move to trash" })).not.toBeDisabled();

    view.rerender(skillsPage({ readOnly: true }));
    const confirmTrash = screen.getByRole("button", { name: "Move to trash" });
    expect(confirmTrash).toBeDisabled();

    fireEvent.click(confirmTrash);
    expect(api.skillTrashAdd).not.toHaveBeenCalled();
  });

  it("restores the selected trash entry by trash id", async () => {
    const onMutation = vi.fn();
    renderPage({ onMutation });
    openTrashView();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    await waitFor(() => {
      expect(api.skillTrashRestore).toHaveBeenCalledWith(
        "old-skill-20260604T010203Z-a1b2c3d4",
        { skill: "old-skill" },
      );
      expect(onMutation).toHaveBeenCalledTimes(1);
    });
  });

  it("purges only after explicit confirmation", async () => {
    renderPage();
    openTrashView();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Purge" })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Purge" }));
    expect(api.skillTrashPurge).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Purge forever" }));

    await waitFor(() => {
      expect(api.skillTrashPurge).toHaveBeenCalledWith("old-skill-20260604T010203Z-a1b2c3d4");
    });
  });

  it("keeps an open purge confirmation read-only safe", async () => {
    const view = renderPage();
    openTrashView();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Purge" })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Purge" }));
    expect(screen.getByRole("button", { name: "Purge forever" })).not.toBeDisabled();

    view.rerender(skillsPage({ readOnly: true }));
    const confirmPurge = screen.getByRole("button", { name: "Purge forever" });
    expect(confirmPurge).toBeDisabled();

    fireEvent.click(confirmPurge);
    expect(api.skillTrashPurge).not.toHaveBeenCalled();
  });

  it("disables trash mutations in read-only mode", async () => {
    renderPage({ readOnly: true });
    openTrashStateTab();
    expect(screen.getByRole("button", { name: "Trash my-skill" })).toBeDisabled();

    openTrashView();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Restore" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Purge" })).toBeDisabled();
    });
  });
});

describe("SkillsPage — lifecycle actions", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    (api.skillSave as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true, cmd: "skill.save", request_id: "req-save" });
    (api.skillSnapshot as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true, cmd: "skill.snapshot", request_id: "req-snapshot" });
    (api.skillRelease as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true, cmd: "skill.release", request_id: "req-release" });
    (api.skillRollback as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true, cmd: "skill.rollback", request_id: "req-rollback" });
  });

  it("runs save, snapshot, release, and rollback for the selected skill", async () => {
    const onMutation = vi.fn();
    renderPage({ onMutation });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(api.skillSave).toHaveBeenCalledWith("my-skill");
      expect(screen.getByRole("button", { name: "Snapshot" })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole("button", { name: "Snapshot" }));
    await waitFor(() => {
      expect(api.skillSnapshot).toHaveBeenCalledWith("my-skill");
    });

    fireEvent.change(screen.getByPlaceholderText("version"), { target: { value: "v1.0.0" } });
    fireEvent.click(screen.getByRole("button", { name: "Release" }));
    await waitFor(() => {
      expect(api.skillRelease).toHaveBeenCalledWith("my-skill", { version: "v1.0.0" });
      expect(screen.getByRole("button", { name: "Rollback" })).not.toBeDisabled();
    });

    fireEvent.change(screen.getByPlaceholderText("HEAD~1"), { target: { value: "snapshot/my-skill/abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Rollback" }));

    await waitFor(() => {
      expect(api.skillRollback).toHaveBeenCalledWith("my-skill", { to: "snapshot/my-skill/abc" });
      expect(onMutation).toHaveBeenCalledTimes(4);
    });
  });

  it("refetches lifecycle history after successful lifecycle mutations", async () => {
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "my-skill", count: 0, events: [] },
    });
    renderPage();

    await waitFor(() => {
      expect(api.skillHistory).toHaveBeenCalledTimes(1);
    });

    fireEvent.change(screen.getByPlaceholderText("version"), { target: { value: "v1.2.3" } });
    fireEvent.click(screen.getByRole("button", { name: "Release" }));

    await waitFor(() => {
      expect(api.skillHistory).toHaveBeenCalledTimes(2);
    });
  });
});
