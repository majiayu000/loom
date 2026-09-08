import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TeamApp } from "./TeamApp";
import * as cloud from "./client";

vi.mock("./client", async (original) => ({
  ...(await original<typeof cloud>()),
  readConfig: vi.fn().mockResolvedValue({ cloud_api_url: "", auth_url: "", auth_public_key: "" }),
  requestOtp: vi.fn(), verifyOtp: vi.fn().mockResolvedValue({ id: "alice", email: "alice@example.test" }),
  request: vi.fn(), signOut: vi.fn(),
}));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

async function login() {
  fireEvent.change(screen.getByLabelText("工作邮箱"), { target: { value: "alice@example.test" } });
  fireEvent.click(screen.getByText("发送验证码 →"));
  fireEvent.change(await screen.findByLabelText("邮箱验证码"), { target: { value: "123456" } });
  fireEvent.click(screen.getByText("验证并登录 →"));
}

describe("team first-run", () => {
  it("keeps browser local-file capabilities clearly unavailable", async () => {
    render(<TeamApp />);
    fireEvent.click(screen.getByRole("button", { name: /本机技能/ }));
    expect(await screen.findByText("在桌面 App 中管理本机技能")).toBeInTheDocument();
    expect(cloud.request).not.toHaveBeenCalled();
  });
  it("reports a failed catalog request without pretending the team is empty", async () => {
    vi.mocked(cloud.request).mockImplementation(async (path) => {
      if (path === "/v1/me/teams") return { teams: [{ id: "team-a", name: "研发组", owner_user_id: "alice" }] };
      throw new Error("目录服务不可用");
    });
    render(<TeamApp />); await login();
    expect(await screen.findByRole("alert")).toHaveTextContent("目录服务不可用");
    expect(screen.queryByText("第一个好方法，由你分享。")).not.toBeInTheDocument();
  });
  it("does not show an old team's delayed response after switching teams", async () => {
    let resolveOld: (value: unknown) => void = () => {};
    const pending = new Promise((resolve) => { resolveOld = resolve; });
    vi.mocked(cloud.request).mockImplementation(async (path) => {
      if (path === "/v1/me/teams") return { teams: [{ id: "a", name: "A", owner_user_id: "alice" }, { id: "b", name: "B", owner_user_id: "alice" }] };
      if (path.includes("/teams/a/")) return pending;
      return { skills: [{ id: "b-skill", slug: "b-skill", title: "B 的技能", description: "B only" }] };
    });
    render(<TeamApp />); await login();
    await waitFor(() => expect(cloud.request).toHaveBeenCalledWith(expect.stringContaining("/teams/a/skills?")));
    fireEvent.change(screen.getByLabelText("当前工作空间"), { target: { value: "b" } });
    expect(await screen.findByText("B 的技能")).toBeInTheDocument();
    resolveOld({ skills: [{ id: "a-skill", title: "A 的私有技能", slug: "a-skill", description: "A only" }] });
    await waitFor(() => expect(screen.queryByText("A 的私有技能")).not.toBeInTheDocument());
  });
});
