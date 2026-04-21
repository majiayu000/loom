import { useEffect, useState } from "react";
import type { Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon } from "../../components/icons/nav_icons";
import { TargetAddForm } from "../../components/panel/forms/TargetAddForm";
import { api, ApiError, type TargetShowPayload } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface TargetsPageProps {
  targets: Target[];
  skills: Skill[];
  selectedTarget: string | null;
  onSelectTarget: (id: string) => void;
  onMutation: () => void;
  readOnly: boolean;
}

export function TargetsPage({ targets, skills, selectedTarget, onSelectTarget, onMutation, readOnly }: TargetsPageProps) {
  const [addOpen, setAddOpen] = useState(false);
  const sel = targets.find((t) => t.id === selectedTarget) ?? null;

  useEffect(() => {
    if (readOnly) setAddOpen(false);
  }, [readOnly]);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Targets</h1>
          <div className="subtitle">
            Agent skill directories Loom knows about. Ownership determines whether Loom writes, reads, or stays
            hands-off.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn primary"
            onClick={() => setAddOpen((v) => !v)}
            disabled={readOnly}
            title={readOnly ? "registry offline" : undefined}
          >
            <PlusIcon /> {addOpen ? "close" : "target add"}
          </button>
        </div>
      </div>
      <div className="page-body">
        {addOpen && (
          <TargetAddForm
            onCancel={() => setAddOpen(false)}
            onSuccess={() => {
              setAddOpen(false);
              onMutation();
            }}
          />
        )}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 12 }}>
          {targets.map((t) => {
            const isSel = selectedTarget === t.id;
            const inbound = skills.filter((s) => s.targets.includes(t.id)).length;
            return (
              <div
                key={t.id}
                className="card"
                style={{ cursor: "pointer", borderColor: isSel ? "var(--accent)" : "var(--line)" }}
                onClick={() => onSelectTarget(t.id)}
              >
                <div style={{ padding: "14px 16px", display: "flex", alignItems: "center", gap: 12 }}>
                  <AgentAvatar agent={t.agent} size={32} radius={8} />
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 14, color: "var(--ink-0)", fontWeight: 500 }}>
                      {t.agent}
                      <span style={{ color: "var(--ink-3)" }}> / </span>
                      {t.profile}
                    </div>
                    <div className="mono" style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 2 }}>
                      {t.path}
                    </div>
                  </div>
                  <span className={`chip ${t.ownership}`}>
                    <span className="dot" />
                    {t.ownership}
                  </span>
                </div>
                <div
                  style={{
                    padding: "10px 16px",
                    borderTop: "1px solid var(--line-soft)",
                    display: "flex",
                    gap: 18,
                    fontSize: 11.5,
                    color: "var(--ink-2)",
                  }}
                >
                  <span>
                    <b style={{ color: "var(--ink-0)" }}>{t.skills}</b> skills present
                  </span>
                  <span>
                    <b style={{ color: "var(--ink-0)" }}>{inbound}</b> inbound bindings
                  </span>
                  <span style={{ marginLeft: "auto", color: "var(--ink-3)" }}>synced {t.lastSync}</span>
                </div>
              </div>
            );
          })}
        </div>
        {sel && (
          <div className="card" style={{ marginTop: 16 }}>
            <div className="card-head">
              <h3>
                {sel.agent}/{sel.profile}
                <span className="mono" style={{ color: "var(--ink-3)", marginLeft: 8, fontSize: 12 }}>
                  {sel.id}
                </span>
              </h3>
              <span className={`chip ${sel.ownership}`}>
                <span className="dot" />
                {sel.ownership}
              </span>
            </div>
            <div className="card-body">
              <TargetDetail
                target={sel}
                readOnly={readOnly}
                onMutation={onMutation}
                onRemoved={() => onSelectTarget(sel.id)}
              />
            </div>
          </div>
        )}
      </div>
    </>
  );
}

type DetailState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: NonNullable<TargetShowPayload["data"]> }
  | { kind: "error"; message: string };

