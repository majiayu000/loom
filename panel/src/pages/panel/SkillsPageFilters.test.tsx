import { useState } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { Binding, Skill, Target } from "../../lib/types";
import { SkillsPage } from "./SkillsPage";

vi.mock("../../lib/api/client", () => ({
  api: {
    skillHistory: vi.fn(),
    skillDiagnose: vi.fn(),
    skillInspect: vi.fn(),
    skillDiff: vi.fn(),
    commitProjection: vi.fn(),
    skillCommit: vi.fn(),
    skillReleaseAnchor: vi.fn(),
    skillRelease: vi.fn(),
    skillRollback: vi.fn(),
    skillTrashList: vi.fn(),
    skillTrashAdd: vi.fn(),
    skillTrashRestore: vi.fn(),
    skillTrashPurge: vi.fn(),
    project: vi.fn(),
  },
}));

// Import after mock registration so we get the mocked version.
// eslint-disable-next-line import/first
import { api } from "../../lib/api/client";

const baseSkill: Skill = {
  id: "ready",
  name: "ready-skill",
  description: "Ready skill",
  tag: "latest",
  sourceStatus: "present",
  releaseTags: ["v1"],
  snapshotTags: [],
  latestRev: "abc12345",
  ruleCount: 1,
  bindingCount: 1,
  projectionCount: 1,
  changed: "1h ago",
  targets: ["target-1"],
};

const target: Target = {
  id: "target-1",
  agent: "claude",
  profile: "home",
  path: "~/.claude/skills",
  ownership: "managed",
  skills: 1,
  lastSync: "now",
};

const binding: Binding = {
  id: "binding-1",
  skill: "ready-skill",
  target: "target-1",
  matcher: "path_prefix:/repo",
  method: "copy",
  policy: "auto",
};

const projection: RegistryProjection = {
  instance_id: "projection-1",
  skill_id: "ready-skill",
  binding_id: "binding-1",
  target_id: "target-1",
  materialized_path: "/tmp/target/ready-skill",
  method: "copy",
  last_applied_rev: "abc12345",
  health: "healthy",
};

function makeInspect(skill = "ready-skill") {
  return {
    skill,
    source: {
      path: `/tmp/registry/skills/${skill}`,
      exists: true,
      entrypoint: "SKILL.md",
      entrypoint_exists: true,
      working_tree_drift: false,
      head_tree_oid: "tree123",
      last_source_commit: "abc12345",
      drifted_paths: [],
    },
    spec: { portable: "pass", codex: "pass", claude: "pass", findings: [] },
    runtime: {},
    dependencies: null,
    quality: { last_eval: null, trigger_precision: null, trigger_recall: null, baseline_delta: null },
    safety: {
      trust: "unknown",
      policy: "unknown",
      scripts_present: null,
      network_requested: null,
      quarantined: false,
      reason: null,
      updated_at: null,
    },
    next_actions: [`loom skill eval ${skill}`],
  };
}

function renderFixture() {
  const attentionSkill: Skill = {
    ...baseSkill,
    id: "attention",
    name: "attention-skill",
    sourceStatus: "missing",
    releaseTags: [],
    bindingCount: 0,
    projectionCount: 0,
    targets: [],
  };
  render(
    <SkillsPage
      skills={[baseSkill, attentionSkill]}
      targets={[target]}
      bindings={[binding]}
      projections={[projection]}
      selectedSkill="ready"
      onSelectSkill={() => {}}
      onMutation={() => {}}
      readOnly={false}
    />,
  );
}

