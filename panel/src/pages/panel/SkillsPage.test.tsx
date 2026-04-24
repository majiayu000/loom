import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SkillsPage } from "./SkillsPage";
import type { Skill } from "../../lib/types";

vi.mock("../../lib/api/client", () => ({
  api: {
    skillHistory: vi.fn(),
    skillDiff: vi.fn(),
    capture: vi.fn(),
  },
}));

// Import after mock registration so we get the mocked version.
// eslint-disable-next-line import/first
import { api } from "../../lib/api/client";

const mockSkill: Skill = {
  id: "skill-1",
  name: "my-skill",
  tag: "latest",
  latestRev: "abc12345",
  ruleCount: 2,
  changed: "1h ago",
  targets: [],
};

function renderPage() {
  return render(
    <SkillsPage
      skills={[mockSkill]}
      targets={[]}
      selectedSkill="skill-1"
      onSelectSkill={() => {}}
      onMutation={() => {}}
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
