import type { Op, ProjectionLink, Skill, Target, VizMode } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { ProjectionGraph } from "../../components/panel/ProjectionGraph";
import { PlusIcon, RefreshIcon, ShieldIcon, TargetIcon } from "../../components/icons/nav_icons";

interface OverviewPageProps {
  skills: Skill[];
  targets: Target[];
  ops: Op[];
  projections: ProjectionLink[];
  vizMode: VizMode;
  setVizMode: (m: VizMode) => void;
  selectedSkill: string | null;
  selectedTarget: string | null;
  onSelectSkill: (id: string) => void;
  onSelectTarget: (id: string) => void;
  registryRoot: string | null;
  onMutation: () => void;
  onNewTarget: () => void;
  onNewBinding: () => void;
  onViewActivity: () => void;
  onOpenSync: () => void;
  readOnly: boolean;
}

export function OverviewPage({
  skills,
  targets,
  ops,
  projections,
  vizMode,
  setVizMode,
  selectedSkill,
  selectedTarget,
  onSelectSkill,
  onSelectTarget,
  registryRoot,
  onNewTarget,
  onNewBinding,
  onViewActivity,
  onOpenSync,
  readOnly,
}: OverviewPageProps) {
  const selSkill = skills.find((s) => s.id === selectedSkill);
  const selTarget = targets.find((t) => t.id === selectedTarget);
  const pendingOps = ops.filter((o) => o.status === "pending").length;
  const errOps = ops.filter((o) => o.status === "err").length;
  const totalProjections = skills.reduce((a, s) => a + s.targets.length, 0);
  const totalRules = skills.reduce((a, s) => a + s.ruleCount, 0);
  const uniqueAgents = new Set(targets.map((t) => t.agent)).size;
  const uniqueProfiles = new Set(targets.map((t) => `${t.agent}/${t.profile}`)).size;
  const methodCounts = projections.reduce<Record<string, number>>((acc, p) => {
    acc[p.method] = (acc[p.method] ?? 0) + 1;
    return acc;
  }, {});
  const rootDisplay = registryRoot ? registryRoot.replace(/^\/Users\/[^/]+/, "~") : "not connected";
  const writeGuardTone = readOnly ? "warn" : "ok";
  const canAddBinding = !readOnly && targets.length > 0;
  const addBindingTitle = readOnly ? "registry offline" : !canAddBinding ? "add a target first" : undefined;

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Overview</h1>
          <div className="subtitle">
            Build the registry in three steps: add a target, add a binding, then replay or sync changes to agent directories.
          </div>
        </div>
        <div className="header-actions">
          <button className="btn primary" onClick={onNewTarget} disabled={readOnly} title={readOnly ? "registry offline" : undefined}>
            <TargetIcon /> Add target
          </button>
          <button className="btn ghost" onClick={onNewBinding} disabled={!canAddBinding} title={addBindingTitle}>
            <PlusIcon /> Add binding
          </button>
          <button className="btn ghost" onClick={onOpenSync}>
            <RefreshIcon /> Replay / sync
          </button>
        </div>
      </div>
      <div className="page-body">
        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-head">
            <h3>Recommended flow</h3>
            {readOnly && <span className="badge warn">read-only</span>}
          </div>
          <div className="card-body" style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, fontSize: 12 }}>
            <div>
              <div className="section-title" style={{ marginTop: 0 }}>1. Add target</div>
              <div style={{ color: "var(--ink-2)" }}>Connect an agent directory Loom can manage, observe, or keep external.</div>
            </div>
            <div>
              <div className="section-title" style={{ marginTop: 0 }}>2. Add binding</div>
              <div style={{ color: "var(--ink-2)" }}>Map a skill to the target with an explicit matcher and projection policy.</div>
            </div>
            <div>
              <div className="section-title" style={{ marginTop: 0 }}>3. Replay / sync</div>
              <div style={{ color: "var(--ink-2)" }}>Apply pending writes locally, then pull or push registry changes when the remote is in use.</div>
            </div>
          </div>
        </div>

        <div className="kpi-row">
          <Kpi
            label="Skills"
            value={skills.length}
            meta={totalRules > 0 ? `${totalRules} binding rule${totalRules === 1 ? "" : "s"}` : "no bindings yet"}
          />
          <Kpi
            label="Targets"
            value={targets.length}
            meta={
              targets.length === 0
                ? "no targets"
                : `${uniqueAgents} agent${uniqueAgents === 1 ? "" : "s"} · ${uniqueProfiles} profile${uniqueProfiles === 1 ? "" : "s"}`
            }
          />
          <Kpi
            label="Projections"
            value={totalProjections}
            meta={
              totalProjections === 0
                ? "no projections"
                : `${methodCounts.symlink ?? 0} symlink · ${methodCounts.copy ?? 0} copy · ${methodCounts.materialize ?? 0} materialize`
            }
          />
          <Kpi
            label="Needs action"
            value={pendingOps + errOps}
            meta={
              pendingOps === 0 && errOps === 0 ? (
                "all clean"
              ) : (
                <>
                  {pendingOps > 0 && <span style={{ color: "var(--pending)" }}>{pendingOps} pending</span>}
                  {pendingOps > 0 && errOps > 0 && " · "}
                  {errOps > 0 && <span style={{ color: "var(--err)" }}>{errOps} failed</span>}
                </>
              )
            }
          />
        </div>

        <div className="proj-wrap">
          <div className="proj-head">
            <div>
              <h3>Skill → Target projections</h3>
              <div className="head-meta">
                {selSkill ? (
                  <>
                    Tracing <b style={{ color: "var(--ink-0)" }}>{selSkill.name}</b> → {selSkill.targets.length} targets
                  </>
                ) : selTarget ? (
                  <>
                    Inbound projections for <b style={{ color: "var(--ink-0)" }}>{selTarget.agent}/{selTarget.profile}</b>
                  </>
                ) : (
                  `${totalProjections} live projections · vertical lines are skills, horizontal lines are targets, and each knot is one projection`
                )}
              </div>
            </div>
            <div className="viz-switch">
              {(["loom", "force", "tree"] as const).map((m) => (
                <button
                  key={m}
                  className={vizMode === m ? "active" : ""}
                  onClick={() => setVizMode(m)}
                  title={m === "loom" ? "woven view" : m === "force" ? "relationship map" : "hierarchy view"}
                >
                  {m}
                </button>
              ))}
            </div>
          </div>
          <div className="proj-canvas">
            <ProjectionGraph
              mode={vizMode}
              selectedSkill={selectedSkill}
              selectedTarget={selectedTarget}
              onSelectSkill={onSelectSkill}
              onSelectTarget={onSelectTarget}
              skills={skills}
              targets={targets}
              projections={projections}
            />
            <div className="proj-legend proj-legend-grouped">
              <span className="legend-group-title">Projection method</span>
              <span>
                <span className="dot" style={{ background: "#6fb78a" }} />
                symlink
              </span>
              <span>
                <span className="dot" style={{ background: "#e6b450" }} />
                copy
              </span>
              <span>
                <span className="dot" style={{ background: "#c79ee0" }} />
                materialize
              </span>
              <span className="divider">│</span>
              <span className="legend-group-title">Target ownership</span>
              <span>
                <span className="dot" style={{ background: "#d97736" }} />
                managed
              </span>
              <span>
                <span className="dot" style={{ background: "#4ea9a0" }} />
                observed
              </span>
              <span>
                <span className="dot" style={{ background: "#8a8271" }} />
                external
              </span>
            </div>
          </div>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, marginTop: 16 }}>
          <div className="card">
            <div className="card-head">
              <h3>Recent Activity</h3>
              <button className="btn sm" onClick={onViewActivity} title="Open the full activity queue">
                View all →
              </button>
            </div>
            <div style={{ padding: 8 }}>
              {ops.length === 0 ? (
                <div className="empty" style={{ padding: "28px 12px" }}>
                  No activity yet. New writes, syncs, and projection checks will appear here.
                </div>
              ) : (
                ops.slice(0, 5).map((o) => <OpRow key={o.id} op={o} />)
              )}
            </div>
          </div>
          <div className="card">
            <div className="card-head">
              <h3>Write Guard</h3>
              <span className={`badge ${writeGuardTone}`}>{readOnly ? "offline" : "active"}</span>
            </div>
            <div className="card-body" style={{ fontSize: 12, color: "var(--ink-1)" }}>
              <div className="row-flex" style={{ marginBottom: 10 }}>
                <ShieldIcon style={{ color: readOnly ? "var(--warn)" : "var(--ok)" }} />
                <span>
                  {readOnly
                    ? "Registry API is offline. Writes are disabled until the panel reconnects."
                    : "Registry root is independent of the Loom repo. Writes are enabled for connected targets."}
                </span>
              </div>
              <pre className="code" style={{ marginBottom: 10 }}>
                <span className="c"># Current registry</span>
                {"\n"}
                <span className="k">--root</span> <span className="s">{rootDisplay}</span>
              </pre>
              <div style={{ color: "var(--ink-3)", fontSize: 11 }}>
                {readOnly ? (
                  "Start the panel backend to load git HEAD and sync state."
                ) : (
                  <>
                    Use <span className="mono" style={{ color: "var(--ink-1)" }}>Git sync</span> to pull, push, or replay registry operations.
                  </>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

interface KpiProps {
  label: string;
  value: number;
  meta: React.ReactNode;
}

function Kpi({ label, value, meta }: KpiProps) {
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
      <div className="meta">{meta}</div>
    </div>
  );
}
