import type {
  HealthPayload,
  InfoPayload,
  PendingPayload,
  RemotePayload,
  V3Payload,
} from "../../types";
import type { V3Binding } from "../../generated/V3Binding";
import type { V3Projection } from "../../generated/V3Projection";
import type { V3Rule } from "../../generated/V3Rule";
import type { V3Target } from "../../generated/V3Target";

export interface BindingShowPayload {
  ok: boolean;
  data?: {
    state_model?: string;
    binding: V3Binding;
    default_target?: V3Target | null;
    rules?: V3Rule[];
    projections?: V3Projection[];
  };
  error?: { code?: string; message?: string };
}

export interface TargetShowPayload {
  ok: boolean;
  data?: {
    state_model?: string;
    target: V3Target;
    bindings?: V3Binding[];
    rules?: V3Rule[];
    projections?: V3Projection[];
  };
  error?: { code?: string; message?: string };
}

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
  bindingShow: (id: string, signal?: AbortSignal) =>
    getJson<BindingShowPayload>(`/api/v3/bindings/${encodeURIComponent(id)}`, signal),
  targetShow: (id: string, signal?: AbortSignal) =>
    getJson<TargetShowPayload>(`/api/v3/targets/${encodeURIComponent(id)}`, signal),
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
