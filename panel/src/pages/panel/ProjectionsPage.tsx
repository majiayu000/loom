import { useMemo, useState } from "react";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { Binding, Target } from "../../lib/types";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import { RefreshIcon } from "../../components/icons/nav_icons";

interface ProjectionsPageProps {
  projections: RegistryProjection[];
  targets: Target[];
  bindings: Binding[];
  readOnly: boolean;
  onMutation: () => void;
}

type ProjectionFilter = "all" | "healthy" | "drifted" | "orphaned";

const FILTERS: ProjectionFilter[] = ["all", "healthy", "drifted", "orphaned"];
const MONO_VALUE_STYLE = {
  overflowWrap: "anywhere",
  minWidth: 0,
  whiteSpace: "normal",
  wordBreak: "break-word",
} as const;

function shortRev(rev: string): string {
  return rev ? rev.slice(0, 8) : "-";
}

function targetLabel(targets: Target[], targetId: string): string {
  const target = targets.find((item) => item.id === targetId);
  return target ? `${target.agent}/${target.profile}` : targetId;
}

function methodForProject(method: string): "symlink" | "copy" | "materialize" {
  if (method === "copy" || method === "materialize") return method;
  return "symlink";
}

function healthClass(projection: RegistryProjection): string {
  if (projection.health === "healthy" && !projection.observed_drift) return "ok";
  if (projection.health === "orphaned") return "warn";
  return "err";
}

