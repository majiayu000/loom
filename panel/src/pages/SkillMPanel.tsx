import { useEffect, useMemo, useState } from "react";
import type { CSSProperties } from "react";
import { usePanelData } from "../lib/api/usePanelData";
import { api } from "../lib/api/client";
import type { Op, PanelPageKey, Skill, Target } from "../lib/types";

type SkillMPage = PanelPageKey | "market" | "forge";
type ToastKind = "ok" | "err" | "info" | "sync";

interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

const iconPath: Record<string, string> = {
  dash: "M3 3h7v9H3zM14 3h7v5h-7zM14 12h7v9h-7zM3 16h7v5H3z",
  lib: "M4 4h4v16H4zM10 4h4v16h-4zM16.5 4.5l4 1-3.5 15-4-1z",
  target: "M12 3a9 9 0 109 9 9 9 9 0 00-9-9zm0 4a5 5 0 105 5 5 5 0 00-5-5zm0 3a2 2 0 102 2 2 2 0 00-2-2z",
  branch: "M6 3v12M6 15a3 3 0 103 3M18 9a3 3 0 10-3-3M18 9a9 9 0 01-9 9",
  graph: "M5 5a2 2 0 104 0 2 2 0 10-4 0M15 12a2 2 0 104 0 2 2 0 10-4 0M7 19a2 2 0 104 0 2 2 0 10-4 0M7.5 6.5l8 4.5M9.5 17.5l6-4.5",
  ops: "M4 6h10M4 12h7M4 18h10M17 7l2.5 2.5L17 12M19 16l-2.5 2.5L14 16",
  clock: "M12 3a9 9 0 109 9 9 9 9 0 00-9-9zm0 4v5l3.5 2",
  sync: "M21 12a9 9 0 01-15.5 6.2M3 12a9 9 0 0115.5-6.2M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3",
  shield: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM9 12l2 2 4-4",
  gear: "M12 8a4 4 0 100 8 4 4 0 000-8zM19 12a7 7 0 00-.1-1.2l2.1-1.6-2-3.4-2.4 1a7 7 0 00-2.1-1.2L14 3h-4l-.5 2.6a7 7 0 00-2.1 1.2l-2.4-1-2 3.4 2.1 1.6A7 7 0 005 12a7 7 0 00.1 1.2L3 14.8l2 3.4 2.4-1a7 7 0 002.1 1.2L10 21h4l.5-2.6a7 7 0 002.1-1.2l2.4 1 2-3.4-2.1-1.6A7 7 0 0019 12z",
  search: "M10.5 3a7.5 7.5 0 105.3 12.8L21 21l-1.5 1.5-5.2-5.2A7.5 7.5 0 1010.5 3z",
  term: "M4 5h16v14H4zM7 9l3 3-3 3M12 15h5",
  plus: "M12 5v14M5 12h14",
  x: "M6 6l12 12M18 6L6 18",
  eye: "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7zm10 3a3 3 0 100-6 3 3 0 000 6z",
  bolt: "M13 2L4 14h6l-1 8 9-12h-6z",
  market: "M4 7l2-4h12l2 4M4 7h16v3a3 3 0 01-6 0 3 3 0 01-4 0 3 3 0 01-6 0V7zM5 13v8h14v-8",
  forge: "M12 2l2.4 7.2L22 12l-7.6 2.8L12 22l-2.4-7.2L2 12l7.6-2.8z",
};

const pages: Array<{ id: SkillMPage; icon: string; label: string; group: "build" | "ops" }> = [
  { id: "overview", icon: "dash", label: "Overview", group: "build" },
  { id: "skills", icon: "lib", label: "Skills", group: "build" },
  { id: "targets", icon: "target", label: "Targets", group: "build" },
  { id: "bindings", icon: "branch", label: "Bindings", group: "build" },
  { id: "projections", icon: "graph", label: "Projections", group: "build" },
  { id: "ops", icon: "ops", label: "Ops", group: "build" },
  { id: "history", icon: "clock", label: "Audit log", group: "ops" },
  { id: "sync", icon: "sync", label: "Git sync", group: "ops" },
  { id: "doctor", icon: "shield", label: "Doctor", group: "ops" },
  { id: "settings", icon: "gear", label: "Settings", group: "ops" },
  { id: "market", icon: "market", label: "Market", group: "ops" },
  { id: "forge", icon: "forge", label: "Forge", group: "ops" },
];

