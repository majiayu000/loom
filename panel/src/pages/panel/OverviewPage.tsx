import type { Op, ProjectionLink, Skill, Target, VizMode } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { ProjectionGraph } from "../../components/panel/ProjectionGraph";
import { PlusIcon, RefreshIcon, ShieldIcon, TargetIcon } from "../../components/icons/nav_icons";
import { COUNT_TERMS, formatReplayableWrites, summarizeOps } from "../../lib/count_labels";
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
  onNewTarget: () => void;
  onNewBinding: () => void;
  onOpenSkills: () => void;
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
  onMutation,
  onNewTarget,
  onNewBinding,
  onOpenSkills,
  onViewActivity,
  onOpenSync,
  readOnly,
}: OverviewPageProps) {
  const selSkill = skills.find((s) => s.id === selectedSkill);
  const selTarget = targets.find((t) => t.id === selectedTarget);
  const importObserved = useMutation();
  const opCounts = summarizeOps(ops);
  const totalProjections = skills.reduce((a, s) => a + s.targets.length, 0);
  const totalRules = skills.reduce((a, s) => a + s.ruleCount, 0);
  const observedSkillCount = skills.filter((s) => s.observedImported || s.sources?.includes("observed")).length;
  const observedTargetCount = targets.filter((t) => t.ownership === "observed").length;
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
  const canImportObserved = skills.length === 0 && observedTargetCount > 0;
  const runImportObserved = () => {
    importObserved.run("import observed skills", () => api.skillImportObserved(), onMutation);
  };
  const skillStepDetail =
    canImportObserved
      ? `No managed registry skills yet. Import creates managed skills from ${observedTargetCount} observed target${observedTargetCount === 1 ? "" : "s"}.`
      : skills.length === 0
      ? "No registry skills imported yet."
      : observedSkillCount > 0
      ? `${skills.length} skill${skills.length === 1 ? "" : "s"} in registry · ${observedSkillCount} imported from observed targets.`
      : `${skills.length} tracked skill${skills.length === 1 ? "" : "s"}.`;
  const skillKpiMeta =
    totalRules > 0
      ? `${totalRules} binding rule${totalRules === 1 ? "" : "s"}`
      : observedSkillCount > 0
      ? "imported · no bindings"
      : skills.length > 0
      ? "tracked · no bindings"
      : "no bindings yet";
  const nextSteps: NextStep[] = [
    {
      label: "Add a skill",
      detail: skillStepDetail,
      done: skills.length > 0,
      action: canImportObserved ? "Import observed skills" : "Open Skills",
      onAction: canImportObserved ? runImportObserved : onOpenSkills,
      disabled: readOnly || importObserved.busy,
      title: readOnly ? "registry offline" : undefined,
    },
    {
      label: "Add a target",
      detail: targets.length === 0 ? "No agent directory connected." : `${targets.length} target${targets.length === 1 ? "" : "s"} connected.`,
      done: targets.length > 0,
      action: "Add target",
      onAction: onNewTarget,
      disabled: readOnly,
    },
    {
      label: "Add a binding",
      detail: totalRules === 0 ? "No routing rule maps a skill to a target." : `${totalRules} binding rule${totalRules === 1 ? "" : "s"}.`,
      done: totalRules > 0,
      action: "Add binding",
      onAction: onNewBinding,
      disabled: readOnly || targets.length === 0,
      title: targets.length === 0 ? "add a target first" : undefined,
    },
    {
      label: "Apply projections",
      detail: totalProjections === 0 ? "No live projection has been applied." : `${totalProjections} live projection${totalProjections === 1 ? "" : "s"}.`,
      done: totalProjections > 0,
      action: "Replay / sync",
      onAction: onOpenSync,
      disabled: readOnly || totalRules === 0,
      title: totalRules === 0 ? "add a binding first" : undefined,
    },
    {
      label: "Clear activity",
      detail:
        opCounts.actionNeeded === 0
          ? "No replayable or failed registry work."
          : `${formatReplayableWrites(opCounts.pending)} · ${opCounts.err} failed`,
      done: opCounts.actionNeeded === 0,
      action: opCounts.err > 0 ? "View activity" : "Replay queued writes",
      onAction: opCounts.err > 0 ? onViewActivity : onOpenSync,
      disabled: readOnly,
    },
  ];
  const graphEmptyAction = readOnly
    ? { label: "Registry offline", onClick: onOpenSync, disabled: true, title: "registry offline" }
    : skills.length === 0
      ? { label: "Open Skills", onClick: onOpenSkills }
      : targets.length === 0
        ? { label: "Add target", onClick: onNewTarget }
        : totalRules === 0
          ? { label: "Add binding", onClick: onNewBinding }
          : { label: "Replay / sync", onClick: onOpenSync };

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
            <h3>Next steps</h3>
            {readOnly && <span className="badge warn">read-only</span>}
          </div>
          <div className="card-body" style={{ display: "grid", gap: 8 }}>
            {nextSteps.map((step, index) => (
              <NextStepRow key={step.label} step={step} active={!step.done && nextSteps.findIndex((candidate) => !candidate.done) === index} />
            ))}
            {(importObserved.error || importObserved.success) && (
              <div className="mutation-note" data-tone={importObserved.error ? "err" : "ok"}>
                {importObserved.error ?? `✓ ${importObserved.success}`}
              </div>
            )}
          </div>
        </div>

        <div className="kpi-row">
          <Kpi label="Skills" value={skills.length} meta={skillKpiMeta} />
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
            label={COUNT_TERMS.actionNeeded}
            value={opCounts.actionNeeded}
            meta={
              opCounts.actionNeeded === 0 ? (
                "all clean"
              ) : (
                <>
                  {opCounts.pending > 0 && <span style={{ color: "var(--pending)" }}>{formatReplayableWrites(opCounts.pending)}</span>}
                  {opCounts.pending > 0 && opCounts.err > 0 && " · "}
                  {opCounts.err > 0 && <span style={{ color: "var(--err)" }}>{opCounts.err} failed</span>}
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
                  `${totalProjections} live projections · lines connect skills to targets`
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
              emptyAction={graphEmptyAction}
            />
            <div className="proj-legend proj-legend-grouped">
              <span className="legend-group-title">Projection method</span>
              <span>
                <span className="dot" style={{ background: "var(--accent-2)" }} />
                symlink
              </span>
              <span>
                <span className="dot" style={{ background: "var(--warn)" }} />
                copy
              </span>
              <span>
                <span className="dot" style={{ background: "var(--accent-3)" }} />
                materialize
              </span>
              <span className="divider">│</span>
              <span className="legend-group-title">Target ownership</span>
              <span>
                <span className="dot" style={{ background: "var(--managed)" }} />
                managed
              </span>
              <span>
                <span className="dot" style={{ background: "var(--observed)" }} />
                observed
              </span>
              <span>
                <span className="dot" style={{ background: "var(--external)" }} />
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
                    : "Registry root is separate from Loom. Writes enabled."}
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
                ) : observedTargetCount > 0 ? (
                  <>
                    Observed targets are read-only imports. External edits are saved only while{" "}
                    <span className="mono" style={{ color: "var(--ink-1)" }}>loom skill monitor-observed</span>{" "}
                    is running; registry source edits need{" "}
                    <span className="mono" style={{ color: "var(--ink-1)" }}>loom skill watch</span>.
                  </>
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

interface NextStep {
  label: string;
  detail: string;
  done: boolean;
  action: string;
  onAction: () => void;
  disabled?: boolean;
  title?: string;
}

function NextStepRow({ step, active }: { step: NextStep; active: boolean }) {
  const status = step.done ? "done" : active ? "next" : "waiting";

  return (
    <div className="next-step-row">
      <span className={`next-step-state ${status}`}>{status}</span>
      <div className="next-step-copy">
        <div className="section-title" style={{ margin: 0 }}>
          {step.label}
        </div>
        <div className="next-step-detail">{step.detail}</div>
      </div>
      {!step.done && (
        <button
          className={`btn sm next-step-action ${active ? "is-primary" : ""}`}
          onClick={step.onAction}
          disabled={step.disabled}
          title={step.title}
        >
          {step.action}
        </button>
      )}
    </div>
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
