import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { SkillDetail } from "./SkillDetail";
import { TeamForms, invitationToken } from "./TeamSettings";
import * as client from "./client";
vi.mock("./client", async (original) => ({
  ...(await original<typeof client>()),
  isDesktop: () => false,
  request: vi.fn(),
}));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  window.history.replaceState(null, "", "/");
});
const skill: client.Skill = {
  id: "s1",
  slug: "review",
  title: "Review",
  description: "Review code",
  example: "Try me",
  maintainer_id: "alice",
  recommended_version_id: "v1",
  archived_at: null,
  revision: 7,
};
const run = async (fn: () => Promise<void>) => fn();
const version = {
  id: "v1",
  version: "1.0.0",
  sha256: "a".repeat(64),
  release_notes: "First",
  created_at: "2026-09-08",
};
describe("cloud detail workflows", () => {
  it("loads more versions, previews files as text and edits with current revision", async () => {
    vi.mocked(client.request).mockImplementation(async (path, options) => {
      if (options?.method === "PATCH")
        return { skill: { ...skill, title: "Updated", revision: 8 } };
      if (path.includes("/file?"))
        return { text: "<script>alert('unsafe')</script>" };
      if (path.endsWith("/files"))
        return { files: [{ path: "SKILL.md", size: 40 }] };
      if (path.includes("cursor="))
        return {
          versions: [{ ...version, id: "v2", version: "0.9.0" }],
          next_cursor: null,
        };
      if (path.includes("/versions?"))
        return { versions: [version], next_cursor: "cursor1" };
      return { skill };
    });
    render(
      <SkillDetail
        team="t1"
        initial={skill}
        userId="alice"
        owner={true}
        run={run}
        back={() => {}}
        notice={() => {}}
      />,
    );
    fireEvent.click(await screen.findByText("查看版本文件"));
    fireEvent.click(await screen.findByText("SKILL.md · 40 字节"));
    expect(
      await screen.findByText("<script>alert('unsafe')</script>"),
    ).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
    fireEvent.click(screen.getByText("加载更多版本"));
    expect(await screen.findByText("v0.9.0")).toBeInTheDocument();
    fireEvent.click(screen.getByText("编辑介绍"));
    fireEvent.change(screen.getByLabelText("显示名称"), {
      target: { value: "Updated" },
    });
    fireEvent.click(screen.getByText("保存介绍"));
    await waitFor(() =>
      expect(client.request).toHaveBeenCalledWith(
        "/v1/teams/t1/skills/s1",
        expect.objectContaining({
          method: "PATCH",
          ifMatch: 7,
          body: expect.objectContaining({ title: "Updated" }),
        }),
      ),
    );
    expect(
      await screen.findByRole("heading", { name: "Updated" }),
    ).toBeInTheDocument();
  });
  it("accepts link tokens and selects the accepted team", async () => {
    window.history.replaceState(null, "", "/?invite=secret-token");
    vi.mocked(client.request).mockResolvedValue({ team_id: "new-team" });
    const done = vi.fn().mockResolvedValue(undefined);
    render(<TeamForms run={run} done={done} />);
    expect(screen.getByLabelText("邀请链接或令牌")).toHaveValue("secret-token");
    fireEvent.click(screen.getByText("加入团队"));
    await waitFor(() => expect(done).toHaveBeenCalledWith("new-team"));
    expect(window.location.search).toBe("");
    expect(
      invitationToken("https://example.test/team.html?invite=another-token"),
    ).toBe("another-token");
  });
});