function Icon({ d, size = 18 }: { d: string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d={iconPath[d] ?? d} />
    </svg>
  );
}

function Glyph({ children }: { children: string }) {
  return <span className="sm-glyph">{children.slice(0, 2).toUpperCase()}</span>;
}

function shortName(value: string) {
  return value.replace(/^target_/, "").replace(/_/g, " ").slice(0, 34);
}

function sourceLabel(skill: Skill) {
  if (skill.observedImported) return "observed";
  if (skill.sourceStatus === "missing") return "missing";
  if (skill.sourceStatus === "non-compliant") return "non-compliant";
  return "registry";
}

function methodTone(method: string) {
  if (method === "materialize") return "var(--acc1)";
  if (method === "copy") return "var(--acc2)";
  if (method === "symlink") return "var(--acc3)";
  return "var(--faint)";
}

function classForOp(op: Op) {
  if (op.status === "ok") return "done";
  if (op.status === "err") return "failed";
  return "pending";
}

export function SkillMPanel() {
  const live = usePanelData();
  const [view, setView] = useState<SkillMPage>("overview");
  const [query, setQuery] = useState("");
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [termOpen, setTermOpen] = useState(false);
  const [tweaksOpen, setTweaksOpen] = useState(false);
  const [dark, setDark] = useState(true);
  const [density, setDensity] = useState<"compact" | "regular" | "comfy">("regular");
  const [accent, setAccent] = useState(["#ff0080", "#7928ca", "#00d9ff"]);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const counts = useMemo(() => {
    const failedOps = live.ops.filter((op) => op.status === "err").length;
    const drifted = live.projections.filter((p) => p.observed_drift || p.health === "drift").length;
    const pending = live.queuedWriteCount + live.ops.filter((op) => op.status === "pending").length;
    return { failedOps, drifted, pending, attention: failedOps + drifted + pending };
  }, [live.ops, live.projections, live.queuedWriteCount]);

  const filteredSkills = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return live.skills;
    return live.skills.filter((skill) => {
      return (
        skill.name.toLowerCase().includes(q) ||
        (skill.description ?? "").toLowerCase().includes(q) ||
        skill.tag.toLowerCase().includes(q)
      );
    });
  }, [live.skills, query]);

  const selected = live.skills.find((skill) => skill.name === selectedSkill) ?? filteredSkills[0] ?? null;

  const toast = (kind: ToastKind, text: string) => {
    const id = Date.now() + Math.random();
    setToasts((items) => [...items, { id, kind, text }]);
    window.setTimeout(() => setToasts((items) => items.filter((item) => item.id !== id)), 3200);
  };

  const runAction = async (label: string, fn: () => Promise<unknown>) => {
    try {
      await fn();
      toast("ok", `${label} completed`);
      live.refetch();
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen(true);
      } else if (event.ctrlKey && event.key === "`") {
        event.preventDefault();
        setTermOpen((open) => !open);
      } else if (event.key === "Escape") {
        setPaletteOpen(false);
        setTermOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div
      className={`sm-app ${dark ? "dark" : "light"} den-${density}`}
      style={{ "--acc1": accent[0], "--acc2": accent[1], "--acc3": accent[2], "--glow": "0.72" } as CSSProperties}
    >
      <div className="sm-particles skillm-grid-bg" aria-hidden="true" />
      <div className="sm-frame">
        <ActivityRail view={view} counts={{ ...counts, skills: live.skills.length, targets: live.targets.length, bindings: live.bindings.length, projections: live.projections.length }} onGo={(page) => setView(page)} onTerm={() => setTermOpen((open) => !open)} />
        <main className="sm-main" data-screen-label={view}>
          {view === "overview" && <Overview live={live} counts={counts} go={setView} />}
          {view === "skills" && <Skills skills={filteredSkills} query={query} setQuery={setQuery} selected={selected} setSelectedSkill={setSelectedSkill} />}
          {(view === "targets" || view === "bindings" || view === "projections") && <Plane live={live} tab={view} go={setView} />}
          {(view === "ops" || view === "history") && <Ops live={live} history={view === "history"} runAction={runAction} />}
          {view === "sync" && <Sync live={live} runAction={runAction} />}
          {view === "doctor" && <Doctor apiReachable={live.apiReachable} live={live.live} />}
          {view === "settings" && <Settings live={live} dark={dark} setDark={setDark} density={density} setDensity={setDensity} accent={accent} setAccent={setAccent} />}
          {view === "market" && <Unavailable title="Market" copy="The SkillM marketplace screen exists in the reference UI, but Loom V1 has no marketplace backend. This stays blank until a real catalog API exists." />}
          {view === "forge" && <Unavailable title="Forge" copy="The Forge wizard is intentionally not wired yet. Creating skills still requires a real source path or Git URL through the existing add flow." />}
          {termOpen && <Terminal live={live} close={() => setTermOpen(false)} />}
        </main>
      </div>
      <StatusBar live={live} counts={counts} dark={dark} setDark={setDark} onSync={() => runAction("Replay / sync", api.syncReplay)} onTerm={() => setTermOpen((open) => !open)} onTweaks={() => setTweaksOpen((open) => !open)} />
      {paletteOpen && <Palette skills={live.skills} go={(page) => { setView(page); setPaletteOpen(false); }} openSkill={(name) => { setSelectedSkill(name); setView("skills"); setPaletteOpen(false); }} close={() => setPaletteOpen(false)} />}
      {tweaksOpen && <Tweaks dark={dark} setDark={setDark} density={density} setDensity={setDensity} accent={accent} setAccent={setAccent} close={() => setTweaksOpen(false)} />}
      <Toasts items={toasts} dismiss={(id) => setToasts((items) => items.filter((item) => item.id !== id))} />
    </div>
  );
}

