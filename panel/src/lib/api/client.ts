import type {
  HealthPayload,
  InfoPayload,
  PendingPayload,
  RemotePayload,
  RegistryPayload,
} from "../../types";
import type { RegistryBinding } from "../../generated/RegistryBinding";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { RegistryRule } from "../../generated/RegistryRule";
import type { RegistryTarget } from "../../generated/RegistryTarget";

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

export interface OpsPayload {
  ok: boolean;
  data?: {
    state_model?: string;
    count: number;
    loaded_count: number;
    offset: number;
    limit: number;
    has_more: boolean;
    operations: RegistryOperationRecord[];
    checkpoint?: { last_scanned_op_id?: string; last_acked_op_id?: string; updated_at?: string };
  };
  error?: { code?: string; message?: string };
}

export interface OpsHistoryDiagnosePayload {
  ok: boolean;
  data?: {
    local_branch: boolean;
    remote_tracking: boolean;
    ahead: number;
    behind: number;
    local_segments: number;
    local_archives: number;
    remote_segments: number;
    remote_archives: number;
    local_snapshot: boolean;
    remote_snapshot: boolean;
    compact_after_segments: number;
    retain_recent_segments: number;
    retain_archives: number;
    conflicts: Array<{
      scope: string;
      path: string;
      local_blob: string;
      remote_blob: string;
      local_rename_path: string;
      remote_rename_path: string;
    }>;
  };
  error?: { code?: string; message?: string };
}

export interface BindingShowPayload {
  ok: boolean;
  data?: {
    state_model?: string;
    binding: RegistryBinding;
    default_target?: RegistryTarget | null;
    rules?: RegistryRule[];
    projections?: RegistryProjection[];
  };
  error?: { code?: string; message?: string };
}

export interface TargetShowPayload {
  ok: boolean;
  data?: {
    state_model?: string;
    target: RegistryTarget;
    bindings?: RegistryBinding[];
    rules?: RegistryRule[];
    projections?: RegistryProjection[];
  };
  error?: { code?: string; message?: string };
}

export interface SkillSummaryPayload {
  skill_id: string;
  description?: string | null;
  source_status?: "present" | "missing" | "non-compliant";
  source_path?: string | null;
  latest_rev?: string | null;
  latest_updated_at?: string | null;
  bindings_count?: number;
  projections_count?: number;
  target_ids?: string[];
  observed_target_ids?: string[];
  release_tags?: string[];
  snapshot_tags?: string[];
  observed_imported?: boolean;
  sources?: string[];
}

export interface SkillsPayload {
  skills?: Array<string | SkillSummaryPayload>;
  skill_names?: string[];
}

export interface RemoteStatusResponse {
  remote?: RemotePayload;
  warnings?: string[];
}

export interface WorkspaceStatusPayload {
  state_model?: string;
  registry?: {
    available?: boolean;
    error?: { code?: string; message?: string };
    counts?: Record<string, number>;
  };
}

export interface CommandEnvelope {
  ok: boolean;
  cmd: string;
  request_id: string;
  data?: Record<string, unknown>;
  error?: { code?: string; message?: string; details?: Record<string, unknown> };
  meta?: { warnings?: string[] };
}

export interface ReadResult<T> {
  data: T;
  warnings: string[];
}

export class ApiError extends Error {
  constructor(public readonly path: string, public readonly status: number, message: string) {
    super(message);
    this.name = "ApiError";
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function warningStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((warning): warning is string => typeof warning === "string" && warning.length > 0);
}

function envelopeWarnings(body: unknown): string[] {
  if (!isRecord(body) || !isRecord(body.meta)) return [];
  return warningStrings(body.meta.warnings);
}

function payloadWarnings(body: unknown): string[] {
  if (!isRecord(body)) return [];
  return warningStrings(body.warnings);
}

function uniqueWarnings(warnings: string[]): string[] {
  return Array.from(new Set(warnings));
}

function unwrapReadResult<T>(path: string, body: unknown): ReadResult<T> {
  if (
    !isRecord(body) ||
    body.ok !== true ||
    typeof body.cmd !== "string" ||
    typeof body.request_id !== "string"
  ) {
    throw new ApiError(path, 200, `GET ${path} returned non-envelope payload`);
  }
  if (!("data" in body)) {
    throw new ApiError(path, 200, `GET ${path} envelope is missing data`);
  }
  return {
    data: body.data as T,
    warnings: uniqueWarnings([...envelopeWarnings(body), ...payloadWarnings(body.data)]),
  };
}

function unwrapReadData<T>(path: string, body: unknown): T {
  return unwrapReadResult<T>(path, body).data;
}

function parseRemoteStatusResponse(path: string, body: unknown): RemoteStatusResponse {
  if (!isRecord(body)) {
    throw new ApiError(path, 200, `GET ${path} returned malformed remote status payload`);
  }
  if (isRecord(body.error)) {
    const message =
      typeof body.error.message === "string"
        ? body.error.message
        : `GET ${path} returned error-shaped payload`;
    throw new ApiError(path, 200, message);
  }
  if (!isRecord(body.remote)) {
    throw new ApiError(path, 200, `GET ${path} returned malformed remote status payload`);
  }
  return body as RemoteStatusResponse;
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, { signal });
  let body: unknown;
  let parseError: string | null = null;
  try {
    body = await res.json();
  } catch (err) {
    if (err instanceof DOMException && err.name === "AbortError") {
      throw err;
    }
    parseError = err instanceof Error ? err.message : String(err);
  }

