import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AskChat, answerAsk } from "./AskChat";
import type { AskSkillHit } from "./AskChat";

afterEach(() => {
  cleanup();
});

const review: AskSkillHit = {
  id: "s1",
  slug: "review",
  title: "代码审查",
  description: "按团队标准读 diff",
};

describe("answerAsk", () => {
  it("refuses an empty question", () => {
    expect(() => answerAsk("  ", [], true)).toThrow("问题不能为空。");
  });

  it("explains login without pretending the catalog is available", () => {
    expect(answerAsk("怎么登录", [], false).text).toMatch(/验证码/);
  });

  it("does not search the catalog before sign-in", () => {
    const answer = answerAsk("代码审查", [review], false);
    expect(answer.skills).toBeUndefined();
    expect(answer.text).toMatch(/登录后/);
  });

  it("returns matching skills from the current catalog", () => {
    const answer = answerAsk("审查", [review], true);
    expect(answer.skills).toEqual([review]);
  });
});

describe("AskChat", () => {
  it("opens a conversation and opens a catalog hit", () => {
    const onOpenSkill = vi.fn();
    render(<AskChat skills={[review]} signedIn onOpenSkill={onOpenSkill} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    expect(screen.getByRole("dialog", { name: "Ask" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("问 Ask"), {
      target: { value: "审查" },
    });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    fireEvent.click(screen.getByRole("button", { name: /代码审查/ }));
    expect(onOpenSkill).toHaveBeenCalledWith(review);
  });

  it("ignores an empty send and closes on Escape", () => {
    render(<AskChat skills={[]} signedIn={false} onOpenSkill={vi.fn()} />);
    const pill = screen.getByRole("button", { name: "Ask" });
    fireEvent.click(pill);
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getAllByText(/想找哪个技能/).length).toBe(1);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Ask" })).not.toBeInTheDocument();
  });
});
