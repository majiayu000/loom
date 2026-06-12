import { afterAll, beforeEach, expect, test, vi } from "vitest";
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
    const navigate = vi.fn();
    const { container } = render(<DoctorPage apiReachable={true} mode="live" refreshKey="tick-1" onNavigate={navigate} />);

    await screen.findByText("Git integrity");
    expect(screen.getByText("Target path")).toBeTruthy();
    expect(screen.getByText("1 issues / 1 checks")).toBeTruthy();
    expect(screen.getByText("target_path_exists:target_claude_claude_project_a · target_claude_claude_project_a")).toBeTruthy();

    const rendered = container.textContent ?? "";
    expect(rendered.indexOf("Target path")).toBeLessThan(rendered.indexOf("target_path_exists:target_claude_claude_project_a"));

    fireEvent.click(screen.getByRole("button", { name: "Open Targets" }));
    expect(navigate).toHaveBeenCalledWith("targets");
  } finally {
    api.workspaceDoctor = originalDoctor;
  }
});

test("DoctorPage routes binding and projection checks before target fallback", async () => {
  const originalDoctor = api.workspaceDoctor;
  const payload: DoctorPayload = {
    healthy: false,
    checks_v1: [
      {
        section: "bindings",
        id: "binding_target_exists:binding_claude_missing",
        ok: false,
        severity: "error",
        message: "binding default target is missing",
        next_action: "remove or update the binding",
        details: {
          binding_id: "binding_claude_missing",
          target_id: "target_claude_missing",
        },
      },
      {
        section: "projections",
        id: "projection_path_exists:projection_1",
        ok: false,
        severity: "warning",
        message: "projection path is missing",
        next_action: "recreate or capture the projection",
        details: {
          instance_id: "projection_1",
          target_id: "target_claude_project_a",
        },
      },
    ],
  };
  api.workspaceDoctor = async () => payload;

  try {
    const navigate = vi.fn();
    render(<DoctorPage apiReachable={true} mode="live" refreshKey="tick-1" onNavigate={navigate} />);

    fireEvent.click(await screen.findByRole("button", { name: "Open Bindings" }));
    fireEvent.click(screen.getByRole("button", { name: "Open Projections" }));

    expect(navigate).toHaveBeenNthCalledWith(1, "bindings");
    expect(navigate).toHaveBeenNthCalledWith(2, "projections");
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
    expect(screen.getByText("Operational metadata")).toBeInTheDocument();
    expect(screen.getByText("Local preferences")).toBeInTheDocument();
    expect(screen.getByText("Reduced motion")).toBeInTheDocument();
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
