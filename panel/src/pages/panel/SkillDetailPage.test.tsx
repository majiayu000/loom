import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SkillInspectPayload, SkillInspectRuntimeStatus } from "../../lib/api/client";
import { SkillInspectSections } from "./SkillDetailPage";

vi.mock("../../lib/api/client", () => ({
  api: {
    skillInspect: vi.fn(),
  },
}));

const runtimeBase: SkillInspectRuntimeStatus = {
  installed_in_registry: true,
  active_rule_present: true,
  projected_to_target: true,
  materialized_path_exists: true,
  visible_to_agent: "unknown",
  enabled_by_agent_config: "unknown",
  restart_required: "unknown",
  target_id: "target-1",
  binding_id: "binding-1",
  target_path: "~/.claude/skills",
  materialized_path: "~/.claude/skills/demo",
  health: "healthy",
  truth_level: "registry_projection",
  findings: [],
};

function inspectFixture(): SkillInspectPayload {
  return {
    skill: "demo",
    source: {
      path: "/tmp/registry/skills/demo",
      exists: true,
      entrypoint: "SKILL.md",
      entrypoint_exists: true,
      working_tree_drift: false,
      head_tree_oid: "tree123",
      last_source_commit: "abc123",
      drifted_paths: [],
    },
    spec: {
      portable: "pass",
      codex: "pass",
      claude: "pass",
      findings: [
        {
          id: "portable_note",
          severity: "warning",
          message: "portable metadata is incomplete",
          suggested_action: "loom skill lint demo --portable",
        },
      ],
    },
    provenance: {},
    runtime: {
      claude: {
        ...runtimeBase,
        enabled_by_agent_config: "disabled-by-config",
        findings: [
          {
            id: "agent_disabled",
            severity: "error",
            message: "agent config disables this skill",
            next_action: "loom codex reconcile-plan --agent claude",
          },
        ],
      },
      codex: {
        ...runtimeBase,
        restart_required: "needs-restart",
      },
      gemini: {
        ...runtimeBase,
        active_rule_present: true,
        projected_to_target: false,
        materialized_path_exists: null,
        health: null,
        target_path: "~/.gemini/skills",
      },
    },
    dependencies: null,
    quality: {
      last_eval: null,
      trigger_precision: null,
      trigger_recall: null,
      baseline_delta: null,
    },
    safety: {
      trust: "unknown",
      policy: "unknown",
      scripts_present: null,
      network_requested: null,
      quarantined: false,
      reason: null,
      updated_at: null,
    },
    next_actions: ["loom skill eval demo", "loom skill policy demo"],
  };
}

describe("SkillInspectSections", () => {
  it("renders inspect sections, runtime attention states, and command next actions", () => {
    render(<SkillInspectSections inspect={inspectFixture()} />);

    expect(screen.getByText("Source")).toBeInTheDocument();
    expect(screen.getByText("Spec compatibility")).toBeInTheDocument();
    expect(screen.getByText("Runtime visibility")).toBeInTheDocument();
    expect(screen.getByText("Quality and eval")).toBeInTheDocument();
    expect(screen.getByText("Safety and trust")).toBeInTheDocument();
    expect(screen.getByText("Next actions")).toBeInTheDocument();

    expect(screen.getByText("disabled-by-config")).toBeInTheDocument();
    expect(screen.getByText("needs-restart")).toBeInTheDocument();
    expect(screen.getByText("missing projection")).toBeInTheDocument();
    expect(screen.getByText("No eval evidence recorded.")).toBeInTheDocument();
    expect(screen.getByText("No safety scan evidence recorded.")).toBeInTheDocument();
    expect(screen.getByText("loom skill eval demo")).toBeInTheDocument();
    expect(screen.getByText("loom codex reconcile-plan --agent claude")).toBeInTheDocument();
  });
});
