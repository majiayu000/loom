import { postJson } from "./client";

export interface ConvergencePlanBody {
  agent?: string;
  workspace?: string;
  profile?: string;
  require_runtime?: boolean;
  accept_restart_required?: boolean;
  push_remote?: boolean;
}

export interface ConvergenceApplyBody {
  plan_id: string;
  plan_digest: string;
  idempotency_key: string;
  approvals?: string[];
}

export const convergenceApi = {
  plan: (name: string, body: ConvergencePlanBody) =>
    postJson(`/api/v1/skills/${encodeURIComponent(name)}/convergence/plan`, body),
  apply: (body: ConvergenceApplyBody) => postJson("/api/v1/convergence/apply", body),
};
