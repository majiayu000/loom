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
  targets_file?: string;
  claude_dir?: string;
  codex_dir?: string;
  remote_url?: string;
};

export type SkillTargetConfig = {
  method?: string;
  claude_path?: string;
  codex_path?: string;
};

export type LegacyTargetsPayload = {
  skills: Record<string, SkillTargetConfig>;
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

export type V3Binding = {
  binding_id: string;
  agent: string;
  profile_id: string;
  workspace_matcher: {
    kind: string;
    value: string;
  };
  default_target_id: string;
  policy_profile: string;
  active: boolean;
  created_at?: string;
};

export type V3Target = {
  target_id: string;
  agent: string;
  path: string;
  ownership: string;
  capabilities: {
    symlink: boolean;
    copy: boolean;
    watch: boolean;
  };
  created_at?: string;
};

export type V3Rule = {
  binding_id: string;
  skill_id: string;
  target_id: string;
  method: string;
  watch_policy: string;
  created_at?: string;
};

export type V3Projection = {
  instance_id: string;
  skill_id: string;
  binding_id: string;
  target_id: string;
  materialized_path: string;
  method: string;
  last_applied_rev: string;
  health: string;
  observed_drift?: boolean;
  updated_at?: string;
};

export type V3Checkpoint = {
  last_scanned_op_id?: string | null;
  last_acked_op_id?: string | null;
  updated_at?: string;
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
  legacyTargets: LegacyTargetsPayload;
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
  legacyTarget?: SkillTargetConfig;
};
