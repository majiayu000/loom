import { useEffect, useState } from "react";
import type { Binding, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon } from "../../components/icons/nav_icons";
import { ActionEmptyState } from "../../components/panel/ActionEmptyState";
import { BindingAddForm } from "../../components/panel/forms/BindingAddForm";
import { api, ApiError, type BindingShowPayload } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import type { RegistryProjection } from "../../generated/RegistryProjection";

interface BindingsPageProps {
  bindings: Binding[];
  targets: Target[];
  projections?: RegistryProjection[];
  onMutation: () => void;
  onNewTarget: () => void;
  readOnly: boolean;
  mutationVersion: number;
}

export function BindingsPage({
  bindings,
  targets,
  projections = [],
  onMutation,
  onNewTarget,
  readOnly,
  mutationVersion,
}: BindingsPageProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [deleteLivePaths, setDeleteLivePaths] = useState(false);
  const sel = bindings.find((b) => b.id === selectedId) ?? null;
  const cleanOrphans = useMutation();
  const orphanProjections = projections.filter((p) => !p.binding_id && p.health === "orphaned");
  const canAddBinding = !readOnly && targets.length > 0;
  const addBindingTitle = readOnly ? "registry offline" : targets.length === 0 ? "add a target first" : undefined;

  const runCleanOrphans = () => {
    if (readOnly || orphanProjections.length === 0) return;
    cleanOrphans.run(
      "clean orphaned projections",
      () => api.orphanClean({ delete_live_paths: deleteLivePaths }),
      onMutation,
    );
  };

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
            disabled={!canAddBinding}
            title={addBindingTitle}
          >
            <PlusIcon /> {addOpen ? "close" : "New binding"}
          </button>
        </div>
      </div>
      <div className="page-body" style={{ padding: 0 }}>
        {orphanProjections.length > 0 && (
          <div
            style={{
              margin: "0 28px 12px",
              padding: "10px 12px",
              borderRadius: 6,
              border: "1px solid rgba(216,167,50,0.35)",
              background: "rgba(216,167,50,0.08)",
              display: "flex",
              gap: 12,
              alignItems: "center",
              justifyContent: "space-between",
              flexWrap: "wrap",
            }}
          >
            <div style={{ minWidth: 220 }}>
              <div style={{ color: "var(--warn)", fontWeight: 700, fontSize: 12 }}>
                {orphanProjections.length} orphaned projection{orphanProjections.length === 1 ? "" : "s"}
              </div>
              <div className="mono" style={{ color: "var(--ink-2)", fontSize: 11, marginTop: 3 }}>
                {orphanProjections.map((p) => p.instance_id).join(", ")}
              </div>
            </div>
            <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
              <label className="row-flex" style={{ fontSize: 12, color: "var(--ink-2)" }}>
                <input
                  type="checkbox"
                  checked={deleteLivePaths}
                  onChange={(event) => setDeleteLivePaths(event.currentTarget.checked)}
                  disabled={readOnly || cleanOrphans.busy}
                />
                Delete live paths
              </label>
              <button
                className="btn ghost danger"
                onClick={runCleanOrphans}
                disabled={readOnly || cleanOrphans.busy}
                title={readOnly ? "registry offline" : "clean orphaned projection metadata"}
              >
                {cleanOrphans.busy ? "Cleaning..." : "Clean metadata"}
              </button>
            </div>
            {(cleanOrphans.error || cleanOrphans.success) && (
              <div
                className="mono"
                style={{
                  flexBasis: "100%",
                  color: cleanOrphans.error ? "var(--err)" : "var(--ok)",
                  fontSize: 11,
                }}
              >
                {cleanOrphans.error ?? cleanOrphans.success}
              </div>
            )}
          </div>
        )}
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
        {bindings.length === 0 ? (
          <BindingsEmptyState
            readOnly={readOnly}
            hasTargets={targets.length > 0}
            onAddBinding={() => setAddOpen(true)}
            onNewTarget={onNewTarget}
          />
        ) : (
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
        )}
      </div>
    </>
  );
}

function BindingsEmptyState({
  readOnly,
  hasTargets,
  onAddBinding,
  onNewTarget,
}: {
  readOnly: boolean;
  hasTargets: boolean;
  onAddBinding: () => void;
  onNewTarget: () => void;
}) {
  if (readOnly) {
    return (
      <ActionEmptyState
        title="Registry API offline"
        body="Start the panel backend to load live binding rules before editing routes."
        primaryLabel="Start panel"
        primaryDisabled
        primaryTitle="run the CLI command below"
        primaryIcon={<PlusIcon />}
        command="loom panel"
      />
    );
  }

  if (!hasTargets) {
    return (
      <ActionEmptyState
        title="No bindings yet"
        body="Add a target before creating the first workspace binding."
        primaryLabel="Add target"
        onPrimary={onNewTarget}
        primaryIcon={<PlusIcon />}
        command="loom target add --agent <agent> --path <abs-path> --ownership observed"
      />
    );
  }

  return (
    <ActionEmptyState
      title="No bindings yet"
      body="Create a workspace binding so Loom knows which skill rules map to a target."
      primaryLabel="New binding"
      onPrimary={onAddBinding}
      primaryIcon={<PlusIcon />}
      command="loom workspace binding add --agent <agent> --profile <id> --matcher-kind path-prefix --matcher-value <workspace> --target <target-id>"
    />
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
    remove.run(
      "delete binding",
      () => api.bindingRemove(binding.id),
      () => {
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
