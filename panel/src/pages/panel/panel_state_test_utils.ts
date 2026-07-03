import { afterAll } from "vitest";
import { act, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { api, type BindingShowPayload, type CommandEnvelope, type DoctorPayload, type OpsPayload, type TargetShowPayload, type RegistryOperationRecord } from "../../lib/api/client";
import type { Binding, Skill, Target } from "../../lib/types";
import type { RegistryProjection } from "../../generated/RegistryProjection";

export const originalWindow = (globalThis as { window?: unknown }).window;
export const originalLocalStorage = (globalThis as { localStorage?: unknown }).localStorage;
export const localStorageStub = {
  getItem: (_key: string) => null,
  setItem: (_key: string, _value: string) => {},
  removeItem: (_key: string) => {},
  clear: () => {},
};
(globalThis as { window?: unknown }).window = {
  setTimeout,
  clearTimeout,
  confirm: () => true,
  location: { reload: () => {} },
  localStorage: localStorageStub,
} as unknown;
(globalThis as { localStorage?: unknown }).localStorage = localStorageStub;

afterAll(() => {
  (globalThis as { window?: unknown }).window = originalWindow;
  (globalThis as { localStorage?: unknown }).localStorage = originalLocalStorage;
});

export function textOf(value: unknown): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map((item) => textOf(item)).join("");
  if (value && typeof value === "object" && "props" in value) {
    return textOf((value as { props?: { children?: unknown } }).props?.children);
  }
  return "";
}

export function markup(renderer: ReactTestRenderer): string {
  return JSON.stringify(renderer.toJSON());
}

export function clickableRows(renderer: ReactTestRenderer): ReactTestInstance[] {
  return renderer.root.findAll((node: ReactTestInstance) => node.type === "tr" && typeof node.props.onClick === "function");
}

export function buttonByLabel(renderer: ReactTestRenderer, label: string): ReactTestInstance {
  const button = renderer.root.findAll(
    (node: ReactTestInstance) => node.type === "button" && textOf(node.props.children).includes(label),
  )[0];
  if (!button) throw new Error(`button ${label} not found`);
  return button;
}

export async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

export function makeOperation(status: string, ack = false, opId = "op_123", intent = "skill.project"): RegistryOperationRecord {
  return {
    op_id: opId,
    intent,
    status,
    ack,
    payload: {},
    effects: {},
    created_at: "2026-04-09T10:05:00Z",
    updated_at: "2026-04-09T10:05:00Z",
  };
}

export function makeTarget(overrides: Partial<Target> = {}): Target {
  return {
    id: "target-1",
    agent: "claude",
    profile: "home",
    path: "~/.claude/skills",
    ownership: "managed",
    skills: 0,
    lastSync: "now",
    ...overrides,
  };
}

export function makeBinding(): Binding {
  return {
    id: "binding-1",
    skill: "skill.writer",
    target: "target-1",
    matcher: "path_prefix:/repo",
    method: "symlink",
    policy: "auto",
  };
}

export function makeSkill(): Skill {
  return {
    id: "s-skill.writer",
    name: "skill.writer",
    tag: "v1",
    sourceStatus: "present",
    releaseTags: ["v1"],
    snapshotTags: [],
    latestRev: "abc12345",
    ruleCount: 1,
    bindingCount: 1,
    projectionCount: 1,
    changed: "now",
    targets: ["target-1"],
  };
}

export function makeOrphanProjection(): RegistryProjection {
  return {
    instance_id: "inst-orphan",
    skill_id: "skill.writer",
    target_id: "target-1",
    materialized_path: "/tmp/inst-orphan",
    method: "copy",
    last_applied_rev: "deadbeefcafebabe",
    health: "orphaned",
  };
}

export function bindingPayload(projectionCount: number): BindingShowPayload {
  return {
    ok: true,
    data: {
      state_model: "registry",
      binding: {
        binding_id: "binding-1",
        agent: "claude",
        profile_id: "home",
        workspace_matcher: { kind: "path_prefix", value: "/repo" },
        default_target_id: "target-1",
        policy_profile: "auto",
        active: true,
        created_at: "2026-04-09T10:05:00Z",
      },
      default_target: {
        target_id: "target-1",
        agent: "claude",
        path: "~/.claude/skills",
        ownership: "managed",
        capabilities: { symlink: true, copy: true, watch: true },
        created_at: "2026-04-09T10:05:00Z",
      },
      rules: [
        {
          binding_id: "binding-1",
          skill_id: "skill.writer",
          target_id: "target-1",
          method: "symlink",
          watch_policy: "auto",
          created_at: "2026-04-09T10:05:00Z",
        },
      ],
      projections:
        projectionCount === 0
          ? []
          : [
              {
                instance_id: "proj-1",
                skill_id: "skill.writer",
                binding_id: "binding-1",
                target_id: "target-1",
                materialized_path: "/tmp/proj-1",
                method: "symlink",
                last_applied_rev: "deadbeefcafebabe",
                health: "healthy",
                updated_at: "2026-04-09T10:05:00Z",
              },
            ],
    },
  };
}

export function targetPayload(projectionCount = 0): TargetShowPayload {
  return {
    ok: true,
    data: {
      state_model: "registry",
      target: {
        target_id: "target-1",
        agent: "claude",
        path: "~/.claude/skills",
        ownership: "managed",
        capabilities: { symlink: true, copy: true, watch: true },
        created_at: "2026-04-09T10:05:00Z",
      },
      bindings: [],
      projections:
        projectionCount === 0
          ? []
          : [
              {
                instance_id: "target-proj-1",
                skill_id: "skill.writer",
                binding_id: "binding-1",
                target_id: "target-1",
                materialized_path: "/tmp/target-proj-1",
                method: "symlink",
                last_applied_rev: "deadbeefcafebabe",
                health: "healthy",
                updated_at: "2026-04-09T10:05:00Z",
              },
            ],
      rules: [],
    },
  };
}

export function opsPayload(operation: RegistryOperationRecord): OpsPayload {
  return {
    ok: true,
    data: {
      count: 1,
      loaded_count: 1,
      offset: 0,
      limit: 100,
      has_more: false,
      operations: [operation],
      checkpoint: { last_scanned_op_id: operation.op_id ?? undefined },
    },
  };
}

export function doctorPayload(): DoctorPayload {
  return {
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
        section: "operations",
        id: "operation_journal_warnings",
        ok: false,
        severity: "warning",
        message: "operation journal has malformed or ignored entries",
        next_action: "inspect state/registry/ops/operations.jsonl and repair or purge malformed operation entries",
        details: { warning_count: 1 },
      },
      {
        section: "agents",
        id: "agent_skill_inventory",
        ok: true,
        severity: "info",
        message: "detected 1 of 2 known agent skill directories",
        next_action: null,
        details: {
          agents: [
            {
              agent: "claude",
              default_path: "/tmp/home/.claude/skills",
              present: true,
              registered_target_count: 1,
              registered_targets: [
                {
                  target_id: "target_claude_claude_skills",
                  ownership: "observed",
                },
              ],
            },
            {
              agent: "codex",
              default_path: "/tmp/home/.codex/skills",
              present: false,
              registered_target_count: 0,
              registered_targets: [],
            },
          ],
        },
      },
    ],
  };
}