function ActivityRail({ view, counts, onGo, onTerm }: { view: SkillMPage; counts: Record<string, number>; onGo: (page: SkillMPage) => void; onTerm: () => void }) {
  return (
    <nav className="sm-actbar">
      <div className="logo" title="Loom"><span>L</span></div>
      <div className="nav-items">
        {(["build", "ops"] as const).map((group) => (
          <div key={group} className="nav-group">
            <span className="nav-grouplabel">{group === "build" ? "BUILD" : "OPS"}</span>
            {pages.filter((page) => page.group === group).map((page) => {
              const count = page.id === "skills" ? counts.skills : page.id === "targets" ? counts.targets : page.id === "bindings" ? counts.bindings : page.id === "projections" ? counts.projections : page.id === "ops" ? counts.attention : 0;
              return (
                <button key={page.id} className={`act ${view === page.id ? "on" : ""}`} title={page.label} onClick={() => onGo(page.id)}>
                  <Icon d={page.icon} size={20} />
                  <span className="act-label">{page.label}</span>
                  {count > 0 && <span className={`act-count ${page.id === "ops" ? "attn" : ""}`}>{count}</span>}
                </button>
              );
            })}
          </div>
        ))}
      </div>
      <div className="nav-bottom">
        <button className="act" title="Terminal" onClick={onTerm}><Icon d="term" size={20} /></button>
      </div>
    </nav>
  );
}

