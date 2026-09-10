import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { LocalDetail } from "./LocalDetail";
import { LocalSkillList } from "./LocalSkillList";
import { LocalActivity } from "./LocalActivity";
import * as client from "./client";

vi.mock("./client", async original => ({ ...(await original<typeof client>()), isDesktop: () => true, native: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

describe("reused local tools", () => {
  it("renders source and installation evidence with the original inspect component", () => {
    render(<LocalDetail root="/tmp/registry" skill="demo" inspection={{
      skill: "demo", source: { path: "/tmp/registry/skills/demo", exists: true, working_tree_drift: true },
      spec: { portable: "pass", codex: "pass", claude: "pass", findings: [] },
      runtime: { codex: { target_path: "/tmp/project/.agents/skills", materialized_path: "/tmp/project/.agents/skills/demo", active_rule_present: true, projected_to_target: false, findings: [] } },
      quality: {}, safety: { trust: "unknown", policy: "review" }, next_actions: [],
    }} />);
    expect(screen.getByText("/tmp/registry/skills/demo")).toBeInTheDocument();
    expect(screen.getByText(/\/tmp\/project\/\.agents\/skills\/demo/)).toBeInTheDocument();
    expect(screen.getByText("missing projection")).toBeInTheDocument();
  });

  it("compares explicitly selected local revisions without invoking cloud or mutations", async () => {
    vi.mocked(client.native).mockImplementation(async name => name === "history_skill"
      ? { ok: true, data: { items: [
        { commit: "bbbb", short_commit: "bbbb", message: "new", committed_at: "today" },
        { commit: "aaaa", short_commit: "aaaa", message: "old", committed_at: "yesterday" },
      ] } }
      : { ok: true, data: { diff: "-old\n+new" } });
    render(<LocalDetail root="/tmp/registry" skill="my-alias" inspection={null} />);
    fireEvent.click(screen.getByText("历史与差异"));
    fireEvent.click(screen.getByText("读取本机历史"));
    await screen.findByLabelText("旧修订");
    fireEvent.click(screen.getByText("查看差异"));
    expect(await screen.findByLabelText("本机版本差异")).toHaveTextContent("+new");
    expect(client.native).toHaveBeenLastCalledWith("diff_skill", { root: "/tmp/registry", skill: "my-alias", from: "aaaa", to: "bbbb" });
  });

  it("does not show an old diagnosis error after changing tabs", async () => {
    let reject: (error: Error) => void = () => {};
    vi.mocked(client.native).mockReturnValue(new Promise((_, no) => { reject = no; }));
    render(<LocalDetail root="/tmp/registry" skill="demo" inspection={null} />);
    fireEvent.click(screen.getByText("诊断"));
    fireEvent.click(screen.getByText("检查本机技能"));
    fireEvent.click(screen.getByText("历史与差异"));
    reject(new Error("old diagnosis failure"));
    await waitFor(() => expect(screen.queryByText("old diagnosis failure")).not.toBeInTheDocument());
  });

  it("reads the chosen registry through native IPC and renders engine failures", async () => {
    vi.mocked(client.native).mockResolvedValue({ ok: false, error: { message: "Registry unavailable" } });
    render(<LocalActivity />);
    expect(client.native).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Registry 目录（留空使用默认）"), { target: { value: "/tmp/chosen" } });
    fireEvent.click(screen.getByText("读取本机记录"));
    await waitFor(() => expect(screen.getAllByText("Registry unavailable").length).toBeGreaterThan(0));
    expect(client.native).toHaveBeenCalledWith("local_operations", { root: "/tmp/chosen", offset: 0 });
    expect(screen.queryByText("No audit history returned by API.")).not.toBeInTheDocument();
  });
});


it("searches the whole registry and resets pagination when changing status", () => {
  const skills = Array.from({ length: 41 }, (_, i) => ({ skill_id: `skill-${i}`, source_status: i === 40 ? "missing" : "present", description: `Description ${i}` }));
  const select = vi.fn();
  render(<LocalSkillList skills={skills} busy={false} onSelect={select} />);
  expect(screen.getByText("skill-0")).toBeInTheDocument();
  expect(screen.queryByText("skill-20")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("下一页"));
  expect(screen.getByText("skill-20")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox", { name: "搜索技能" }), { target: { value: "skill-40" } });
  fireEvent.click(screen.getByText("skill-40"));
  expect(select).toHaveBeenCalledWith(skills[40]);
  expect(screen.getByText("上一页")).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "已导入40" }));
  expect(screen.getByText("没有匹配的技能")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
  expect(screen.getByText("skill-0")).toBeInTheDocument();
});
