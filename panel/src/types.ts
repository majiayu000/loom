// V3 schema types are generated from Rust via ts-rs.
// Do not hand-edit under ./generated/ — run `cargo test` to regenerate.
export type { V3Target } from "./generated/V3Target";
export type { V3TargetCapabilities } from "./generated/V3TargetCapabilities";
export type { V3Binding } from "./generated/V3Binding";
export type { V3WorkspaceMatcher } from "./generated/V3WorkspaceMatcher";
export type { V3Rule } from "./generated/V3Rule";
export type { V3Projection } from "./generated/V3Projection";
export type { V3Checkpoint } from "./generated/V3Checkpoint";

import type { V3Binding } from "./generated/V3Binding";
import type { V3Target } from "./generated/V3Target";
import type { V3Rule } from "./generated/V3Rule";
import type { V3Projection } from "./generated/V3Projection";
import type { V3Checkpoint } from "./generated/V3Checkpoint";

export type PageId =
  | "overview"
  | "skills"
  | "bindings"
  | "targets"
  | "projections"
  | "ops"
  | "settings";

export type HealthPayload = {
  ok?: boolean;
  service?: string;
};

export type InfoPayload = {
  root?: string;
  state_dir?: string;
  v3_targets_file?: string;
  claude_dir?: string;
  codex_dir?: string;
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

export type RemoteStatusError = {
  code?: string;
  message?: string;
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

export type V3Payload = {
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
    bindings?: V3Binding[];
    targets?: V3Target[];
    rules?: V3Rule[];
    projections?: V3Projection[];
    checkpoint?: V3Checkpoint;
  };
  error?: {
    code?: string;
    message?: string;
  };
};

export type V3Model = {
  available: boolean;
  counts: NonNullable<NonNullable<V3Payload["data"]>["counts"]>;
  bindings: V3Binding[];
  targets: V3Target[];
  rules: V3Rule[];
  projections: V3Projection[];
  checkpoint?: V3Checkpoint;
  error?: string;
};

export type PanelData = {
  health: HealthPayload;
  info: InfoPayload;
  skills: string[];
  remote: RemotePayload;
  pending: PendingPayload;
  v3: V3Model;
  remoteWarnings: string[];
  live: boolean;
  lastUpdated: string;
};

export type SkillView = {
  name: string;
  projections: V3Projection[];
  rules: V3Rule[];
  bindings: V3Binding[];
  targets: V3Target[];
  methods: string[];
  driftedCount: number;
};