  const messageFromBody =
    typeof body === "object" && body !== null && "error" in body
      ? ((body as { error?: { message?: string } }).error?.message ?? null)
      : null;

  if (!res.ok) {
    const msg = messageFromBody ?? `GET ${path} returned ${res.status}`;
    throw new ApiError(path, res.status, msg);
  }

  if (parseError !== null) {
    throw new ApiError(path, res.status, `GET ${path} returned non-JSON body: ${parseError}`);
  }

  if (
    typeof body === "object" &&
    body !== null &&
    "ok" in body &&
    (body as { ok?: boolean }).ok === false
  ) {
    throw new ApiError(
      path,
      res.status,
      messageFromBody ?? `GET ${path} envelope ok=false with no message`,
    );
  }

  return body as T;
}

async function getJsonData<T>(path: string, signal?: AbortSignal): Promise<T> {
  return unwrapReadData<T>(path, await getJson<unknown>(path, signal));
}

async function getJsonWithWarnings<T>(path: string, signal?: AbortSignal): Promise<ReadResult<T>> {
  const body = await getJson<unknown>(path, signal);
  return { data: body as T, warnings: uniqueWarnings([...envelopeWarnings(body), ...payloadWarnings(body)]) };
}

async function getJsonDataWithWarnings<T>(path: string, signal?: AbortSignal): Promise<ReadResult<T>> {
  return unwrapReadResult<T>(path, await getJson<unknown>(path, signal));
}

async function postJson(path: string, body: unknown): Promise<CommandEnvelope> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  // Parse the body, but don't conflate "server returned non-JSON (e.g.
  // upstream proxy error page)" with "envelope says ok=false" (cf. PR
  // #7 review H2). Keep the HTTP statusText so ApiError surfaces the
  // real cause instead of silently masking it.
  let envelope: CommandEnvelope | null = null;
  let parseError: string | null = null;
  try {
    envelope = (await res.json()) as CommandEnvelope;
  } catch (err) {
    parseError = err instanceof Error ? err.message : String(err);
  }

  if (!res.ok) {
    const msg =
      envelope?.error?.message ??
      parseError ??
      `POST ${path} returned ${res.status} ${res.statusText || ""}`.trim();
    throw new ApiError(path, res.status, msg);
  }
  if (!envelope) {
    throw new ApiError(
      path,
      res.status,
      `POST ${path} returned non-JSON body: ${parseError ?? "unknown parse error"}`,
    );
  }
  if (envelope.ok === false) {
    const msg = envelope.error?.message ?? `POST ${path} envelope ok=false with no message`;
    throw new ApiError(path, res.status, msg);
  }
  return envelope;
}

export interface TargetAddBody {
  agent: string;
  path: string;
  ownership?: "managed" | "observed" | "external";
}

export interface BindingAddBody {
  agent: string;
  profile: string;
  matcher_kind: "path_prefix" | "exact_path" | "name";
  matcher_value: string;
  target: string;
  policy_profile?: string;
}

export interface ProjectBody {
  skill: string;
  binding: string;
  target?: string;
  method?: "symlink" | "copy" | "materialize";
}

export interface UseSkillBody {
  agents: string[];
  scope?: "project";
  workspace?: string;
  profile?: string;
  method?: "symlink" | "copy" | "materialize";
  target_root?: string;
  apply?: boolean;
}

export interface SkillAddBody {
  source: string;
  name: string;
}

export interface SkillImportObservedBody {
  target?: string;
}

export interface SkillSaveBody {
  message?: string;
}

export interface SkillReleaseBody {
  version: string;
}

export interface SkillRollbackBody {
  to?: string;
  steps?: number;
}

export interface SkillTrashEntry {
  trash_id: string;
  skill: string;
  original_path: string;
  trashed_at: string;
  source_commit: string;
  trash_path: string;
}

export interface SkillTrashPayload {
  items: SkillTrashEntry[];
}

export interface SkillTrashRestoreBody {
  skill: string;
}

export interface CaptureBody {
  skill?: string;
  binding?: string;
  instance?: string;
  message?: string;
}

