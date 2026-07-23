import { useState } from "react";
import { AGENT_OPTIONS } from "../../lib/agent_options";
import { convergenceApi } from "../../lib/api/convergence";

type PlanData = {
  plan_id?: unknown;
  plan_digest?: unknown;
  safe_to_apply?: unknown;
  execution_enabled?: unknown;
  effects?: unknown;
  risks?: unknown;
  input_conflicts?: unknown;
  required_approvals?: unknown;
};

function newIdempotencyKey(): string {
  const suffix = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `panel-converge-${suffix}`;
}

function listLength(value: unknown): number {
  return Array.isArray(value) ? value.length : 0;
}

function reviewValue(value: unknown): string {
  return JSON.stringify(value ?? [], null, 2);
}

export function SkillConvergencePanel({ skillName, supported, onApplied }: { skillName: string; supported: boolean; onApplied: () => void }) {
  const [agent, setAgent] = useState("");
  const [requireRuntime, setRequireRuntime] = useState(false);
  const [acceptRestart, setAcceptRestart] = useState(false);
  const [pushRemote, setPushRemote] = useState(false);
  const [plan, setPlan] = useState<PlanData | null>(null);
  const [idempotencyKey, setIdempotencyKey] = useState("");
  const [reviewed, setReviewed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!supported) return null;

  const planId = typeof plan?.plan_id === "string" ? plan.plan_id : "";
  const planDigest = typeof plan?.plan_digest === "string" ? plan.plan_digest : "";
  const planSafe = plan?.safe_to_apply === true && plan?.execution_enabled === true;

  const invalidatePlan = () => {
    setPlan(null);
    setIdempotencyKey("");
    setReviewed(false);
    setMessage(null);
    setError(null);
  };

  const createPlan = async () => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await convergenceApi.plan(skillName, {
        agent: agent || undefined,
        require_runtime: requireRuntime,
        accept_restart_required: requireRuntime && acceptRestart,
        push_remote: pushRemote,
      });
      setPlan((result.data ?? {}) as PlanData);
      setIdempotencyKey(newIdempotencyKey());
      setReviewed(false);
    } catch (cause) {
      setPlan(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const applyPlan = async () => {
    if (!planSafe || !reviewed || !planId || !planDigest || !idempotencyKey) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await convergenceApi.apply({ plan_id: planId, plan_digest: planDigest, idempotency_key: idempotencyKey, approvals: [] });
      const data = (result.data ?? {}) as Record<string, unknown>;
      const blockers = Array.isArray(data.completion_blockers) ? data.completion_blockers.filter((item): item is string => typeof item === "string") : [];
      setMessage(data.complete === true ? "Convergence complete." : `Local apply recorded; blockers: ${blockers.join(", ") || "evidence incomplete"}.`);
      onApplied();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel" aria-label="Skill convergence" style={{ marginTop: 14 }}>
      <div className="panel-head"><h3>Converge</h3><span className="panel-hint">reviewed plan → digest-confirmed apply</span></div>
      <div className="card-body" style={{ display: "grid", gap: 10 }}>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <select aria-label="Convergence agent" value={agent} onChange={(event) => { setAgent(event.target.value); invalidatePlan(); }} disabled={busy}>
            <option value="">all selected runtimes</option>
            {AGENT_OPTIONS.map((option) => <option key={option.slug} value={option.slug}>{option.label}</option>)}
          </select>
          <label className="chip"><input type="checkbox" checked={requireRuntime} onChange={(event) => { setRequireRuntime(event.target.checked); if (!event.target.checked) setAcceptRestart(false); invalidatePlan(); }} disabled={busy} /> require runtime</label>
          <label className="chip"><input type="checkbox" checked={acceptRestart} onChange={(event) => { setAcceptRestart(event.target.checked); invalidatePlan(); }} disabled={busy || !requireRuntime} /> accept restart-required</label>
          <label className="chip"><input type="checkbox" checked={pushRemote} onChange={(event) => { setPushRemote(event.target.checked); invalidatePlan(); }} disabled={busy} /> push remote last</label>
          <button className="btn-ghost sm" type="button" onClick={createPlan} disabled={busy}>{busy && !plan ? "planning…" : "Plan convergence"}</button>
        </div>
        {plan && <div className="mono" data-testid="convergence-plan-review"><div>plan_id: {planId || "missing"}</div><div>plan_digest: {planDigest || "missing"}</div><div>effects: {listLength(plan.effects)} · risks: {listLength(plan.risks)} · conflicts: {listLength(plan.input_conflicts)} · approvals: {listLength(plan.required_approvals)}</div><div>safe_to_apply: {String(plan.safe_to_apply === true)} · execution_enabled: {String(plan.execution_enabled === true)}</div>{(["effects", "risks", "input_conflicts", "required_approvals"] as const).map((field) => <details key={field}><summary>{field}</summary><pre style={{ whiteSpace: "pre-wrap" }}>{reviewValue(plan[field])}</pre></details>)}</div>}
        {plan && <label><input type="checkbox" checked={reviewed} onChange={(event) => setReviewed(event.target.checked)} disabled={!planSafe || busy} /> I reviewed this exact plan id, digest, effects, risks, conflicts, and approvals.</label>}
        {plan && <div style={{ display: "flex", gap: 8 }}><input aria-label="Convergence idempotency key" value={idempotencyKey} onChange={(event) => setIdempotencyKey(event.target.value)} disabled={busy} /><button className="btn-grad sm" type="button" onClick={applyPlan} disabled={!planSafe || !reviewed || !planId || !planDigest || !idempotencyKey || busy}>{busy ? "applying…" : "Apply reviewed plan"}</button></div>}
        {plan && !planSafe && <div className="mutation-note" data-tone="err">Plan is blocked; inspect conflicts, risks, and required approvals before retrying.</div>}
        {error && <div className="mutation-note" data-tone="err">{error}</div>}
        {message && <div className="mutation-note" data-tone="warn">{message}</div>}
      </div>
    </section>
  );
}