function Overview({ live, counts, go }: { live: ReturnType<typeof usePanelData>; counts: { failedOps: number; drifted: number; pending: number; attention: number }; go: (page: SkillMPage) => void }) {
  const ownership = tally(live.targets.map((target) => target.ownership));
  const methods = tally(live.projections.map((projection) => projection.method));
  const health = tally(live.projections.map((projection) => projection.health));
  const lastOp = live.ops[0];
  return (
    <div className="view view-dash">
      <header className="dash-hero">
        <div>
          <div className="hero-kicker">{live.live ? "registry online" : live.apiReachable ? "api degraded" : "api offline"}</div>
          <h1>SkillM control room for <em>Loom</em></h1>
          <p>{live.registryRoot ?? "Registry root unavailable"} · {live.mode}</p>
        </div>
        <div className="hero-orb"><span /><span /><span /><i className="orb-core" /></div>
      </header>
      <section className="stat-row">
        <Stat label="Skills" value={live.skills.length} sub={`${live.skills.filter((s) => s.observedImported).length} observed`} icon="lib" onClick={() => go("skills")} />
        <Stat label="Targets" value={live.targets.length} sub={`${ownership.managed ?? 0} managed · ${ownership.observed ?? 0} observed`} icon="target" onClick={() => go("targets")} />
        <Stat label="Bindings" value={live.bindings.length} sub="workspace routes" icon="branch" onClick={() => go("bindings")} />
        <Stat label="Attention" value={counts.attention} sub={`${counts.pending} queued · ${counts.drifted} drift`} icon="bolt" hot onClick={() => go("ops")} />
      </section>
      {live.error && <div className="dash-attn"><div className="da-text"><span className="da-fail"><b>API</b>{live.error}</span></div></div>}
      {live.warnings.length > 0 && <div className="dash-attn"><div className="da-text">{live.warnings.slice(0, 3).map((warning) => <span key={warning}><b>warning</b>{warning}</span>)}</div></div>}
      <div className="dash-grid">
        <section className="panel">
          <div className="panel-head"><h3><Icon d="graph" />Skill to target projections</h3><span className="panel-hint">{live.projections.length} live edges</span></div>
          <ProjectionGraph skills={live.skills} targets={live.targets} />
        </section>
        <section className="panel">
          <div className="panel-head"><h3><Icon d="lib" />Top skills</h3><button className="link-btn" onClick={() => go("skills")}>Open library</button></div>
          <div className="top-list">
            {live.skills.slice(0, 6).map((skill, index) => (
              <button className="top-item" key={skill.name} onClick={() => go("skills")}>
                <span className="top-rank">{String(index + 1).padStart(2, "0")}</span>
                <Glyph>{skill.name}</Glyph>
                <span className="top-name">{skill.name}</span>
                <span className="top-calls">{skill.projectionCount} edges</span>
              </button>
            ))}
            {live.skills.length === 0 && <div className="panel-empty">No skills from live registry yet.</div>}
          </div>
        </section>
        <section className="panel">
          <div className="panel-head"><h3><Icon d="ops" />Operations</h3><span className="panel-hint">{lastOp?.time ?? "none"}</span></div>
          <div className="upd-list">
            {live.ops.slice(0, 5).map((op) => <OpLine key={op.id} op={op} />)}
            {live.ops.length === 0 && <div className="panel-empty">No operation history loaded.</div>}
          </div>
        </section>
      </div>
      <div className="dash-attn">
        <div className="da-text">
          <span><b>methods</b>{Object.entries(methods).map(([k, v]) => `${k}:${v}`).join(" · ") || "none"}</span>
          <span><b>health</b>{Object.entries(health).map(([k, v]) => `${k}:${v}`).join(" · ") || "unavailable"}</span>
        </div>
        <div className="da-acts"><button className="da-link" onClick={() => go("projections")}>inspect graph</button><button className="da-link" onClick={() => go("doctor")}>run doctor</button></div>
      </div>
    </div>
  );
}

function Stat({ label, value, sub, icon, hot, onClick }: { label: string; value: number | string; sub: string; icon: string; hot?: boolean; onClick?: () => void }) {
  return (
    <button className={`stat-card link ${hot ? "hot" : ""}`} onClick={onClick}>
      <div className="stat-top"><Icon d={icon} />{label}</div>
      <div className="stat-val">{value}</div>
      <div className="stat-sub">{sub}</div>
    </button>
  );
}