export interface OrphanCleanBody {
  delete_live_paths?: boolean;
}

export interface HistoryRepairBody {
  strategy: "local" | "remote";
}

export interface RemoteSetBody {
  url: string;
}

export interface WorkspaceInitBody {
  scan_existing?: boolean;
}

export interface SkillDiffFile {
  path: string;
  added: number;
  removed: number;
  hunks: Array<{ header: string; lines: string[] }>;
  /** Server-side flag: this file's diff was clipped to keep the response bounded. */
  truncated?: boolean;
  /** Number of `+`/`-` lines counted but not retained when `truncated` is true. */
  truncated_lines?: number;
}

export interface SkillDiffPayload {
  ok: boolean;
  data?: {
    skill: string;
    rev_a: string;
    rev_b: string;
    files: SkillDiffFile[];
  };
  error?: { code?: string; message?: string };
}

export interface RegistryObservationEvent {
  event_id: string;
  instance_id: string;
  kind: string;
  path?: string;
  from?: string;
  to?: string;
  observed_at: string;
}

export interface SkillHistoryPayload {
  ok: boolean;
  data?: {
    skill: string;
    count: number;
    events: RegistryObservationEvent[];
  };
  error?: { code?: string; message?: string };
  meta?: { warnings?: string[] };
}

export type SkillDiagnoseStatus = "healthy" | "attention" | "blocked";

export interface SkillDiagnoseCheck {
  section: string;
  id: string;
  ok: boolean;
  severity: "ok" | "warning" | "error" | string;
  message: string;
  next_action?: string | null;
  details?: Record<string, unknown>;
}

export interface SkillDiagnosePayload {
  skill: string;
  healthy: boolean;
  status: SkillDiagnoseStatus | string;
  summary: {
    source_status: string;
    binding_count: number;
    target_count: number;
    projection_count: number;
    failed_check_count: number;
    warning_check_count: number;
    drifted_path_count: number;
    recent_failed_op_count: number;
  };
  checks: SkillDiagnoseCheck[];
  related?: Record<string, unknown>;
}

export interface DoctorCheck {
  section: string;
  id: string;
  ok: boolean;
  severity: "ok" | "warning" | "error" | string;
  message: string;
  next_action?: string | null;
  details?: Record<string, unknown>;
}

export interface DoctorPayload {
  healthy: boolean;
  checks_v1: DoctorCheck[];
  checks?: Record<string, unknown>;
}

async function remoteStatusWithWarnings(signal?: AbortSignal): Promise<ReadResult<RemoteStatusResponse>> {
  const path = "/api/v1/sync/status";
  const result = unwrapReadResult<RemoteStatusResponse>(path, await getJson<unknown>(path, signal));
  const data = parseRemoteStatusResponse(path, result.data);
  return { data, warnings: uniqueWarnings([...result.warnings, ...payloadWarnings(data)]) };
}

