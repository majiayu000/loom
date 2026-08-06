import { useEffect, useMemo, useState } from "react";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { Binding, PanelPageKey, Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { isMultiBinding } from "../../lib/binding_labels";
import { BindingsPage } from "./BindingsPage";
import { ProjectionsPage } from "./ProjectionsPage";
import { TargetsPage } from "./TargetsPage";

export type ControlPlaneTab = "graph" | "targets" | "bindings" | "projections";

interface ControlPlanePageProps {
  initialTab: Exclude<ControlPlaneTab, "graph">;
  skills: Skill[];
  targets: Target[];
  bindings: Binding[];
  projections: RegistryProjection[];
  selectedTarget: string | null;
  onSelectTarget: (id: string) => void;
  onRemoveTarget: (id: string) => void;
  onMutation: () => void;
  onNavigate: (page: PanelPageKey) => void;
  readOnly: boolean;
  mutationVersion: number;
}

const TABS: Array<{ id: ControlPlaneTab; label: string; page?: PanelPageKey }> = [
  { id: "graph", label: "Graph" },
  { id: "targets", label: "Targets", page: "targets" },
  { id: "bindings", label: "Bindings", page: "bindings" },
  { id: "projections", label: "Projections", page: "projections" },
];

export function ControlPlanePage({
  initialTab,
  skills,
  targets,
  bindings,
  projections,
  selectedTarget,
  onSelectTarget,
  onRemoveTarget,
  onMutation,
  onNavigate,
  readOnly,
  mutationVersion,
}: ControlPlanePageProps) {
  const [tab, setTab] = useState<ControlPlaneTab>(initialTab);

  useEffect(() => setTab(initialTab), [initialTab]);

  const selectTab = (next: ControlPlaneTab) => {
    setTab(next);
    const page = TABS.find((item) => item.id === next)?.page;
    if (page) onNavigate(page);
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Control Plane</h1>
          <div className="subtitle">Target ownership, binding routes, and realized projections.</div>
        </div>
        <div className="header-actions" role="tablist" aria-label="Control plane tabs">
          {TABS.map((item) => (
            <button type="button"
              key={item.id}
              role="tab"
              aria-selected={tab === item.id}
              className={`btn ${tab === item.id ? "primary" : "ghost"}`}
              onClick={() => selectTab(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>
      {tab === "graph" && (
        <ControlPlaneGraph targets={targets} bindings={bindings} projections={projections} />
      )}
      {tab === "targets" && (
        <TargetsPage
          targets={targets}
          skills={skills}
          selectedTarget={selectedTarget}
          onSelectTarget={onSelectTarget}
          onRemoveTarget={onRemoveTarget}
          onMutation={onMutation}
          readOnly={readOnly}
          mutationVersion={mutationVersion}
        />
      )}
      {tab === "bindings" && (
        <BindingsPage
          bindings={bindings}
          targets={targets}
          projections={projections}
          onMutation={onMutation}
          readOnly={readOnly}
          mutationVersion={mutationVersion}
        />
      )}
      {tab === "projections" && (
        <ProjectionsPage
          projections={projections}
          targets={targets}
          bindings={bindings}
          readOnly={readOnly}
          onMutation={onMutation}
        />
      )}
    </>
  );
}

function ControlPlaneGraph({
  targets,
  bindings,
  projections,
}: {
  targets: Target[];
  bindings: Binding[];
  projections: RegistryProjection[];
}) {
  const health = useMemo(() => countBy(projections, (projection) => projection.observed_drift ? "drifted" : projection.health || "unknown"), [projections]);
  const methods = useMemo(() => countBy(projections, (projection) => projection.method || "unknown"), [projections]);

  return (
    <div className="page-body">
      <div className="kpi-row" style={{ marginBottom: 16 }}>
        <Kpi label="Targets" value={targets.length} meta={ownershipSummary(targets)} />
        <Kpi label="Bindings" value={bindings.length} meta={`${bindings.filter(isMultiBinding).length} multi`} />
        <Kpi label="Projection methods" value={projections.length} meta={summary(methods)} />
        <Kpi label="Projection health" value={projections.length} meta={summary(health)} />
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(240px, 0.9fr) minmax(320px, 1.2fr)", gap: 16 }}>
        <section className="card">
          <div className="card-head">
            <h3>Targets</h3>
            <span className="badge">{targets.length}</span>
          </div>
          <div className="card-body" style={{ display: "grid", gap: 10 }}>
            {targets.length === 0 && <div className="empty">No targets registered.</div>}
            {targets.map((target) => (
              <TargetCard key={target.id} target={target} bindings={bindings} projections={projections} />
            ))}
          </div>
        </section>

        <section className="card">
          <div className="card-head">
            <h3>Routes</h3>
            <span className="badge">{bindings.length} bindings</span>
          </div>
          <div className="card-body" style={{ padding: 0 }}>
            {bindings.length === 0 ? (
              <div className="empty" style={{ padding: 18 }}>No binding routes configured.</div>
            ) : (
              <table className="tbl mobile-cards">
                <thead>
                  <tr>
                    <th>Binding</th>
                    <th>Skill</th>
                    <th>Target</th>
                    <th>Method</th>
                    <th>Health</th>
                  </tr>
                </thead>
                <tbody>
                  {bindings.map((binding) => (
                    <BindingGraphRow
                      key={binding.id}
                      binding={binding}
                      target={targets.find((target) => target.id === binding.target)}
                      projections={projections.filter((projection) => projection.binding_id === binding.id)}
                    />
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}

function TargetCard({ target, bindings, projections }: { target: Target; bindings: Binding[]; projections: RegistryProjection[] }) {
  const inboundBindings = bindings.filter((binding) => binding.target === target.id).length;
  const targetProjections = projections.filter((projection) => projection.target_id === target.id);
  return (
    <div style={{ border: "1px solid var(--line)", borderRadius: 6, padding: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <AgentAvatar agent={target.agent} size={28} radius={7} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ color: "var(--ink-0)", fontSize: 13 }}>{target.agent}/{target.profile}</div>
          <div className="mono" style={{ color: "var(--ink-2)", fontSize: 11, overflowWrap: "anywhere" }}>{target.path}</div>
        </div>
        <span className={`chip ${target.ownership}`}>{target.ownership}</span>
      </div>
      <div className="mono dim" style={{ marginTop: 9, fontSize: 11 }}>
        {capabilities(target)} · {inboundBindings} binding{inboundBindings === 1 ? "" : "s"} · {targetProjections.length} projection{targetProjections.length === 1 ? "" : "s"}
      </div>
    </div>
  );
}

function BindingGraphRow({ binding, target, projections }: { binding: Binding; target?: Target; projections: RegistryProjection[] }) {
  const health = summary(countBy(projections, (projection) => projection.observed_drift ? "drifted" : projection.health || "unknown"));
  const method = isMultiBinding(binding) ? "multi" : binding.method;
  return (
    <tr>
      <td className="mono dim" data-label="Binding">{binding.id}</td>
      <td data-label="Skill">{isMultiBinding(binding) ? <span className="badge warn">multi</span> : binding.skill}</td>
      <td data-label="Target">{target ? `${target.agent}/${target.profile}` : binding.target}</td>
      <td data-label="Method"><span className={`chip method ${method}`}>{method}</span></td>
      <td className="mono dim" data-label="Health">{health || "no projections"}</td>
    </tr>
  );
}

function Kpi({ label, value, meta }: { label: string; value: number; meta: string }) {
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
      <div className="meta">{meta}</div>
    </div>
  );
}

function capabilities(target: Target): string {
  if (target.ownership === "managed") return "writes projections";
  if (target.ownership === "observed") return "reads observed inventory";
  if (target.ownership === "external") return "context only";
  return "capabilities unknown";
}

function ownershipSummary(targets: Target[]): string {
  return summary(countBy(targets, (target) => target.ownership || "unknown")) || "none";
}

function summary(counts: Record<string, number>): string {
  return Object.entries(counts).map(([key, value]) => `${key} ${value}`).join(" · ");
}

function countBy<T>(items: T[], keyOf: (item: T) => string): Record<string, number> {
  return items.reduce<Record<string, number>>((acc, item) => {
    const key = keyOf(item);
    acc[key] = (acc[key] ?? 0) + 1;
    return acc;
  }, {});
}
