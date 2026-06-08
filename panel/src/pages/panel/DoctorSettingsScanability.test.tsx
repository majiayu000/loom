import { afterAll, beforeEach, expect, test } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api, type DoctorPayload } from "../../lib/api/client";
import { DoctorPage } from "./DoctorPage";
import { SettingsPage } from "./SettingsPage";

const originalNavigator = (globalThis as { navigator?: unknown }).navigator;
const clipboardWrites: string[] = [];

Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    clipboard: {
      writeText: async (value: string) => {
        clipboardWrites.push(value);
      },
    },
  },
});

afterAll(() => {
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: originalNavigator,
  });
});

beforeEach(() => {
  clipboardWrites.length = 0;
});

test("DoctorPage renders human labels before internal check IDs", async () => {
  const originalDoctor = api.workspaceDoctor;
  const payload: DoctorPayload = {
    healthy: false,
    checks_v1: [
      {
        section: "git",
        id: "git_fsck",
        ok: true,
        severity: "ok",
        message: "git object database is healthy",
        next_action: null,
        details: {},
      },
      {
        section: "targets",
        id: "target_path_exists:target_claude_claude_project_a",
        ok: false,
        severity: "error",
        message: "target path is missing",
        next_action: "recreate the target path or remove/update the target",
        details: {
          target_id: "target_claude_claude_project_a",
          agent: "claude",
          path: "/tmp/home/.claude/projects/project-a/skills",
          ownership: "observed",
        },
      },
    ],
  };
  api.workspaceDoctor = async () => payload;

  try {
    const { container } = render(<DoctorPage apiReachable={true} mode="live" refreshKey="tick-1" />);

    await screen.findByText("Git integrity");
    expect(screen.getByText("Target path")).toBeTruthy();
    expect(screen.getByText("1 issues / 1 checks")).toBeTruthy();
    expect(screen.getByText("target_path_exists:target_claude_claude_project_a · target_claude_claude_project_a")).toBeTruthy();

    const rendered = container.textContent ?? "";
    expect(rendered.indexOf("Target path")).toBeLessThan(rendered.indexOf("target_path_exists:target_claude_claude_project_a"));
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("SettingsPage wraps long paths and exposes copy buttons", async () => {
  const originalInfo = api.info;
  api.info = async () => ({
    root: "/tmp/loom",
    state_dir: "/tmp/loom/.loom/state/with/a/long/path/that/needs/wrapping",
    registry_targets_file: "/tmp/loom/.loom/registry/targets/with/a/long/file/name/targets.json",
    agent_dirs: [
      {
        agent: "claude",
        env_var: "CLAUDE_SKILLS_DIR",
        path: "/tmp/home/.claude/projects/example-with-a-long-name/skills",
      },
    ],
    remote_url: "git@example.com:loom.git",
  });

  try {
    const { container } = render(<SettingsPage live={true} mode="live" registryRoot="/tmp/loom-registry-with-a-long-path" />);

    await screen.findByText("/tmp/home/.claude/projects/example-with-a-long-name/skills");
    expect(container.querySelector(".setting-path-text")).toBeTruthy();
    expect(container.querySelector(".setting-copy-btn")).toBeTruthy();

    fireEvent.click(screen.getAllByText("Copy")[0]);

    await waitFor(() => {
      expect(clipboardWrites).toEqual(["/tmp/loom-registry-with-a-long-path"]);
      expect(screen.getByText("Copied")).toBeTruthy();
    });
  } finally {
    api.info = originalInfo;
  }
});
