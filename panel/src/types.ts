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
  pending_ops?: number;
  tracking_ref?: boolean;
  sync_state?: string;
};

export type PendingOp = {
  op_id?: string;
  request_id: string;
  command: string;
  created_at: string;
  details: Record<string, unknown>;
};

export type PendingPayload = {
  count: number;
  ops: PendingOp[];
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
