import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { TeamApp } from "./TeamApp";
import * as cloud from "./client";

vi.mock("./client", async (original) => ({
  ...(await original<typeof cloud>()),
  readConfig: vi.fn().mockResolvedValue({
    cloud_api_url: "",
    auth_url: "",
    auth_public_key: "",
  }),
  requestOtp: vi.fn(),
  verifyOtp: vi
    .fn()
    .mockResolvedValue({ id: "alice", email: "alice@example.test" }),
  isDesktop: vi.fn().mockReturnValue(false),
  native: vi.fn(),
  request: vi.fn(),
  signOut: vi.fn(),
}));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.mocked(cloud.isDesktop).mockReturnValue(false);
});

async function login() {
  fireEvent.change(screen.getByLabelText("工作邮箱"), {
    target: { value: "alice@example.test" },
  });
  fireEvent.click(screen.getByText("发送验证码 →"));
  fireEvent.change(await screen.findByLabelText("邮箱验证码"), {
    target: { value: "123456" },
  });
  fireEvent.click(screen.getByText("验证并登录 →"));
}

describe("team first-run", () => {
  it("keeps the Ask launcher on the login surface", async () => {
    render(<TeamApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Ask" }));
    expect(screen.getByRole("dialog", { name: "Ask" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "怎么登录" }));
    expect(screen.getByRole("dialog", { name: "Ask" })).toHaveTextContent(
      /验证码即可登录/,
    );
  });

  it("carries a selected installed alias through update preview and project activation", async () => {
    const skill = {
      id: "s1",
      slug: "review",
      title: "Review",
      description: "Review code",
      example: "Try",
      maintainer_id: "alice",
      recommended_version_id: "v2",
      archived_at: null,
      revision: 1,
    };
    vi.mocked(cloud.isDesktop).mockReturnValue(true);
    vi.mocked(cloud.readConfig).mockResolvedValueOnce({
      cloud_api_url: "https://cloud.example.test",
      auth_url: "",
      auth_public_key: "",
    });
    vi.mocked(cloud.request).mockImplementation(async (path) => {
      if (path === "/v1/me/teams")
        return { teams: [{ id: "t1", name: "Team", owner_user_id: "alice" }] };
      if (path.includes("/skills?")) return { skills: [skill] };
      if (path.includes("/versions?"))
        return {
          versions: [
            {
              id: "v2",
              version: "2.0.0",
              sha256: "a".repeat(64),
              created_at: "2026-09-08",
              release_notes: "Update",
            },
          ],
        };
      return { skill };
    });
    vi.mocked(cloud.native).mockImplementation(async (name) => {
      if (name === "current_user")
        return { id: "alice", email: "alice@example.test" };
      if (name === "local_skills")
        return {
          ok: true,
          data: {
            skills: [{ skill_id: "review-one" }, { skill_id: "review-two" }],
          },
        };
      if (name === "inspect_skill")
        return {
          ok: true,
          data: {
            provenance: {
              team: {
                service_origin: "https://cloud.example.test",
                team_id: "t1",
                skill_id: "s1",
                version_id: "v1",
                requested_ref: "recommended",
              },
            },
          },
        };
      if (name === "preview_team_install")
        return {
          ok: true,
          data: {
            plan_id: "p1",
            plan_digest: "digest",
            safe_to_apply: true,
            execution_enabled: true,
          },
        };
      if (name === "choose_directory") return "/tmp/project";
      if (name === "preview_activate")
        return {
          ok: true,
          data: { materialized_path: "/tmp/project/.agents/skills/review-two" },
        };
      return { ok: true, data: {} };
    });
    render(<TeamApp />);
    await screen.findByText("Review");
    fireEvent.click(screen.getByRole("button", { name: /版本更新/ }));
    fireEvent.change(
      await screen.findByLabelText("Registry 目录（留空使用 Loom 默认目录）"),
      { target: { value: "/tmp/registry" } },
    );
    fireEvent.click(screen.getByText("检查本机版本"));
    expect(await screen.findByText("更新本机 review-one")).toBeInTheDocument();
    fireEvent.click(screen.getByText("更新本机 review-two"));
    fireEvent.click(await screen.findByText("预览安装"));
    await waitFor(() =>
      expect(cloud.native).toHaveBeenCalledWith(
        "preview_team_install",
        expect.objectContaining({
          root: "/tmp/registry",
          name: "review-two",
          skill: "s1",
        }),
      ),
    );
    fireEvent.click(await screen.findByText("确认导入本机仓库"));
    fireEvent.click(await screen.findByText("选择项目"));
    await waitFor(() => expect(screen.getByText("预览激活")).toBeEnabled());
    fireEvent.click(screen.getByText("预览激活"));
    await waitFor(() =>
      expect(cloud.native).toHaveBeenCalledWith("preview_activate", {
        root: "/tmp/registry",
        skill: "review-two",
        agent: "codex",
        workspace: "/tmp/project",
      }),
    );
    fireEvent.click(await screen.findByText("确认激活到项目"));
    await waitFor(() =>
      expect(cloud.native).toHaveBeenCalledWith("apply_activate", {
        root: "/tmp/registry",
        skill: "review-two",
        agent: "codex",
        workspace: "/tmp/project",
      }),
    );
  });

  it("restores the native identity before loading private teams", async () => {
    vi.mocked(cloud.isDesktop).mockReturnValue(true);
    vi.mocked(cloud.native).mockResolvedValue({
      id: "alice",
      email: "alice@example.test",
    });
    vi.mocked(cloud.request).mockResolvedValue({ teams: [] });
    render(<TeamApp />);
    expect(await screen.findByText("从一个团队开始。")).toBeInTheDocument();
    expect(cloud.native).toHaveBeenCalledWith("current_user");
    expect(cloud.request).toHaveBeenCalledWith("/v1/me/teams");
    expect(screen.queryByText("发送验证码 →")).not.toBeInTheDocument();
  });
  it("keeps browser local-file capabilities clearly unavailable", async () => {
    render(<TeamApp />);
    fireEvent.click(screen.getByRole("button", { name: /本机技能/ }));
    expect(
      await screen.findByText("在桌面 App 中管理本机技能"),
    ).toBeInTheDocument();
    expect(cloud.request).not.toHaveBeenCalled();
  });
  it("reports a failed catalog request without pretending the team is empty", async () => {
    vi.mocked(cloud.request).mockImplementation(async (path) => {
      if (path === "/v1/me/teams")
        return {
          teams: [{ id: "team-a", name: "研发组", owner_user_id: "alice" }],
        };
      throw new Error("目录服务不可用");
    });
    render(<TeamApp />);
    await login();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "目录服务不可用",
    );
    expect(
      screen.queryByText("第一个好方法，由你分享。"),
    ).not.toBeInTheDocument();
  });
  it("does not show an old team's delayed response after switching teams", async () => {
    let resolveOld: (value: unknown) => void = () => {};
    const pending = new Promise((resolve) => {
      resolveOld = resolve;
    });
    vi.mocked(cloud.request).mockImplementation(async (path) => {
      if (path === "/v1/me/teams")
        return {
          teams: [
            { id: "a", name: "A", owner_user_id: "alice" },
            { id: "b", name: "B", owner_user_id: "alice" },
          ],
        };
      if (path.includes("/teams/a/")) return pending;
      return {
        skills: [
          {
            id: "b-skill",
            slug: "b-skill",
            title: "B 的技能",
            description: "B only",
          },
        ],
      };
    });
    render(<TeamApp />);
    await login();
    await waitFor(() =>
      expect(cloud.request).toHaveBeenCalledWith(
        expect.stringContaining("/teams/a/skills?"),
      ),
    );
    fireEvent.change(screen.getByLabelText("当前工作空间"), {
      target: { value: "b" },
    });
    expect(await screen.findByText("B 的技能")).toBeInTheDocument();
    resolveOld({
      skills: [
        {
          id: "a-skill",
          title: "A 的私有技能",
          slug: "a-skill",
          description: "A only",
        },
      ],
    });
    await waitFor(() =>
      expect(screen.queryByText("A 的私有技能")).not.toBeInTheDocument(),
    );
  });
});
