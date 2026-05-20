import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SkillsPage } from "./SkillsPage";
import type { Binding, Skill } from "../../lib/types";

vi.mock("../../lib/api/client", () => ({
  api: {
    skillHistory: vi.fn(),
    skillDiff: vi.fn(),
    capture: vi.fn(),
    skillSave: vi.fn(),
    skillSnapshot: vi.fn(),
    skillRelease: vi.fn(),
    skillRollback: vi.fn(),
  },
}));

// Import after mock registration so we get the mocked version.
// eslint-disable-next-line import/first
import { api } from "../../lib/api/client";

const mockSkill: Skill = {
  id: "skill-1",
  name: "my-skill",
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

const secondMockBinding: Binding = {
  id: "binding-2",
  skill: "my-skill",
  target: "target-2",
  matcher: "path_prefix:/repo/team",
  method: "materialize",
  policy: "manual",
};

function renderPage(overrides: { onMutation?: () => void; bindings?: Binding[] } = {}) {
  return render(
    <SkillsPage
      skills={[mockSkill]}
      targets={[]}
      bindings={overrides.bindings ?? []}
      selectedSkill="skill-1"
      onSelectSkill={() => {}}
      onMutation={overrides.onMutation ?? (() => {})}
      readOnly={false}
    />,
  );
}

function makeSkill(overrides: Partial<Skill> = {}): Skill {
  return {
    ...mockSkill,
    ...overrides,
  };
}

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

  it("lets users choose which binding to capture from", async () => {
    renderPage({ bindings: [mockBinding, secondMockBinding] });

    fireEvent.change(screen.getByLabelText("Capture binding"), {
      target: { value: "binding-2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Capture" }));

    await waitFor(() => {
      expect(api.capture).toHaveBeenCalledWith({ skill: "my-skill", binding: "binding-2" });
    });
  });

  it("disables capture when the skill has no projected binding selector", () => {
    renderPage();

    expect(screen.getByRole("button", { name: "Capture" })).toBeDisabled();
  });
});

describe("SkillsPage — history tab", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    // skillDiff is only rendered when its tab is active; keep it pending so
    // it doesn't interfere with history tab assertions.
    (api.skillDiff as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));
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
});