function Skills({ skills, query, setQuery, selected, setSelectedSkill }: { skills: Skill[]; query: string; setQuery: (value: string) => void; selected: Skill | null; setSelectedSkill: (name: string) => void }) {
  return (
    <div className="view view-lib">
      <header className="view-head">
        <div><h1>Skill library</h1><p>Live registry inventory rendered in the SkillM library layout.</p></div>
        <div className="searchbox"><Icon d="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search skills, tags, descriptions" /></div>
      </header>
      <div className="lib-grid">
        {skills.map((skill) => (
          <article key={skill.name} className={`skill-card ${selected?.name === skill.name ? "sel" : ""}`} onClick={() => setSelectedSkill(skill.name)}>
            <div className="sc-head"><Glyph>{skill.name}</Glyph><div className="sc-title"><b>{skill.name}</b><span className="sc-meta">{sourceLabel(skill)} · {skill.changed}</span></div></div>
            <p className="sc-desc">{skill.description || "No description from backend."}</p>
            <div className="sc-tags"><span className="sm-tag">{skill.tag}</span><span className="sm-tag">{skill.latestRev}</span><span className="sm-tag">{skill.projectionCount} projections</span></div>
            <div className="sc-foot"><span>{skill.bindingCount} bindings</span><span>{skill.targets.length + (skill.observedTargetIds?.length ?? 0)} targets</span></div>
          </article>
        ))}
        {skills.length === 0 && <div className="lib-empty"><Icon d="lib" size={28} /><p>No skills available from the live API.</p></div>}
      </div>
      {selected && (
        <section className="skill-detail panel">
          <div className="det-head">
            <div className="det-title"><Glyph>{selected.name}</Glyph><div><h2>{selected.name}</h2><p>{selected.description || "No backend description."}</p></div></div>
            <span className="sec-badge good"><Icon d="shield" />{selected.sourceStatus}</span>
          </div>
          <div className="det-stats">
            <Stat label="Bindings" value={selected.bindingCount} sub="routing rules" icon="branch" />
            <Stat label="Projections" value={selected.projectionCount} sub="materialized edges" icon="graph" />
            <Stat label="Latest rev" value={selected.latestRev} sub="backend reported" icon="clock" />
            <Stat label="Targets" value={selected.targets.length + (selected.observedTargetIds?.length ?? 0)} sub="observed + projected" icon="target" />
          </div>
        </section>
      )}
    </div>
  );
}

function Plane({ live, tab, go }: { live: ReturnType<typeof usePanelData>; tab: "targets" | "bindings" | "projections"; go: (page: SkillMPage) => void }) {
  return (
    <div className="view view-plane">
      <header className="view-head"><div><h1>Control plane</h1><p>Targets, bindings, and projection graph share one SkillM plane.</p></div></header>
      <nav className="plane-tabs">
        {(["targets", "bindings", "projections"] as const).map((id) => <button key={id} className={`det-tab ${tab === id ? "on" : ""}`} onClick={() => go(id)}>{id}</button>)}
      </nav>
      {tab === "targets" && <DataGrid columns={["agent", "profile", "ownership", "skills", "path"]} rows={live.targets.map((t) => [t.agent, t.profile, t.ownership, `${t.observedSkills ?? 0} observed / ${t.projectedSkills ?? 0} projected`, t.path])} />}
      {tab === "bindings" && <DataGrid columns={["binding", "skill", "target", "matcher", "method"]} rows={live.bindings.map((b) => [shortName(b.id), b.skill, shortName(b.target), b.matcher, b.method])} />}
      {tab === "projections" && (
        <section className="panel">
          <div className="panel-head"><h3><Icon d="graph" />Projection graph</h3><span className="panel-hint">{live.projections.length} edges</span></div>
          <ProjectionGraph skills={live.skills} targets={live.targets} />
          <DataGrid columns={["skill", "target", "method", "health", "rev"]} rows={live.projections.map((p) => [p.skill_id, shortName(p.target_id), p.method, p.health, p.last_applied_rev?.slice(0, 8) || "—"])} />
        </section>
      )}
    </div>
  );
}

function DataGrid({ columns, rows }: { columns: string[]; rows: Array<Array<string | number>> }) {
  return (
    <div className="skillm-table panel">
      <table><thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead><tbody>{rows.map((row, index) => <tr key={index}>{row.map((cell, cellIndex) => <td key={cellIndex}>{cell}</td>)}</tr>)}</tbody></table>
      {rows.length === 0 && <div className="panel-empty">No live rows.</div>}
    </div>
  );
}

