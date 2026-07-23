// Registry schema types are generated from Rust via ts-rs.
// Do not hand-edit under ./generated/ — run `cargo test` to regenerate.
export type { RegistryTarget } from "./generated/RegistryTarget";
export type { RegistryTargetCapabilities } from "./generated/RegistryTargetCapabilities";
export type { RegistryBinding } from "./generated/RegistryBinding";
export type { RegistryWorkspaceMatcher } from "./generated/RegistryWorkspaceMatcher";
export type { RegistryRule } from "./generated/RegistryRule";
export type { RegistryProjection } from "./generated/RegistryProjection";
export type { RegistryCheckpoint } from "./generated/RegistryCheckpoint";

import type { RegistryBinding } from "./generated/RegistryBinding";
import type { RegistryTarget } from "./generated/RegistryTarget";
import type { RegistryRule } from "./generated/RegistryRule";
import type { RegistryProjection } from "./generated/RegistryProjection";
import type { RegistryCheckpoint } from "./generated/RegistryCheckpoint";

export type HealthPayload = {
  ok?: boolean;
  service?: string;
  capabilities?: {
    skill_convergence?: {
      plan?: boolean;
      apply?: boolean;
      requires_plan_digest?: boolean;
      remote_last?: boolean;
    };
  };
};

export type InfoPayload = {
  root?: string;
  state_dir?: string;
  registry_targets_file?: string;
  claude_dir?: string;
  codex_dir?: string;
  agent_dirs?: Array<{
    agent: string;
    env_var?: string;
    path: string;
  }>;
  remote_url?: string;
};

export type RemotePayload = {
  configured?: boolean;
  remote?: string;
  url?: string;
  ahead?: number;
  behind?: number;
  operation_backlog?: number;
  operation_counts: OperationCounts;
  tracking_ref?: boolean;
  sync_state?: string;
};

export type RegistryTransportState =
  | "SYNCED"
  | "PENDING_PUSH"
  | "DIVERGED"
  | "CONFLICTED"
  | "LOCAL_ONLY"
  | "ERROR";

export type ProjectionConvergenceState =
  | "converged"
  | "drifted"
  | "missing"
  | "conflict"
  | "not_applicable"
  | "unknown"
  | "error";

export type VisibilityState =
  | "visible"
  | "not_visible"
  | "restart_required"
  | "unsupported"
  | "unknown"
  | "error";

export type ConvergenceAxisError = { code: string; message: string };

export type ConvergenceStatusPayload = {
  registry_transport: {
    state: RegistryTransportState;
    evidence?: unknown;
    observed_at?: string;
    stale?: boolean;
    errors?: ConvergenceAxisError[];
  };
  projections: {
    state: ProjectionConvergenceState;
    items: unknown[];
    evidence?: unknown;
    observed_at?: string;
    stale?: boolean;
    errors?: ConvergenceAxisError[];
  };
  visibility: {
    state: VisibilityState;
    agent?: string | null;
    evidence?: unknown;
    observed_at?: string;
    stale?: boolean;
    errors?: ConvergenceAxisError[];
  };
  observed_at?: string;
  complete: boolean;
  incomplete_axes: string[];
};

export type OperationCounts = {
  actionable_operations: number;
  local_journal_events: number;
  unpushed_history_events: number;
  local_only_history_events: number;
};

export const ZERO_OPERATION_COUNTS: OperationCounts = {
  actionable_operations: 0,
  local_journal_events: 0,
  unpushed_history_events: 0,
  local_only_history_events: 0,
};

export interface RegistryOperationRecord {
  op_id: string | null;
  audit_id?: string | null;
  source?: string;
  intent: string;
  status: string;
  ack: boolean;
  request_id?: string | null;
  skill?: string | null;
  target?: string | null;
  binding?: string | null;
  method?: string | null;
  payload?: unknown;
  effects?: unknown;
  last_error?: { code: string; message: string };
  created_at: string;
  updated_at: string;
}

export type PendingPayload = {
  count: number;
  ops: RegistryOperationRecord[];
  operation_counts: OperationCounts;
  journal_events?: number;
  history_events?: number;
  warnings?: string[];
};

export type RegistryPayload = {
  ok: boolean;
  data?: {
    counts?: {
      skills?: number;
      targets?: number;
      bindings?: number;
      active_bindings?: number;
      rules?: number;
      projections?: number;
      drifted_projections?: number;
      operations?: number;
    };
    bindings?: RegistryBinding[];
    targets?: RegistryTarget[];
    rules?: RegistryRule[];
    projections?: RegistryProjection[];
    checkpoint?: RegistryCheckpoint;
  };
  error?: {
    code?: string;
    message?: string;
    next_actions?: { cmd: string; reason: string }[];
  };
};