describe("SkillsPage filters and detail tabs", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    (api.skillHistory as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      data: { skill: "ready-skill", count: 0, events: [] },
    });
    (api.skillInspect as ReturnType<typeof vi.fn>).mockImplementation((name: string) =>
      Promise.resolve(makeInspect(name)),
    );
  });

  it("filters by source status, target, attention state, and tags", () => {
    renderFixture();
    const list = () => within(screen.getByRole("table"));

    fireEvent.change(screen.getByLabelText("Source status filter"), { target: { value: "missing" } });
    expect(list().getByText("attention-skill")).toBeInTheDocument();
    expect(list().queryByText("ready-skill")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Source status filter"), { target: { value: "all" } });
    fireEvent.change(screen.getByLabelText("Target filter"), { target: { value: "target-1" } });
    expect(list().getByText("ready-skill")).toBeInTheDocument();
    expect(list().queryByText("attention-skill")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Target filter"), { target: { value: "all" } });
    fireEvent.change(screen.getByLabelText("Attention filter"), { target: { value: "attention" } });
    expect(list().getByText("attention-skill")).toBeInTheDocument();
    expect(list().queryByText("ready-skill")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Attention filter"), { target: { value: "all" } });
    fireEvent.change(screen.getByLabelText("Tag filter"), { target: { value: "tagged" } });
    expect(list().getByText("ready-skill")).toBeInTheDocument();
    expect(list().queryByText("attention-skill")).not.toBeInTheDocument();
  });

  it("shows filter empty state and clears non-query filters", () => {
    renderFixture();

    fireEvent.change(screen.getByLabelText("Source status filter"), { target: { value: "non-compliant" } });

    expect(screen.getByText("No matching skills")).toBeInTheDocument();
    expect(screen.getByText("No skills match the selected filters.")).toBeInTheDocument();
    expect(screen.queryByText("No tracked skills yet")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Clear filter" }));

    expect((screen.getByLabelText("Source status filter") as HTMLSelectElement).value).toBe("all");
    expect(within(screen.getByRole("table")).getByText("ready-skill")).toBeInTheDocument();
    expect(within(screen.getByRole("table")).getByText("attention-skill")).toBeInTheDocument();
  });

  it("renders real detail tabs without source or files tabs", () => {
    renderFixture();
    expect(screen.queryByRole("button", { name: "Source" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Files" })).not.toBeInTheDocument();

    for (const name of ["Overview", "Lifecycle", "Diagnose", "History", "Diff", "Projections (1)", "Trash state"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }

    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    expect(screen.getByText("healthy 1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Projections (1)" }));
    expect(screen.getByText("/tmp/target/ready-skill")).toBeInTheDocument();
    expect(screen.getByText("copy · abc12345")).toBeInTheDocument();
  });

  it("projects with the binding for the currently selected skill", async () => {
    const secondSkill: Skill = {
      ...baseSkill,
      id: "second",
      name: "second-skill",
      description: "Second skill",
      latestRev: "def67890",
      targets: ["target-2"],
    };
    const secondTarget: Target = { ...target, id: "target-2", agent: "codex", profile: "work" };
    const secondBinding: Binding = { ...binding, id: "binding-2", skill: "second-skill", target: "target-2" };
    const secondProjection: RegistryProjection = {
      ...projection,
      instance_id: "projection-2",
      skill_id: "second-skill",
      binding_id: "binding-2",
      target_id: "target-2",
      materialized_path: "/tmp/target/second-skill",
      last_applied_rev: "def67890",
    };
    function Harness() {
      const [selectedSkill, setSelectedSkill] = useState<string | null>("ready");
      return (
        <SkillsPage
          skills={[baseSkill, secondSkill]}
          targets={[target, secondTarget]}
          bindings={[binding, secondBinding]}
          projections={[projection, secondProjection]}
          selectedSkill={selectedSkill}
          onSelectSkill={setSelectedSkill}
          onMutation={() => {}}
          readOnly={false}
        />
      );
    }
    (api.project as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true });

    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Projections (1)" }));
    fireEvent.click(within(screen.getByRole("table")).getByText("second-skill"));

    await waitFor(() => expect(screen.getByDisplayValue(/binding-2/)).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Project" }));

    await waitFor(() =>
      expect(api.project).toHaveBeenCalledWith({ skill: "second-skill", binding: "binding-2", method: "symlink" }),
    );
  });
});