function ProjectionGraph({ skills, targets }: { skills: Skill[]; targets: Target[] }) {
  return (
    <div className="plane-graph skillm-mini-graph">
      <div className="pg-cols"><span>Skills</span><span>Targets</span></div>
      <div className="skillm-graph-grid">
        <div>{skills.slice(0, 8).map((skill) => <div className="pg-node-row" key={skill.name}><Glyph>{skill.name}</Glyph><span>{skill.name}</span></div>)}</div>
        <svg viewBox="0 0 500 260" className="proj-svg" aria-hidden="true">
          {skills.slice(0, 8).map((skill, index) => {
            const y = 24 + index * 29;
            const targetIndex = targets.findIndex((target) => skill.targets.includes(target.id) || skill.observedTargetIds?.includes(target.id));
            const ty = 24 + Math.max(targetIndex, 0) * 34;
            return <path key={skill.name} d={`M20 ${y} C180 ${y}, 300 ${ty}, 480 ${ty}`} className="proj-edge" stroke={methodTone("symlink")} />;
          })}
        </svg>
        <div>{targets.slice(0, 7).map((target) => <div className="pg-node-row target" key={target.id}><span className="tc-agent">{target.agent.slice(0, 2).toUpperCase()}</span><span>{target.path}</span></div>)}</div>
      </div>
    </div>
  );
}

function Ops({ live, history, runAction }: { live: ReturnType<typeof usePanelData>; history: boolean; runAction: (label: string, fn: () => Promise<unknown>) => void }) {
  const failed = live.ops.filter((op) => op.status === "err").length;
  const pending = live.ops.filter((op) => op.status === "pending").length + live.queuedWriteCount;
  return (
    <div className="view view-ops">
      <header className="view-head">
        <div><h1>{history ? "Audit history" : "Ops queue"}</h1><p>Real operation records from the Panel API.</p></div>
        <div className="ops-head-actions"><button className="btn-ghost sm" onClick={() => runAction("Purge ops", api.opsPurge)}><Icon d="x" />purge</button><button className="btn-grad sm" onClick={() => runAction("Replay queued ops", api.opsRetry)}><Icon d="sync" />retry / replay</button></div>
      </header>
      <div className="ops-stats"><div className="pstat acc"><span className="pstat-l">Pending</span><span className="pstat-n">{pending}</span></div><div className="pstat warn"><span className="pstat-l">Failed</span><span className="pstat-n">{failed}</span></div><div className="pstat"><span className="pstat-l">Events</span><span className="pstat-n">{live.ops.length}</span></div></div>
      <section className="op-table panel">{live.ops.map((op) => <OpLine key={op.id} op={op} />)}{live.ops.length === 0 && <div className="panel-empty">No operations returned by API.</div>}</section>
    </div>
  );
}

function OpLine({ op }: { op: Op }) {
  return <div className={`op-row op-row-${classForOp(op)}`}><span className={`op-pill op-${classForOp(op)}`}>{op.status}</span><span className="op-time">{op.time}</span><span className="op-verb">{op.kind}</span><span className="op-detail">{op.skill}<span className="op-arrow">{" -> "}<code>{op.target}</code></span></span><span className="op-note">{op.reason ?? op.method}</span></div>;
}

function Sync({ live, runAction }: { live: ReturnType<typeof usePanelData>; runAction: (label: string, fn: () => Promise<unknown>) => void }) {
  const remote = live.remote;
  return (
    <div className="view view-sync">
      <header className="view-head"><div><h1>Git sync</h1><p>Sync is live only when a registry remote is configured.</p></div><button className="btn-grad sm" onClick={() => runAction("Sync replay", api.syncReplay)}><Icon d="sync" />replay</button></header>
      <div className="sync-grid">
        <section className="panel"><div className="panel-head"><h3><Icon d="branch" />Remote</h3></div><p className="set-v">{remote?.url ?? "No remote configured"}</p><p>{remote?.sync_state ?? "local only"}</p></section>
        <section className="panel"><div className="panel-head"><h3><Icon d="ops" />Queue</h3></div><div className="stat-val">{live.queuedWriteCount}</div><p>queued writes</p></section>
      </div>
    </div>
  );
}