export const api = {
  health: (signal?: AbortSignal) => getJsonData<HealthPayload>("/api/v1/health", signal),
  info: (signal?: AbortSignal) => getJsonData<InfoPayload>("/api/v1/workspace/info", signal),
  infoWithWarnings: (signal?: AbortSignal) =>
    getJsonDataWithWarnings<InfoPayload>("/api/v1/workspace/info", signal),
  workspaceStatus: (signal?: AbortSignal) =>
    getJsonData<WorkspaceStatusPayload>("/api/v1/workspace/status", signal),
  workspaceStatusWithWarnings: (signal?: AbortSignal) =>
    getJsonDataWithWarnings<WorkspaceStatusPayload>("/api/v1/workspace/status", signal),
  skills: (signal?: AbortSignal) => getJsonData<SkillsPayload>("/api/v1/skills", signal),
  skillsWithWarnings: (signal?: AbortSignal) =>
    getJsonDataWithWarnings<SkillsPayload>("/api/v1/skills", signal),
  registryStatus: (signal?: AbortSignal) => getJson<RegistryPayload>("/api/v1/registry/status", signal),
  registryStatusWithWarnings: (signal?: AbortSignal) =>
    getJsonWithWarnings<RegistryPayload>("/api/v1/registry/status", signal),
  workspaceDoctor: (signal?: AbortSignal) =>
    getJsonData<DoctorPayload>("/api/v1/workspace/doctor", signal),
  opsHistoryDiagnose: (signal?: AbortSignal) =>
    getJson<OpsHistoryDiagnosePayload>("/api/v1/ops/diagnose", signal),
  ops: (options?: { limit?: number; offset?: number }, signal?: AbortSignal) => {
    const params = new URLSearchParams();
    if (typeof options?.limit === "number") params.set("limit", String(options.limit));
    if (typeof options?.offset === "number") params.set("offset", String(options.offset));
    const qs = params.size > 0 ? `?${params.toString()}` : "";
    return getJson<OpsPayload>(`/api/v1/ops${qs}`, signal);
  },
  opsWithWarnings: (options?: { limit?: number; offset?: number }, signal?: AbortSignal) => {
    const params = new URLSearchParams();
    if (typeof options?.limit === "number") params.set("limit", String(options.limit));
    if (typeof options?.offset === "number") params.set("offset", String(options.offset));
    const qs = params.size > 0 ? `?${params.toString()}` : "";
    return getJsonWithWarnings<OpsPayload>(`/api/v1/ops${qs}`, signal);
  },
  bindingShow: (id: string, signal?: AbortSignal) =>
    getJson<BindingShowPayload>(`/api/v1/bindings/${encodeURIComponent(id)}`, signal),
  targetShow: (id: string, signal?: AbortSignal) =>
    getJson<TargetShowPayload>(`/api/v1/targets/${encodeURIComponent(id)}`, signal),
  remoteStatus: async (signal?: AbortSignal) =>
    (await remoteStatusWithWarnings(signal)).data,
  remoteStatusWithWarnings,
  pending: (signal?: AbortSignal) => getJsonData<PendingPayload>("/api/v1/ops/pending", signal),
  pendingWithWarnings: (signal?: AbortSignal) =>
    getJsonDataWithWarnings<PendingPayload>("/api/v1/ops/pending", signal),

  opsRetry: () => postJson("/api/v1/ops/retry", {}),
  opsPurge: () => postJson("/api/v1/ops/purge", {}),
  remoteSet: (body: RemoteSetBody) => postJson("/api/v1/workspace/remote", body),
  workspaceInit: (body: WorkspaceInitBody) => postJson("/api/v1/workspace/init", body),

  targetAdd: (body: TargetAddBody) => postJson("/api/v1/targets", body),
  targetRemove: (targetId: string) => postJson(`/api/v1/targets/${encodeURIComponent(targetId)}/remove`, {}),
  bindingAdd: (body: BindingAddBody) => postJson("/api/v1/bindings", body),
  bindingRemove: (bindingId: string) => postJson(`/api/v1/bindings/${encodeURIComponent(bindingId)}/remove`, {}),
  skillAdd: (body: SkillAddBody) => postJson("/api/v1/skills", body),
  skillImportObserved: (body: SkillImportObservedBody = {}) =>
    postJson("/api/v1/skills/import-observed", body),
  skillSave: (name: string, body: SkillSaveBody = {}) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/save`, body),
  skillSnapshot: (name: string) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/snapshot`, {}),
  skillRelease: (name: string, body: SkillReleaseBody) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/release`, body),
  skillRollback: (name: string, body: SkillRollbackBody) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/rollback`, body),
  skillTrashList: (signal?: AbortSignal) =>
    getJsonData<SkillTrashPayload>("/api/v1/skills/trash", signal),
  skillTrashAdd: (name: string) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/trash`, {}),
  skillTrashRestore: (trashId: string, body: SkillTrashRestoreBody) =>
    postJson(`/api/v1/skills/trash/${encodeURIComponent(trashId)}/restore`, body),
  skillTrashPurge: (trashId: string) =>
    postJson(`/api/v1/skills/trash/${encodeURIComponent(trashId)}/purge`, {}),
  skillUse: (name: string, body: UseSkillBody) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/use`, body),
  project: (body: ProjectBody) => postJson("/api/v1/projections/project", body),
  capture: (body: CaptureBody) => postJson("/api/v1/projections/capture", body),
  orphanClean: (body: OrphanCleanBody) => postJson("/api/v1/orphans/clean", body),

  syncPush: () => postJson("/api/v1/sync/push", {}),
  syncPull: () => postJson("/api/v1/sync/pull", {}),
  syncReplay: () => postJson("/api/v1/sync/replay", {}),
  opsHistoryRepair: (body: HistoryRepairBody) => postJson("/api/v1/ops/history/repair", body),

  skillHistory: (name: string, signal?: AbortSignal) =>
    getJson<SkillHistoryPayload>(
      `/api/v1/skills/${encodeURIComponent(name)}/history`,
      signal,
    ),

  skillDiagnose: (name: string, signal?: AbortSignal) =>
    getJsonData<SkillDiagnosePayload>(
      `/api/v1/skills/${encodeURIComponent(name)}/diagnose`,
      signal,
    ),

  skillDiff: (name: string, revA?: string, revB?: string, signal?: AbortSignal) => {
    const params = new URLSearchParams();
    if (revA) params.set("rev_a", revA);
    if (revB) params.set("rev_b", revB);
    const qs = params.size > 0 ? `?${params.toString()}` : "";
    return getJson<SkillDiffPayload>(
      `/api/v1/skills/${encodeURIComponent(name)}/diff${qs}`,
      signal,
    );
  },
};
