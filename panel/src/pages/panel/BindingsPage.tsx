import { useEffect, useState } from "react";
import type { Binding, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon } from "../../components/icons/nav_icons";
import { BindingAddForm } from "../../components/panel/forms/BindingAddForm";
import { api, ApiError, type BindingShowPayload } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface BindingsPageProps {
  bindings: Binding[];
  targets: Target[];
  onMutation: () => void;
  readOnly: boolean;
  mutationVersion: number;
}

export function BindingsPage({ bindings, targets, onMutation, readOnly, mutationVersion }: BindingsPageProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const sel = bindings.find((b) => b.id === selectedId) ?? null;

  useEffect(() => {
    if (readOnly) setAddOpen(false);
  }, [readOnly]);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Bindings</h1>
          <div className="subtitle">
            Rules mapping skills to targets. Matchers decide when a binding applies; policy decides whether Loom
            auto-projects.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn primary"
            onClick={() => setAddOpen((v) => !v)}
            disabled={readOnly}
            title={readOnly ? "registry offline" : undefined}
          >
            <PlusIcon /> {addOpen ? "close" : "New binding"}
          </button>
        </div>
      </div>
      <div className="page-body" style={{ padding: 0 }}>
        {addOpen && (
          <div style={{ padding: "0 28px 12px" }}>
            <BindingAddForm
              targets={targets}
              onCancel={() => setAddOpen(false)}
              onSuccess={() => {
                setAddOpen(false);
                onMutation();
              }}
            />
          </div>
        )}
        <div className="two-col" style={{ height: "100%", gap: 0 }}>
          <div style={{ overflow: "auto", borderRight: "1px solid var(--line)" }}>
            <table className="tbl">
              <thead>
                <tr>
                  <th>Binding</th>
                  <th>Skill</th>
                  <th>Target</th>
                  <th>Matcher</th>
                  <th>Method</th>
                  <th>Policy</th>
                </tr>
              </thead>
              <tbody>
                {bindings.map((b) => {
                  const t = targets.find((x) => x.id === b.target);
                  return (
                    <tr
                      key={b.id}
                      className={selectedId === b.id ? "selected" : ""}
                      onClick={() => setSelectedId(b.id === selectedId ? null : b.id)}
                    >
                      <td className="mono dim">{b.id}</td>
                      <td className="name">{b.skill}</td>
                      <td>
                        {t && (
                          <span className="row-flex">
                            <AgentAvatar agent={t.agent} />
                            <span style={{ color: "var(--ink-1)" }}>
                              {t.agent}/{t.profile}
                            </span>
                          </span>
                        )}
                      </td>
                      <td className="mono">{b.matcher}</td>
                      <td>
                        <span className={`chip method ${b.method}`}>{b.method}</span>
                      </td>
                      <td>
                        <span
                          className="chip"
                          style={{ color: b.policy === "auto" ? "var(--ok)" : "var(--warn)" }}
                        >
                          {b.policy}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div style={{ padding: 20, overflow: "auto" }}>
            {sel ? (
              <BindingDetail
                binding={sel}
                targets={targets}
                readOnly={readOnly}
                onMutation={onMutation}
                mutationVersion={mutationVersion}
                onRemoved={(bindingId) => setSelectedId((cur) => (cur === bindingId ? null : cur))}
              />
            ) : (
              <div className="empty">Select a binding to inspect its rules, projections, and default target.</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

type DetailState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: NonNullable<BindingShowPayload["data"]> }
  | { kind: "error"; message: string };

function BindingDetail({
  binding,
  targets,
  readOnly,
  onMutation,
  mutationVersion,
  onRemoved,
}: {
  binding: Binding;
  targets: Target[];
  readOnly: boolean;
  onMutation: () => void;
  mutationVersion: number;
  onRemoved: (bindingId: string) => void;
}) {
  const [state, setState] = useState<DetailState>({ kind: "idle" });
  const [orphanedIds, setOrphanedIds] = useState<string[]>([]);
  const project = useMutation();
  const remove = useMutation();

  useEffect(() => {
    if (readOnly) {
      setState({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ kind: "loading" });
    api
      .bindingShow(binding.id, controller.signal)
      .then((res) => {
        if (controller.signal.aborted) return;
        if (!res.ok || !res.data) {
          setState({ kind: "error", message: res.error?.message ?? "binding fetch returned ok=false" });
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
  }, [binding.id, mutationVersion, readOnly]);

  const t = targets.find((x) => x.id === binding.target);
  const rules = state.kind === "ready" ? state.payload.rules ?? [] : [];
  const projections = state.kind === "ready" ? state.payload.projections ?? [] : [];
  const canProject = !readOnly && binding.skill !== "—" && Boolean(t);
  const actionBusy = project.busy || remove.busy;

  const runProject = () => {
    if (!canProject) return;
    project.run(
      `project ${binding.skill}`,
      () =>
        api.project({
          skill: binding.skill,
          binding: binding.id,
          target: binding.target,
          method: binding.method,
        }),
      onMutation,
    );
  };

  const runRemove = () => {
    if (readOnly) return;
    if (!window.confirm(`Delete binding ${binding.id}? This removes the binding metadata from the registry.`)) return;
    let orphaned: string[] = [];
    remove.run(
      "delete binding",
      async () => {
        const res = await api.bindingRemove(binding.id);
        const ids = res.data?.orphaned_projection_ids;
        if (Array.isArray(ids)) orphaned = ids as string[];
      },
      () => {
        if (orphaned.length > 0) setOrphanedIds(orphaned);
        onRemoved(binding.id);
        onMutation();
      },
    );
  };

  return (
    <div className="detail">
      <h4>{binding.id}</h4>
      <div className="dpath">
        {binding.skill} → {binding.target}
      </div>
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", margin: "14px 0" }}>
        <button
          className="btn primary"
          onClick={runProject}
          disabled={!canProject || actionBusy}
          title={
            readOnly
              ? "registry offline"
              : binding.skill === "—"
              ? "binding has no skill rule"
              : !t
              ? "binding target is missing"
              : "project this binding to its target"
          }
        >
          {project.busy ? "Projecting…" : "Project now"}
        </button>
        <button
          className="btn ghost danger"
          onClick={runRemove}
          disabled={readOnly || actionBusy}
          title={readOnly ? "registry offline" : "delete this binding"}
        >
          {remove.busy ? "Deleting…" : "Delete binding"}
        </button>
      </div>
      {orphanedIds.length > 0 && (
        <div
          style={{
            marginBottom: 12,
            padding: "6px 10px",
            borderRadius: 6,
            border: "1px solid rgba(216,167,50,0.35)",
            background: "rgba(216,167,50,0.08)",
            color: "var(--warn)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          {orphanedIds.length} projection(s) orphaned: {orphanedIds.join(", ")}
          {" — run "}
          <span style={{ fontWeight: 600 }}>loom skill orphan clean</span>
          {" to remove metadata."}
          <button
            style={{ marginLeft: 10, cursor: "pointer", background: "none", border: "none", color: "inherit", textDecoration: "underline", fontSize: "inherit", fontFamily: "inherit", padding: 0 }}
            onClick={() => setOrphanedIds([])}
          >
            dismiss
          </button>
        </div>
      )}
      {(project.error || project.success || remove.error || remove.success) && (
        <div
          style={{
            marginBottom: 12,
            padding: "6px 10px",
            borderRadius: 6,
            border: "1px solid",
            borderColor: project.error || remove.error ? "rgba(216,90,90,0.3)" : "rgba(111,183,138,0.25)",
            background: project.error || remove.error ? "rgba(216,90,90,0.08)" : "rgba(111,183,138,0.08)",
            color: project.error || remove.error ? "var(--err)" : "var(--ok)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          {project.error ?? remove.error ?? `✓ ${project.success ?? remove.success}`}
        </div>
      )}
      <div className="kv">
        <div className="k">Skill</div>
        <div className="v">{binding.skill}</div>
        <div className="k">Target</div>
        <div className="v">
          {t ? `${t.agent}/${t.profile}` : binding.target}
        </div>
        <div className="k">Matcher</div>
        <div className="v mono">{binding.matcher}</div>
        <div className="k">Method</div>
        <div className="v">{binding.method}</div>
        <div className="k">Policy</div>
        <div className="v">{binding.policy}</div>
      </div>

      <div style={{ marginTop: 18 }}>
        <div className="section-title">Rules on chain</div>
        {readOnly && (
          <div className="empty">Registry offline. Start with <span className="mono">loom panel</span> to load live binding rules.</div>
        )}
        {state.kind === "loading" && <div className="empty mono">loading…</div>}
        {state.kind === "error" && <div className="empty" style={{ color: "var(--err)" }}>{state.message}</div>}
        {!readOnly && state.kind === "ready" && rules.length === 0 && <div className="empty">No rules bound.</div>}
        {!readOnly && state.kind === "ready" && rules.length > 0 && (
          <ul style={{ fontSize: 12, paddingLeft: 0, listStyle: "none" }}>
            {rules.map((r, i) => (
              <li key={i} style={{ padding: "6px 0", borderBottom: "1px solid var(--line-soft)" }}>
                <span className="mono" style={{ color: "var(--ink-1)" }}>
                  {r.skill_id}
                </span>
                <span style={{ color: "var(--ink-3)", marginLeft: 8 }}>
                  method={r.method}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div style={{ marginTop: 18 }}>
        <div className="section-title">Projections</div>
        {readOnly && (
          <div className="empty">Registry offline. Start with <span className="mono">loom panel</span> to load live projections.</div>
        )}
        {state.kind === "loading" && <div className="empty mono">loading…</div>}
        {!readOnly && state.kind === "ready" && projections.length === 0 && (
          <div className="empty">No projections realized yet for this binding.</div>
        )}
        {!readOnly && state.kind === "ready" && projections.length > 0 && (
          <ul style={{ fontSize: 12, paddingLeft: 0, listStyle: "none" }}>
            {projections.map((p, i) => (
              <li key={i} style={{ padding: "6px 0", borderBottom: "1px solid var(--line-soft)" }}>
                <span className="mono" style={{ color: "var(--ink-1)" }}>
                  {p.skill_id} → {p.target_id}
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
  );
}