function Doctor({ apiReachable, live }: { apiReachable: boolean; live: boolean }) {
  return (
    <div className="view view-doctor">
      <header className="view-head"><div><h1>Doctor</h1><p>Degraded states stay visible; diagnostics are not hidden behind registry failures.</p></div></header>
      <div className="doc-summary"><div className={`doc-verdict ${apiReachable ? "ok" : "warn"}`}><div className="dv-ring" /><div className="dv-text"><b>{apiReachable ? "API reachable" : "API unreachable"}</b><span>{live ? "registry live" : "registry degraded or offline"}</span></div></div></div>
    </div>
  );
}

function Settings({ live, dark, setDark, density, setDensity, accent, setAccent }: { live: ReturnType<typeof usePanelData>; dark: boolean; setDark: (value: boolean) => void; density: "compact" | "regular" | "comfy"; setDensity: (value: "compact" | "regular" | "comfy") => void; accent: string[]; setAccent: (value: string[]) => void }) {
  const themes = [["#ff0080", "#7928ca", "#00d9ff"], ["#34d399", "#0ea5e9", "#a3e635"], ["#ff6b35", "#f43f5e", "#fbbf24"]];
  return (
    <div className="view view-settings">
      <header className="view-head"><div><h1>Settings</h1><p>Appearance controls from the SkillM prototype, backed by local UI state.</p></div></header>
      <section className="set-card panel">
        <div className="set-row"><div className="set-k"><h4>Registry root</h4><p>Live backend value</p></div><code className="set-v">{live.registryRoot ?? "unavailable"}</code></div>
        <div className="set-row"><div className="set-k"><h4>Theme</h4><p>Dark mode</p></div><Switch on={dark} onChange={setDark} /></div>
        <div className="set-row"><div className="set-k"><h4>Accent</h4><p>Neon / Aurora / Sunset</p></div><div className="twk-chips">{themes.map((theme) => <button key={theme.join("")} className="twk-chip" data-on={theme.join("") === accent.join("") ? "1" : "0"} onClick={() => setAccent(theme)}>{theme.map((color) => <i key={color} style={{ background: color }} />)}</button>)}</div></div>
        <div className="set-row"><div className="set-k"><h4>Density</h4><p>Layout spacing</p></div><div className="twk-radio">{(["compact", "regular", "comfy"] as const).map((value) => <button key={value} data-on={density === value ? "1" : "0"} onClick={() => setDensity(value)}>{value}</button>)}</div></div>
      </section>
    </div>
  );
}

function Switch({ on, onChange }: { on: boolean; onChange: (value: boolean) => void }) {
  return <button className={`sm-switch ${on ? "on" : ""}`} role="switch" aria-checked={on} onClick={() => onChange(!on)}><span className="knob" /></button>;
}

function Unavailable({ title, copy }: { title: string; copy: string }) {
  return <div className="view"><header className="view-head"><div><h1>{title}</h1><p>{copy}</p></div></header><section className="panel"><div className="panel-empty">No backend contract declared for this surface.</div></section></div>;
}

function Terminal({ live, close }: { live: ReturnType<typeof usePanelData>; close: () => void }) {
  return (
    <div className="sm-terminal">
      <div className="term-head"><span><Icon d="term" /> TERMINAL - skillm shell</span><button className="btn-icon" onClick={close}><Icon d="x" /></button></div>
      <div className="term-body"><p>SkillM Terminal - read-only preview</p><p><b>$</b> loom workspace status</p><p>{live.live ? "registry live" : live.error ?? "offline"} · {live.skills.length} skills · {live.targets.length} targets · {live.queuedWriteCount} queued</p></div>
      <div className="term-input"><span>&gt;</span><span>help · ls · doctor · sync</span></div>
    </div>
  );
}

