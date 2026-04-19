import type { Op, ProjectionLink, Skill, Target, VizMode } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { ProjectionGraph } from "../../components/panel/ProjectionGraph";
import { PlusIcon, RefreshIcon, ShieldIcon, SyncIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

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
  onNewBinding: () => void;
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
  onMutation,
  onNewBinding,
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
  const rootDisplay = registryRoot
    ? registryRoot.replace(/^\/Users\/[^/]+/, "~")
    : "~/.loom-registry";
  const sync = useMutation();

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Overview</h1>
          <div className="subtitle">
            Your skill registry projected across {targets.length} agent targets. Click any thread to trace its bindings.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn ghost"
            disabled={readOnly || sync.busy}
            onClick={() => sync.run("sync pull", api.syncPull, onMutation)}
            title={readOnly ? "registry offline" : undefined}
          >
            <SyncIcon /> Sync pull
          </button>
          <button
            className="btn ghost"
            disabled={readOnly || sync.busy}
            onClick={() => sync.run("sync push", api.syncPush, onMutation)}
            title={readOnly ? "registry offline" : undefined}
          >
            <SyncIcon /> Sync push
          </button>
          <button
            className="btn ghost"
            disabled={readOnly || sync.busy}
            onClick={() => sync.run("sync replay", api.syncReplay, onMutation)}
            title={readOnly ? "registry offline" : undefined}
          >
            <RefreshIcon /> Replay pending
          </button>
          <button className="btn primary" onClick={onNewBinding} disabled={readOnly}>
            <PlusIcon /> New binding
          </button>
        </div>
      </div>
      {(sync.error || sync.success || sync.busy) && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: sync.error ? "var(--err)" : sync.busy ? "var(--ink-2)" : "var(--ok)",
            background: sync.error
              ? "rgba(216,90,90,0.08)"
              : sync.busy
              ? "var(--bg-2)"
              : "rgba(111,183,138,0.08)",
          }}
        >
          {sync.busy ? "…" : sync.error ?? `✓ ${sync.success}`}
        </div>
      )}
      <div className="page-body">
        <div className="kpi-row">
          <Kpi
            label="Skills"
            value={skills.length}
            meta={totalRules > 0 ? `${totalRules} rule${totalRules === 1 ? "" : "s"} on chain` : "no rules yet"}
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
            label="Ops"
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
              <h3>Registry → Targets</h3>
              <div className="head-meta">
                {selSkill ? (
                  <>
                    Tracing <b style={{ color: "var(--ink-0)" }}>{selSkill.name}</b> → {selSkill.targets.length} targets
                  </>
                ) : selTarget ? (
                  <>
                    Inbound projections for{" "}
                    <b style={{ color: "var(--ink-0)" }}>
                      {selTarget.agent}/{selTarget.profile}
                    </b>
                  </>
                ) : (
                  `${totalProjections} active projections · click a warp thread (skill) or weft thread (target) to isolate`
                )}
              </div>
            </div>
            <div className="viz-switch">
              {(["loom", "force", "tree"] as const).map((m) => (
                <button key={m} className={vizMode === m ? "active" : ""} onClick={() => setVizMode(m)}>
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
            <div className="proj-legend">
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
              <span>
                <span className="dot" style={{ background: "#d97736" }} />
                managed
              </span>
              <span>
                <span className="dot" style={{ background: "#6fb78a" }} />
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
              <h3>Recent Ops</h3>
              <button className="btn sm">View all →</button>
            </div>
            <div style={{ padding: 8 }}>
              {ops.slice(0, 5).map((o) => (
                <OpRow key={o.id} op={o} />
              ))}
            </div>
          </div>
          <div className="card">
            <div className="card-head">
              <h3>Write Guard</h3>
              <span className="badge ok">active</span>
            </div>
            <div className="card-body" style={{ fontSize: 12, color: "var(--ink-1)" }}>
              <div className="row-flex" style={{ marginBottom: 10 }}>
                <ShieldIcon style={{ color: "var(--ok)" }} />
                <span>Registry root is independent of Loom tool repo. Mutable ops enabled.</span>
              </div>
              <pre className="code" style={{ marginBottom: 10 }}>
                <span className="c"># Current registry</span>
                {"\n"}
                <span className="k">--root</span> <span className="s">{rootDisplay}</span>
              </pre>
              <div style={{ color: "var(--ink-3)", fontSize: 11 }}>
                Run <span className="mono" style={{ color: "var(--ink-1)" }}>loom status</span> for git HEAD · sync state
                available via the Topbar.
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
