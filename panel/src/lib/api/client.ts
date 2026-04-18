import type {
  HealthPayload,
  InfoPayload,
  PendingPayload,
  RemotePayload,
  V3Payload,
} from "../../types";

export interface SkillsPayload {
  skills?: string[];
}

export interface RemoteStatusResponse {
  remote?: RemotePayload;
  warnings?: string[];
}

export interface CommandEnvelope {
  ok: boolean;
  cmd: string;
  request_id: string;
  data?: Record<string, unknown>;
  error?: { code?: string; message?: string; details?: Record<string, unknown> };
  meta?: { warnings?: string[] };
}

export class ApiError extends Error {
  constructor(public readonly path: string, public readonly status: number, message: string) {
    super(message);
    this.name = "ApiError";
  }
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, { signal });
  if (!res.ok) {
    throw new ApiError(path, res.status, `GET ${path} returned ${res.status}`);
  }
  return (await res.json()) as T;
}

async function postJson(path: string, body: unknown): Promise<CommandEnvelope> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const envelope = (await res.json().catch(() => ({}))) as CommandEnvelope;
  if (!res.ok || envelope.ok === false) {
    const msg = envelope.error?.message ?? `POST ${path} returned ${res.status}`;
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
  matcher_kind: "path-prefix" | "exact-path" | "name";
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

export interface CaptureBody {
  skill?: string;
  binding?: string;
  instance?: string;
  message?: string;
}

export const api = {
  health: (signal?: AbortSignal) => getJson<HealthPayload>("/api/health", signal),
  info: (signal?: AbortSignal) => getJson<InfoPayload>("/api/info", signal),
  skills: (signal?: AbortSignal) => getJson<SkillsPayload>("/api/skills", signal),
  v3Status: (signal?: AbortSignal) => getJson<V3Payload>("/api/v3/status", signal),
  remoteStatus: (signal?: AbortSignal) => getJson<RemoteStatusResponse>("/api/remote/status", signal),
  pending: (signal?: AbortSignal) => getJson<PendingPayload>("/api/pending", signal),

  targetAdd: (body: TargetAddBody) => postJson("/api/v3/targets", body),
  targetRemove: (targetId: string) => postJson(`/api/v3/targets/${encodeURIComponent(targetId)}/remove`, {}),
  bindingAdd: (body: BindingAddBody) => postJson("/api/v3/bindings", body),
  bindingRemove: (bindingId: string) => postJson(`/api/v3/bindings/${encodeURIComponent(bindingId)}/remove`, {}),
  project: (body: ProjectBody) => postJson("/api/v3/project", body),
  capture: (body: CaptureBody) => postJson("/api/v3/capture", body),

  syncPush: () => postJson("/api/sync/push", {}),
  syncPull: () => postJson("/api/sync/pull", {}),
  syncReplay: () => postJson("/api/sync/replay", {}),
};