export function ProjectionsPage({ projections, targets, bindings, readOnly, onMutation }: ProjectionsPageProps) {
  const [filter, setFilter] = useState<ProjectionFilter>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [deleteLivePaths, setDeleteLivePaths] = useState(false);
  const action = useMutation();

  const filtered = useMemo(() => {
    return projections.filter((projection) => {
      if (filter === "all") return true;
      if (filter === "drifted") return projection.observed_drift || projection.health === "drifted";
      return projection.health === filter;
    });
  }, [filter, projections]);

  const selected = projections.find((projection) => projection.instance_id === selectedId) ?? filtered[0] ?? null;
  const selectedBinding = selected?.binding_id
    ? bindings.find((binding) => binding.id === selected.binding_id)
    : undefined;
  const canProject = Boolean(selected?.binding_id) && !readOnly && !action.busy;
  const canCapture = Boolean(selected) && !readOnly && !action.busy;
  const canCleanOrphan =
    Boolean(selected) && selected?.health === "orphaned" && !selected.binding_id && !readOnly && !action.busy;

  const capture = () => {
    if (!selected || !canCapture) return;
    action.run(
      "capture projection",
      () => api.capture({ instance: selected.instance_id }),
      onMutation,
    );
  };

  const project = () => {
    if (!selected || !selected.binding_id || !canProject) return;
    action.run(
      "re-project",
      () =>
        api.project({
          skill: selected.skill_id,
          binding: selected.binding_id!,
          target: selected.target_id,
          method: methodForProject(selected.method),
        }),
      onMutation,
    );
  };

  const cleanOrphan = () => {
    if (!canCleanOrphan) return;
    action.run(
      "clean orphaned projection",
      () => api.orphanClean({ delete_live_paths: deleteLivePaths }),
      onMutation,
    );
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Projections</h1>
          <div className="subtitle">Materialized skill instances across registered targets.</div>
        </div>
        <div className="header-actions">
          <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
            {FILTERS.map((item) => (
              <button key={item} className={`btn ${filter === item ? "primary" : "ghost"}`} onClick={() => setFilter(item)}>
                {item}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div className="page-body" style={{ padding: 0 }}>
        <div className="two-col projections-layout">
          <div className="projections-list">
            <table className="tbl">
              <thead>
                <tr>
                  <th>Instance</th>
                  <th>Skill</th>
                  <th>Target</th>
                  <th>Method</th>
                  <th>Health</th>
                  <th>Rev</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((projection) => (
                  <tr
                    key={projection.instance_id}
                    className={selected?.instance_id === projection.instance_id ? "selected" : ""}
                    onClick={() => setSelectedId(projection.instance_id)}
                  >
                    <td className="mono dim">{projection.instance_id}</td>
                    <td className="name">{projection.skill_id}</td>
                    <td>{targetLabel(targets, projection.target_id)}</td>
                    <td>{projection.method}</td>
                    <td>
                      <span className={`badge ${healthClass(projection)}`}>
                        {projection.observed_drift ? "drifted" : projection.health}
                      </span>
                    </td>
                    <td className="mono dim">{shortRev(projection.last_applied_rev)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {filtered.length === 0 && (
              <div className="empty-panel compact">
                <div className="empty-panel-title">
                  {projections.length === 0 ? "No projections applied" : "No projections match this filter"}
                </div>
                <div className="empty-panel-copy">
                  {projections.length === 0
                    ? "Apply a binding to materialize a skill into a target. Projection rows will show method, health, revision, and drift."
                    : "Choose another health filter or refresh live data to review the current projection set."}
                </div>
                <div className="empty-panel-actions">
                  <button className="btn ghost" onClick={onMutation} title="re-fetch projections">
                    <RefreshIcon /> Refresh
                  </button>
                </div>
              </div>
            )}
          </div>
          <div className="projections-detail-panel">
            {selected ? (
              <ProjectionDetail
                projection={selected}
                binding={selectedBinding}
                targetLabel={targetLabel(targets, selected.target_id)}
                readOnly={readOnly}
                actionBusy={action.busy}
                canCapture={canCapture}
                canProject={canProject}
                canCleanOrphan={canCleanOrphan}
                deleteLivePaths={deleteLivePaths}
                setDeleteLivePaths={setDeleteLivePaths}
                onCapture={capture}
                onProject={project}
                onCleanOrphan={cleanOrphan}
                message={action.error ?? action.success}
                messageTone={action.error ? "var(--err)" : "var(--ok)"}
              />
            ) : (
              <div className="empty-panel">
                <div className="empty-panel-title">Projection details will appear here</div>
                <div className="empty-panel-copy">
                  Select a projection to inspect its instance path, binding, target, applied revision, and available
                  repair actions.
                </div>
                <ul className="empty-panel-list">
                  <li>Healthy rows are in sync with the recorded revision.</li>
                  <li>Drifted rows can be captured or re-projected.</li>
                  <li>Orphaned rows can be cleaned when their binding was removed.</li>
                </ul>
              </div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

function ProjectionDetail({
  projection,
  binding,
  targetLabel,
  readOnly,
  actionBusy,
  canCapture,
  canProject,
  canCleanOrphan,
  deleteLivePaths,
  setDeleteLivePaths,
  onCapture,
  onProject,
  onCleanOrphan,
  message,
  messageTone,
}: {
  projection: RegistryProjection;
  binding?: Binding;
  targetLabel: string;
  readOnly: boolean;
  actionBusy: boolean;
  canCapture: boolean;
  canProject: boolean;
  canCleanOrphan: boolean;
  deleteLivePaths: boolean;
  setDeleteLivePaths: (next: boolean) => void;
  onCapture: () => void;
  onProject: () => void;
  onCleanOrphan: () => void;
  message: string | null;
  messageTone: string;
}) {
  return (
    <div className="card">
      <div className="card-head">
        <h3>{projection.skill_id}</h3>
        <span className={`chip ${healthClass(projection)}`}>{projection.health}</span>
      </div>
      <div className="card-body">
        <div style={{ display: "grid", gap: 10, marginBottom: 14, fontSize: 12 }}>
          <DetailRow label="Instance" value={projection.instance_id} mono />
          <DetailRow label="Target" value={targetLabel} />
          <DetailRow label="Binding" value={projection.binding_id ?? "-"} mono />
          <DetailRow label="Method" value={projection.method} />
          <DetailRow label="Revision" value={projection.last_applied_rev || "-"} mono />
          <DetailRow label="Path" value={projection.materialized_path} mono />
        </div>

        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
          <button
            className="btn"
            onClick={onCapture}
            disabled={!canCapture}
            title={readOnly ? "registry offline" : undefined}
          >
            Capture
          </button>
          <button
            className="btn primary"
            onClick={onProject}
            disabled={!canProject}
            title={
              readOnly
                ? "registry offline"
                : binding
                  ? `project via ${binding.id}`
                  : "projection has no binding"
            }
          >
            <RefreshIcon /> {actionBusy ? "Working..." : "Re-project"}
          </button>
          {projection.health === "orphaned" && !projection.binding_id && (
            <>
              <label className="row-flex" style={{ fontSize: 12, color: "var(--ink-2)" }}>
                <input
                  type="checkbox"
                  checked={deleteLivePaths}
                  onChange={(event) => setDeleteLivePaths(event.currentTarget.checked)}
                  disabled={readOnly || actionBusy}
                />
                Delete live path
              </label>
              <button
                className="btn ghost danger"
                onClick={onCleanOrphan}
                disabled={!canCleanOrphan}
                title={readOnly ? "registry offline" : undefined}
              >
                Clean orphan
              </button>
            </>
          )}
        </div>

        {message && (
          <div className="mono" style={{ color: messageTone, marginTop: 12, fontSize: 11 }}>
            {message}
          </div>
        )}
      </div>
    </div>
  );
}

function DetailRow({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "82px minmax(0, 1fr)", gap: 12, alignItems: "start" }}>
      <div style={{ color: "var(--ink-2)" }}>{label}</div>
      <div className={mono ? "mono" : undefined} style={mono ? MONO_VALUE_STYLE : undefined}>
        {value}
      </div>
    </div>
  );
}