function Palette({ skills, go, openSkill, close }: { skills: Skill[]; go: (page: SkillMPage) => void; openSkill: (name: string) => void; close: () => void }) {
  return (
    <div className="sm-veil" onMouseDown={close}>
      <div className="cmd-pal" onMouseDown={(event) => event.stopPropagation()}>
        <div className="cmd-search"><Icon d="search" /><span>Command palette</span><button className="btn-icon" onClick={close}><Icon d="x" /></button></div>
        <div className="cmd-list">
          {pages.map((page) => <button key={page.id} className="cmd-item" onClick={() => go(page.id)}><Icon d={page.icon} />Go to {page.label}<span>{page.group}</span></button>)}
          {skills.slice(0, 8).map((skill) => <button key={skill.name} className="cmd-item" onClick={() => openSkill(skill.name)}><Icon d="eye" />Open {skill.name}<span>{sourceLabel(skill)}</span></button>)}
        </div>
      </div>
    </div>
  );
}

function Tweaks({ dark, setDark, density, setDensity, accent, setAccent, close }: { dark: boolean; setDark: (value: boolean) => void; density: "compact" | "regular" | "comfy"; setDensity: (value: "compact" | "regular" | "comfy") => void; accent: string[]; setAccent: (value: string[]) => void; close: () => void }) {
  const themes = [["#ff0080", "#7928ca", "#00d9ff"], ["#34d399", "#0ea5e9", "#a3e635"], ["#ff6b35", "#f43f5e", "#fbbf24"]];
  return (
    <aside className="twk-panel skillm-tweaks">
      <div className="twk-hd"><b>Tweaks</b><button className="twk-x" onClick={close}>×</button></div>
      <div className="twk-body">
        <div className="twk-sect">视觉方向</div>
        <div className="twk-row"><div className="twk-lbl"><span>配色（Neon / Aurora / Sunset）</span></div><div className="twk-chips">{themes.map((theme) => <button key={theme.join("")} className="twk-chip" data-on={theme.join("") === accent.join("") ? "1" : "0"} onClick={() => setAccent(theme)}>{theme.map((color) => <i key={color} style={{ background: color }} />)}</button>)}</div></div>
        <div className="twk-row twk-row-h"><div className="twk-lbl"><span>深色模式</span></div><Switch on={dark} onChange={setDark} /></div>
        <div className="twk-sect">布局</div>
        <div className="twk-radio">{(["compact", "regular", "comfy"] as const).map((value) => <button key={value} data-on={density === value ? "1" : "0"} onClick={() => setDensity(value)}>{value}</button>)}</div>
      </div>
    </aside>
  );
}

function Toasts({ items, dismiss }: { items: Toast[]; dismiss: (id: number) => void }) {
  return <div className="sm-toasts">{items.map((toast) => <button key={toast.id} className={`sm-toast ${toast.kind}`} onClick={() => dismiss(toast.id)}><Icon d={toast.kind === "err" ? "x" : "bolt"} />{toast.text}</button>)}</div>;
}

function StatusBar({ live, counts, dark, setDark, onSync, onTerm, onTweaks }: { live: ReturnType<typeof usePanelData>; counts: { pending: number; drifted: number }; dark: boolean; setDark: (value: boolean) => void; onSync: () => void; onTerm: () => void; onTweaks: () => void }) {
  return (
    <footer className="sm-statusbar">
      <button className="sb-item sb-sync" onClick={onSync}><Icon d="sync" size={14} />{live.remote?.sync_state ?? "local"}</button>
      <span className="sb-item">{live.live ? "已同步" : "offline"} · {live.lastUpdated ? "刚刚" : "pending"}</span>
      <span className="sb-item warn">{counts.drifted} drift · {counts.pending} queued</span>
      <span className="sb-flex" />
      <button className="sb-item" onClick={onTerm}><Icon d="term" size={14} />terminal</button>
      <button className="sb-item" onClick={() => setDark(!dark)}>{dark ? "dark" : "light"}</button>
      <button className="sb-item" onClick={onTweaks}><Icon d="gear" size={14} />tweaks</button>
      <span className="sb-ver">SkillM 1.0.0</span>
    </footer>
  );
}

function tally(values: string[]) {
  return values.reduce<Record<string, number>>((acc, value) => {
    acc[value] = (acc[value] ?? 0) + 1;
    return acc;
  }, {});
}