function TargetDetail({
  target,
  readOnly,
  onMutation,
  onRemoved,
}: {
  target: Target;
  readOnly: boolean;
  onMutation: () => void;
  onRemoved: () => void;
}) {
  const [state, setState] = useState<DetailState>({ kind: "idle" });
  const remove = useMutation();

  useEffect(() => {
    if (readOnly) {
      setState({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ kind: "loading" });
    api
      .targetShow(target.id, controller.signal)
      .then((res) => {
        if (controller.signal.aborted) return;
        if (!res.ok || !res.data) {
          setState({ kind: "error", message: res.error?.message ?? "target fetch returned ok=false" });
          return;
        }
        setState({ kind: "ready", payload: res.data });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setState({ kind: "error", message });
      });
    return () => controller.abort();
  }, [readOnly, target.id]);

  const bindings = state.kind === "ready" ? state.payload.bindings ?? [] : [];
  const projections = state.kind === "ready" ? state.payload.projections ?? [] : [];
  const canRemove = !readOnly && state.kind === "ready" && bindings.length === 0;

  const runRemove = () => {
    if (!canRemove) return;
    if (!window.confirm(`Delete target ${target.id}? The target directory itself is not removed.`)) return;
    remove.run("delete target", () => api.targetRemove(target.id), () => {
      onRemoved();
      onMutation();
    });
  };

  return (
    <div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 14 }}>
        <button
          className="btn ghost danger"
          onClick={runRemove}
          disabled={!canRemove || remove.busy}
          title={
            readOnly
              ? "registry offline"
              : state.kind !== "ready"
              ? "load target details first"
              : bindings.length > 0
              ? "remove bindings that reference this target first"
              : "delete this target"
          }
        >
          {remove.busy ? "Deleting…" : "Delete target"}
        </button>
        {!readOnly && bindings.length > 0 && (
          <span className="hint">Target is referenced by {bindings.length} binding{bindings.length === 1 ? "" : "s"}.</span>
        )}
      </div>
      {(remove.error || remove.success) && (
        <div
          style={{
            marginBottom: 12,
            padding: "6px 10px",
            borderRadius: 6,
            border: "1px solid",
            borderColor: remove.error ? "rgba(216,90,90,0.3)" : "rgba(111,183,138,0.25)",
            background: remove.error ? "rgba(216,90,90,0.08)" : "rgba(111,183,138,0.08)",
            color: remove.error ? "var(--err)" : "var(--ok)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          {remove.error ?? `✓ ${remove.success}`}
        </div>
      )}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 18, fontSize: 12 }}>
      <div>
        <div className="section-title">Bindings → this target</div>
        {readOnly && (
          <div className="empty">Registry offline. Start with <span className="mono">loom panel</span> to load live bindings.</div>
        )}
        {state.kind === "loading" && <div className="empty mono">loading…</div>}
        {state.kind === "error" && <div className="empty" style={{ color: "var(--err)" }}>{state.message}</div>}
        {!readOnly && state.kind === "ready" && bindings.length === 0 && <div className="empty">No bindings point here yet.</div>}
        {!readOnly && state.kind === "ready" && bindings.length > 0 && (
          <ul style={{ paddingLeft: 0, listStyle: "none" }}>
            {bindings.map((b) => (
              <li key={b.binding_id} style={{ padding: "6px 0", borderBottom: "1px solid var(--line-soft)" }}>
                <span className="mono" style={{ color: "var(--ink-1)" }}>
                  {b.binding_id}
                </span>
                <span style={{ color: "var(--ink-3)", marginLeft: 8 }}>
                  {b.workspace_matcher.kind}:{b.workspace_matcher.value}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div>
        <div className="section-title">Projections realized</div>
        {readOnly && (
          <div className="empty">Registry offline. Start with <span className="mono">loom panel</span> to load live projections.</div>
        )}
        {state.kind === "loading" && <div className="empty mono">loading…</div>}
        {!readOnly && state.kind === "ready" && projections.length === 0 && (
          <div className="empty">No projections realized yet.</div>
        )}
        {!readOnly && state.kind === "ready" && projections.length > 0 && (
          <ul style={{ paddingLeft: 0, listStyle: "none" }}>
            {projections.map((p, i) => (
              <li key={i} style={{ padding: "6px 0", borderBottom: "1px solid var(--line-soft)" }}>
                <span className="mono" style={{ color: "var(--ink-1)" }}>
                  {p.skill_id}
                </span>
                <span style={{ color: "var(--ink-3)", marginLeft: 8 }}>
                  {p.method} · rev {p.last_applied_rev?.slice(0, 8) ?? "—"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
      </div>
    </div>
  );
}
