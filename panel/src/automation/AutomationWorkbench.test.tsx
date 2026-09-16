import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AutomationWorkbench } from "./AutomationWorkbench";

describe("AutomationWorkbench", () => {
  it("requires a successful preview and new confirmation when parameters change", async () => {
    const execute = vi.fn().mockResolvedValue({ ok: true, data: { provider: "codex-cli", dry_run: true } });
    render(<AutomationWorkbench execute={execute} />);
    fireEvent.change(screen.getByLabelText("技能或集合名称"), { target: { value: "demo" } });
    fireEvent.change(screen.getByLabelText("改写要求"), { target: { value: "Clarify examples" } });
    expect(screen.getByRole("button", { name: "确认执行" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "预览" }));
    await waitFor(() => expect(execute).toHaveBeenCalledWith({ action: "author_rewrite", name: "demo", instruction: "Clarify examples", dry_run: true }, undefined));
    await screen.findByText("已完成");
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "确认执行" }));
    await waitFor(() => expect(execute).toHaveBeenLastCalledWith({ action: "author_rewrite", name: "demo", instruction: "Clarify examples", dry_run: false }, undefined));
    await waitFor(() => expect(screen.getByRole("button", { name: "预览" })).toBeEnabled());
    fireEvent.change(screen.getByLabelText("改写要求"), { target: { value: "Different task" } });
    expect(screen.getByRole("button", { name: "确认执行" })).toBeDisabled();
    expect(screen.getByRole("checkbox")).toBeDisabled();
  });

  it("shows failures and never enables execution after a failed preview", async () => {
    const execute = vi.fn().mockResolvedValue({ ok: false, error: { message: "source has changed" } });
    render(<AutomationWorkbench execute={execute} />);
    fireEvent.change(screen.getByLabelText("技能或集合名称"), { target: { value: "demo" } });
    fireEvent.change(screen.getByLabelText("改写要求"), { target: { value: "Clarify" } });
    fireEvent.click(screen.getByRole("button", { name: "预览" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("source has changed");
    expect(screen.getByRole("button", { name: "确认执行" })).toBeDisabled();
  });

  it("passes approvals as separate values and retains retry identity", async () => {
    const execute = vi.fn().mockResolvedValue({ ok: true, data: { required_approvals: ["approval:workflow-execute"] } });
    render(<AutomationWorkbench execute={execute} desktop />);
    fireEvent.click(screen.getByRole("button", { name: "执行已审阅计划" }));
    fireEvent.change(screen.getByLabelText("Registry 目录"), { target: { value: "/registry" } });
    fireEvent.change(screen.getByLabelText("已审阅计划"), { target: { value: "plan-1" } });
    fireEvent.change(screen.getByLabelText(/计划要求的审批名称/), { target: { value: "approval:workflow-execute\napproval:workspace-write" } });
    const key = (screen.getByLabelText("执行标识") as HTMLInputElement).value;
    fireEvent.click(screen.getByRole("button", { name: "预览" }));
    await screen.findByText("已完成");
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "确认执行" }));
    await waitFor(() => expect(execute).toHaveBeenLastCalledWith(expect.objectContaining({ idempotency_key: key, approvals: ["approval:workflow-execute", "approval:workspace-write"], dry_run: false }), "/registry"));
  });

  it("prevents execution in unavailable and read-only runtimes", () => {
    const execute = vi.fn();
    const { rerender } = render(<AutomationWorkbench execute={execute} unavailable />);
    expect(screen.queryByRole("button", { name: "预览" })).not.toBeInTheDocument();
    rerender(<AutomationWorkbench execute={execute} readOnly />);
    expect(screen.getByRole("button", { name: "预览" })).toBeDisabled();
    expect(execute).not.toHaveBeenCalled();
  });
});
