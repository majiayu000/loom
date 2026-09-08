import { invoke } from "@tauri-apps/api/core";

export interface Team {
  id: string;
  name: string;
  owner_user_id: string;
}
export interface Skill {
  id: string;
  slug: string;
  title: string;
  description: string;
  example: string;
  maintainer_id: string;
  recommended_version_id: string | null;
  archived_at: string | null;
  revision: number;
}
export interface Version {
  id: string;
  version: string;
  sha256: string;
  release_notes: string;
  created_at: string;
}
export interface Member {
  user_id: string;
  email?: string;
  joined_at: string;
}
export interface Session {
  access_token: string;
  user: { id: string; email?: string };
}
export interface Config {
  cloud_api_url: string;
  auth_url: string;
  auth_public_key: string;
}
export interface Envelope {
  ok: boolean;
  data?: Record<string, unknown>;
  error?: { message?: string };
}

export const isDesktop = () => "__TAURI_INTERNALS__" in window;
export const native = <T>(name: string, args?: Record<string, unknown>) =>
  invoke<T>(name, args);
const configured: Config = {
  cloud_api_url: import.meta.env.VITE_LOOM_CLOUD_URL ?? "",
  auth_url: import.meta.env.VITE_LOOM_AUTH_URL ?? "",
  auth_public_key: import.meta.env.VITE_LOOM_AUTH_PUBLIC_KEY ?? "",
};
let session: Session | null = null;
let config = configured;

function base(value: string) {
  const url = new URL(value);
  if (
    url.protocol !== "https:" &&
    !(
      url.protocol === "http:" &&
      ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
    )
  ) {
    throw new Error("服务地址必须使用 HTTPS，本机开发可使用 HTTP。");
  }
  if (url.username || url.password || url.search || url.hash)
    throw new Error("服务地址不能包含凭证、查询或片段。");
  return value.replace(/\/$/, "");
}

export async function readConfig(): Promise<Config> {
  if (isDesktop()) config = await native<Config>("get_cloud_config");
  return config;
}

export async function saveConfig(value: Config) {
  base(value.cloud_api_url);
  base(value.auth_url);
  if (isDesktop()) await native("save_cloud_config", { config: value });
  config = value;
  session = null;
}

async function readResponse(response: Response) {
  const body = await response.json().catch(() => {
    throw new Error(`服务返回了无法读取的响应（${response.status}）。`);
  });
  if (!response.ok) {
    if (response.status === 401) session = null;
    throw new Error(
      body.error?.message ??
        body.msg ??
        body.error_description ??
        body.message ??
        `请求失败（${response.status}）`,
    );
  }
  return body;
}

async function auth(path: string, body: object) {
  if (!config.auth_url || !config.auth_public_key)
    throw new Error("请先配置登录服务地址与公开客户端密钥。");
  return readResponse(
    await fetch(`${base(config.auth_url)}/auth/v1/${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        apikey: config.auth_public_key,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(30_000),
    }),
  );
}

export async function requestOtp(email: string) {
  if (isDesktop()) return native("request_otp", { email });
  await auth("otp", { email, create_user: true });
}

export async function verifyOtp(
  email: string,
  token: string,
): Promise<{ id: string; email?: string }> {
  if (isDesktop()) return native("verify_otp", { email, token });
  const result: Session = await auth("verify", { email, token, type: "email" });
  if (!result.access_token || !result.user?.id)
    throw new Error("登录响应缺少身份信息。");
  session = result;
  return result.user;
}

export async function signOut() {
  session = null;
  if (isDesktop()) await native("logout");
}

export async function request<T>(
  path: string,
  options: {
    method?: string;
    body?: unknown;
    signal?: AbortSignal;
    ifMatch?: number;
    idempotencyKey?: string;
  } = {},
): Promise<T> {
  if (!path.startsWith("/v1/")) throw new Error("无效的云端请求路径。");
  options.signal?.throwIfAborted();
  if (isDesktop()) {
    const result = await native<{ data: T }>("cloud_request", {
      method: options.method ?? "GET",
      path,
      body: options.body ?? null,
      ifMatch: options.ifMatch === undefined ? null : String(options.ifMatch),
    });
    options.signal?.throwIfAborted();
    return result.data;
  }
  if (!session) throw new Error("请先登录团队账户。");
  if (!config.cloud_api_url) throw new Error("请先配置团队服务地址。");
  const form = options.body instanceof FormData;
  const response = await fetch(`${base(config.cloud_api_url)}${path}`, {
    method: options.method ?? "GET",
    headers: {
      Authorization: `Bearer ${session.access_token}`,
      ...(form ? {} : { "Content-Type": "application/json" }),
      ...(options.method === "POST"
        ? { "Idempotency-Key": options.idempotencyKey ?? crypto.randomUUID() }
        : {}),
      ...(options.ifMatch === undefined
        ? {}
        : { "If-Match": String(options.ifMatch) }),
    },
    body:
      options.body === undefined
        ? undefined
        : form
          ? (options.body as FormData)
          : JSON.stringify(options.body),
    signal: options.signal ?? AbortSignal.timeout(30_000),
  });
  return (await readResponse(response)).data;
}

export const segment = (value: string) => encodeURIComponent(value);
export const teamPath = (team: string) => `/v1/teams/${segment(team)}`;
export function requireSuccess(value: Envelope): Record<string, unknown> {
  if (!value.ok)
    throw new Error(value.error?.message ?? "Loom 操作失败，请查看诊断信息。");
  return value.data ?? {};
}
