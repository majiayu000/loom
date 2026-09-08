import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Activation, InstallSkill, LocalSkills } from "./operations";
import { PublishForm } from "./PublishForm";
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
  id: "skill-uuid",
  slug: "review",
  title: "Review",
  description: "Review code",
  example: "Review this",
  maintainer_id: "alice",
  recommended_version_id: "version-uuid",
  archived_at: null,
  revision: 1,
};
const versions = [
  {
    id: "version-uuid",
    version: "1.0.0",
    sha256: "a".repeat(64),
    release_notes: "First",
    created_at: "2026-09-08",
  },
];
const errors: string[] = [];
const run = async (fn: () => Promise<void>) => {
  try {
    await fn();
  } catch (error) {
    errors.push(String(error));
  }
};
describe("native team workflows", () => {
  it("binds apply to a safe preview and activates separately", async () => {
    vi.mocked(client.native).mockImplementation(async (name) =>
      name === "preview_team_install"
        ? {
            ok: true,
            data: {
              plan_id: "p1",
              plan_digest: "d1",
              safe_to_apply: true,
              execution_enabled: true,
              effects: [],
            },
          }
        : { ok: true, data: {} },
    );
    render(
      <InstallSkill
        team="team-uuid"
        skill={skill}
        versions={versions}
        run={run}
      />,
    );
    expect(screen.queryByText("确认导入本机仓库")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Registry 目录（留空使用默认）"), {
      target: { value: "/tmp/test-registry" },
    });
    fireEvent.click(screen.getByText("预览安装"));
    fireEvent.click(await screen.findByText("确认导入本机仓库"));
    await waitFor(() =>
      expect(client.native).toHaveBeenCalledWith("apply_plan", {
        root: "/tmp/test-registry",
        planId: "p1",
        planDigest: "d1",
        idempotencyKey: expect.any(String),
      }),
    );
    expect(client.native).toHaveBeenCalledWith("preview_team_install", {
      team: "team-uuid",
      skill: "skill-uuid",
      version: "version-uuid",
      name: "review",
      root: "/tmp/test-registry",
      requestedRef: "recommended",
    });
    expect(await screen.findByText("激活到项目")).toBeInTheDocument();
    expect(
      vi
        .mocked(client.native)
        .mock.calls.some(([name]) => name === "apply_activate"),
    ).toBe(false);
  });
  it("blocks policy-rejected plans and invalidates preview when version changes", async () => {
    vi.mocked(client.native).mockResolvedValue({
      ok: true,
      data: {
        plan_id: "p1",
        plan_digest: "d1",
        safe_to_apply: false,
        execution_enabled: false,
        required_approvals: ["security-review"],
        conflicts: [{ message: "需要审批" }],
      },
    });
    render(
      <InstallSkill team="team" skill={skill} versions={versions} run={run} />,
    );
    fireEvent.click(screen.getByText("预览安装"));
    expect(await screen.findByText("确认导入本机仓库")).toBeDisabled();
    expect(screen.getByText("需要审批")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("版本"), {
      target: { value: "version-uuid" },
    });
    expect(screen.queryByText("确认导入本机仓库")).not.toBeInTheDocument();
  });
  it("previews folder contents and publishes the exact digest", async () => {
    const done = vi.fn();
    vi.mocked(client.native).mockImplementation(async (name) =>
      name === "choose_directory"
        ? "/tmp/review"
        : name === "preview_publish"
          ? {
              files: [{ path: "SKILL.md", size: 42 }],
              size_bytes: 80,
              sha256: "digest",
            }
          : { data: { skill_id: "skill-uuid" } },
    );
    const view = render(
      <PublishForm
        team="team"
        busy={false}
        run={run}
        done={done}
        close={() => {}}
      />,
    );
    expect(screen.getByText("发布到团队")).toBeDisabled();
    fireEvent.click(screen.getByText("选择目录并预览"));
    expect(await screen.findByText("SKILL.md · 42 字节")).toBeInTheDocument();
    fireEvent.submit(view.container.querySelector("form") as HTMLFormElement);
    await waitFor(() => expect(done).toHaveBeenCalledOnce());
    expect(client.native).toHaveBeenCalledWith(
      "publish_skill",
      expect.objectContaining({
        source: "/tmp/review",
        expectedSha256: "digest",
        idempotencyKey: expect.any(String),
      }),
    );
  });
  it("does not claim activation succeeded after engine failure", async () => {
    vi.mocked(client.native).mockImplementation(async (name) =>
      name === "choose_directory"
        ? "/tmp/project"
        : name === "preview_activate"
          ? {
              ok: true,
              data: { materialized_path: "/tmp/project/.agents/skills/review" },
            }
          : { ok: false, error: { message: "projection conflict" } },
    );
    render(<Activation skill="review" root="/tmp/registry" run={run} />);
    fireEvent.click(screen.getByText("选择项目"));
    await waitFor(() => expect(screen.getByText("预览激活")).toBeEnabled());
    fireEvent.click(screen.getByText("预览激活"));
    fireEvent.click(await screen.findByText("确认激活到项目"));
    await waitFor(() => expect(errors).toContain("Error: projection conflict"));
    expect(screen.queryByText(/文件已激活/)).not.toBeInTheDocument();
  });
  it("renders actual inventory without implicit initialization", async () => {
    vi.mocked(client.native).mockResolvedValue({
      ok: true,
      data: {
        registry_available: true,
        skills: [
          {
            skill_id: "my-skill",
            description: "Useful",
            source_status: "present",
            source_path: "/tmp/registry/skills/my-skill",
          },
        ],
      },
    });
    render(<LocalSkills run={run} />);
    expect(client.native).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("读取本机技能"));
    expect(await screen.findByText("my-skill")).toBeInTheDocument();
    expect(client.native).not.toHaveBeenCalledWith(
      "initialize_registry",
      expect.anything(),
    );
  });
});
