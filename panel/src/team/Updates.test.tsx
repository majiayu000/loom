import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Updates } from "./Updates";
import * as client from "./client";
vi.mock("./client", async (original) => ({
  ...(await original<typeof client>()),
  isDesktop: () => true,
  native: vi.fn(),
}));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
const skill: client.Skill = {
  id: "cloud-skill",
  slug: "review",
  title: "Review",
  description: "Review code",
  example: "Try",
  maintainer_id: "alice",
  recommended_version_id: "v2",
  archived_at: null,
  revision: 1,
};
const skills = [skill];
const source = {
  service_origin: "https://cloud.example.test",
  team_id: "team-a",
  skill_id: "cloud-skill",
  version_id: "v1",
  requested_ref: "recommended",
};
const run = async (fn: () => Promise<void>) => fn();
function app(team = "team-a") {
  return (
    <Updates
      team={team}
      origin="https://cloud.example.test/"
      skills={skills}
      run={run}
      open={() => {}}
    />
  );
}
describe("installed team version comparison", () => {
  it("does not inspect discovered skills that have not been imported", async () => {
    vi.mocked(client.native).mockImplementation(async (name, args) => {
      if (name === "local_skills") return {
        ok: true, data: { skills: [
          { skill_id: "discovered", source_status: "missing" },
          { skill_id: "review", source_status: "present" },
        ] },
      };
      if (args?.skill !== "review") throw new Error("unmanaged source cannot be inspected");
      return { ok: true, data: { provenance: { team: source } } };
    });
    render(app());
    fireEvent.click(screen.getByText("检查本机版本"));
    expect(await screen.findByText("可更新：团队推荐版本已变化。")).toBeInTheDocument();
    expect(client.native).not.toHaveBeenCalledWith("inspect_skill", expect.objectContaining({ skill: "discovered" }));
  });
  it("does not count a same slug from another service, team, or skill as installed", async () => {
    const sources = {
      review: { ...source, service_origin: "https://other.example.test" },
      otherTeam: { ...source, team_id: "team-b" },
      otherSkill: { ...source, skill_id: "different-skill" },
    };
    vi.mocked(client.native).mockImplementation(async (name, args) =>
      name === "local_skills"
        ? {
            ok: true,
            data: {
              skills: Object.keys(sources).map((skill_id) => ({ skill_id })),
            },
          }
        : {
            ok: true,
            data: {
              provenance: {
                team: sources[args?.skill as keyof typeof sources],
              },
            },
          },
    );
    render(app());
    fireEvent.click(screen.getByText("检查本机版本"));
    expect(
      await screen.findByText("同名技能来自其他来源，未算作此团队技能的安装。"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/当前版本 ID/)).not.toBeInTheDocument();
    expect(screen.queryByText(/可更新：/)).not.toBeInTheDocument();
  });
  it.each([
    ["v1", "v1", "固定版本：保留当前选择。"],
    ["recommended", "v1", "可更新：团队推荐版本已变化。"],
    ["recommended", "v2", "已是推荐版本。"],
  ])(
    "classifies %s at %s using exact provenance",
    async (requested_ref, version_id, label) => {
      vi.mocked(client.native).mockImplementation(async (name) =>
        name === "local_skills"
          ? { ok: true, data: { skills: [{ skill_id: "review" }] } }
          : {
              ok: true,
              data: {
                provenance: { team: { ...source, requested_ref, version_id } },
              },
            },
      );
      render(app());
      fireEvent.change(
        screen.getByLabelText("Registry 目录（留空使用 Loom 默认目录）"),
        { target: { value: "/tmp/selected-registry" } },
      );
      fireEvent.click(screen.getByText("检查本机版本"));
      expect(await screen.findByText(label)).toBeInTheDocument();
      expect(client.native).toHaveBeenCalledWith("inspect_skill", {
        root: "/tmp/selected-registry",
        skill: "review",
      });
      expect(screen.getByText(/当前版本 ID/)).toHaveTextContent(version_id);
    },
  );
  it("discards an old team's pending inspection when the scope changes", async () => {
    let resolve: (value: unknown) => void = () => {};
    const pending = new Promise((done) => {
      resolve = done;
    });
    vi.mocked(client.native).mockImplementation(async (name) =>
      name === "local_skills"
        ? { ok: true, data: { skills: [{ skill_id: "review" }] } }
        : pending,
    );
    const view = render(app());
    fireEvent.click(screen.getByText("检查本机版本"));
    await waitFor(() =>
      expect(client.native).toHaveBeenCalledWith("inspect_skill", {
        root: null,
        skill: "review",
      }),
    );
    view.rerender(app("team-b"));
    resolve({ ok: true, data: { provenance: { team: source } } });
    await waitFor(() =>
      expect(screen.getByText("本机状态：尚未检查")).toBeInTheDocument(),
    );
    expect(screen.queryByText(/当前版本 ID/)).not.toBeInTheDocument();
  });
});
