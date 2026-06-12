import { vi } from "vitest";

export function jsonResponse(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: vi.fn().mockResolvedValue(body),
  } as unknown as Response;
}

export function errorResponse(status: number, body: unknown): Response {
  return {
    ok: false,
    status,
    statusText: "Service Unavailable",
    json: vi.fn().mockResolvedValue(body),
  } as unknown as Response;
}
