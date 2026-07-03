import { useEffect, useMemo, useState } from "react";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { Binding, Target } from "../../lib/types";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import { RefreshIcon } from "../../components/icons/nav_icons";
import { EmptyState } from "../../components/panel/EmptyState";

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
  const cleanOrphans = useMutation();

  const filtered = useMemo(() => {
    return projections.filter((projection) => {
      if (filter === "all") return true;
      if (filter === "drifted") return projection.observed_drift || projection.health === "drifted";
      return projection.health === filter;
    });
  }, [filter, projections]);
  const orphanProjections = useMemo(
    () => projections.filter((projection) => projection.health === "orphaned" && !projection.binding_id),
    [projections],
  );

  useEffect(() => {
    if (selectedId && !filtered.some((projection) => projection.instance_id === selectedId)) {
      setSelectedId(null);
    }
  }, [filtered, selectedId]);

  const selected = filtered.find((projection) => projection.instance_id === selectedId) ?? filtered[0] ?? null;
  const selectedBinding = selected?.binding_id
    ? bindings.find((binding) => binding.id === selected.binding_id)
    : undefined;
  const canProject = Boolean(selected?.binding_id) && !readOnly && !action.busy;
  const canCapture = Boolean(selected) && !readOnly && !action.busy;

  const capture = () => {
    if (!selected || !canCapture) return;
    action.run(
      "commit projection",
      () => api.commitProjection({ skill: selected.skill_id, instance: selected.instance_id }),
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

  const cleanOrphansBulk = () => {
    if (readOnly || orphanProjections.length === 0 || cleanOrphans.busy) return;
    if (
      deleteLivePaths &&
      !window.confirm(
        `Delete live paths for ${orphanProjections.length} orphaned projection${
          orphanProjections.length === 1 ? "" : "s"
        }? This removes live projection directories as well as registry metadata.`,
      )
    ) {
      return;
    }
    cleanOrphans.run(
      deleteLivePaths ? "delete orphaned projection live paths" : "clean orphaned projections",
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
                Bulk orphan cleanup · {orphanProjections.length} projection
                {orphanProjections.length === 1 ? "" : "s"}
              </div>
              <div className="mono" style={{ color: "var(--ink-2)", fontSize: 11, marginTop: 3 }}>
                {orphanProjections.map((projection) => projection.instance_id).join(", ")}
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
                Also delete all live paths
              </label>
              <button
                className="btn ghost danger"
                onClick={cleanOrphansBulk}
                disabled={readOnly || cleanOrphans.busy}
                title={
                  readOnly
                    ? "registry offline"
                    : deleteLivePaths
                      ? "delete live paths for all orphaned projections"
                      : "clean orphaned projection metadata"
                }
              >
                {cleanOrphans.busy
                  ? "Cleaning..."
                  : deleteLivePaths
                    ? "Delete live paths and clean metadata"
                    : "Clean orphan metadata"}
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
        {projections.length === 0 ? (
          <EmptyState title="No projections yet" icon={<RefreshIcon />} command="loom sync replay">
            Create a binding that maps skills to targets, then replay or sync queued work to materialize projections.
          </EmptyState>
        ) : filtered.length === 0 ? (
          <EmptyState
            title="No projections in this filter"
            icon={<RefreshIcon />}
            actions={[{ label: "Show all", onClick: () => setFilter("all"), variant: "ghost" }]}
          >
            Existing projections are present, but none are currently marked <span className="mono">{filter}</span>.
          </EmptyState>
        ) : (
          <div className="two-col projections-layout">
            <div className="projections-list">
              <table className="tbl mobile-cards">
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
                      <td className="mono dim" data-label="Instance">
                        {projection.instance_id}
                      </td>
                      <td className="name" data-label="Skill">
                        {projection.skill_id}
                      </td>
                      <td data-label="Target">{targetLabel(targets, projection.target_id)}</td>
                      <td data-label="Method">{projection.method}</td>
                      <td data-label="Health">
                        <span className={`badge ${healthClass(projection)}`}>
                          {projection.observed_drift ? "drifted" : projection.health}
                        </span>
                      </td>
                      <td className="mono dim" data-label="Rev">
                        {shortRev(projection.last_applied_rev)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="projections-detail-panel">
              {selected && (
                <ProjectionDetail
                  projection={selected}
                  binding={selectedBinding}
                  targetLabel={targetLabel(targets, selected.target_id)}
                  readOnly={readOnly}
                  actionBusy={action.busy}
                  canCapture={canCapture}
                  canProject={canProject}
                  onCapture={capture}
                  onProject={project}
                  message={action.error ?? action.success}
                  messageTone={action.error ? "var(--err)" : "var(--ok)"}
                />
              )}
            </div>
          </div>
        )}
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
  onCapture,
  onProject,
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
  onCapture: () => void;
  onProject: () => void;
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
            Commit
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
