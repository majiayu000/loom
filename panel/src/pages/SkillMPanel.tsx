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
  { id: "ops", icon: "ops", label: "Activity", group: "build" },
  { id: "history", icon: "clock", label: "Audit log", group: "ops" },
  { id: "sync", icon: "sync", label: "Git sync", group: "ops" },
  { id: "doctor", icon: "shield", label: "Doctor", group: "ops" },
  { id: "settings", icon: "gear", label: "Settings", group: "ops" },
  { id: "market", icon: "market", label: "Market", group: "ops" },
  { id: "forge", icon: "forge", label: "Forge", group: "ops" },
];

const agentMeta: Record<string, { name: string; short: string; color: string }> = {
  claude: { name: "Claude Code", short: "CC", color: "#d97757" },
  codex: { name: "Codex", short: "CX", color: "#19c37d" },
  cursor: { name: "Cursor", short: "CU", color: "#8b8bf5" },
  windsurf: { name: "Windsurf", short: "WS", color: "#58b2dc" },
  cline: { name: "Cline", short: "CL", color: "#8b5cf6" },
  copilot: { name: "Copilot", short: "CP", color: "#22c55e" },
  aider: { name: "Aider", short: "AD", color: "#f97316" },
  opencode: { name: "OpenCode", short: "OC", color: "#06b6d4" },
  "gemini-cli": { name: "Gemini CLI", short: "GM", color: "#4285f4" },
  goose: { name: "Goose", short: "GO", color: "#a855f7" },
};

const supportedAgents = ["claude", "codex", "cursor", "windsurf", "cline", "copilot", "aider", "opencode", "gemini-cli", "goose"];

function initialView(): SkillMPage {
  if (typeof window === "undefined") return "overview";
  const candidate = new URL(window.location.href).searchParams.get("view");
  return pages.some((page) => page.id === candidate) ? (candidate as SkillMPage) : "overview";
}

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

function registryLabel(root: string | null) {
  return root?.replace(/^\/Users\/[^/]+/, "~") ?? "~/.loom-registry";
}

function statusText(ok: boolean, warn: boolean) {
  if (!ok) return "需修复";
  if (warn) return "有告警";
  return "可操作";
}

function operationTone(status: Op["status"]) {
  if (status === "ok") return "done";
  if (status === "err") return "failed";
  return "pending";
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
  const [view, setView] = useState<SkillMPage>(initialView);
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

  const go = (page: SkillMPage) => {
    setView(page);
    if (typeof window !== "undefined") {
      const url = new URL(window.location.href);
      url.searchParams.set("view", page);
      window.history.replaceState(null, "", url);
    }
  };

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
        <ActivityRail view={view} counts={{ ...counts, skills: live.skills.length, targets: live.targets.length, bindings: live.bindings.length, projections: live.projections.length }} onGo={go} onTerm={() => setTermOpen((open) => !open)} />
        <main className="sm-main" data-screen-label={view}>
          {view === "overview" && <Overview live={live} counts={counts} go={go} />}
          {view === "skills" && <Skills skills={live.skills} query={query} setQuery={setQuery} selected={selected} setSelectedSkill={setSelectedSkill} />}
          {(view === "targets" || view === "bindings" || view === "projections") && <Plane live={live} tab={view} go={go} />}
          {(view === "ops" || view === "history") && <Ops live={live} history={view === "history"} runAction={runAction} />}
          {view === "sync" && <Sync live={live} runAction={runAction} />}
          {view === "doctor" && <Doctor live={live} />}
          {view === "settings" && <Settings live={live} dark={dark} setDark={setDark} density={density} setDensity={setDensity} accent={accent} setAccent={setAccent} />}
          {view === "market" && <Market live={live} />}
          {view === "forge" && <Forge live={live} />}
          {termOpen && <Terminal live={live} close={() => setTermOpen(false)} />}
        </main>
      </div>
      <StatusBar live={live} counts={counts} dark={dark} setDark={setDark} onSync={() => runAction("Replay / sync", api.syncReplay)} onTerm={() => setTermOpen((open) => !open)} onTweaks={() => setTweaksOpen((open) => !open)} />
      {paletteOpen && <Palette skills={live.skills} go={(page) => { go(page); setPaletteOpen(false); }} openSkill={(name) => { setSelectedSkill(name); go("skills"); setPaletteOpen(false); }} close={() => setPaletteOpen(false)} />}
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
  const failed = counts.failedOps;
  const root = registryLabel(live.registryRoot);
  return (
    <div className="view view-dash">
      <header className="dash-hero">
        <div>
          <div className="hero-kicker">注册表 · {statusText(live.live, counts.attention > 0 || live.warnings.length > 0)}</div>
          <h1><em>{root}</em></h1>
          <p>{live.mode} · {live.live ? "工作区已连接" : live.apiReachable ? "API 可达，注册表降级" : "API offline"} · sync {live.remote?.sync_state ?? "LOCAL_ONLY"}</p>
        </div>
        <div className="hero-orb"><span /><span /><span /><span className="orb-core" /></div>
      </header>
      <section className="stat-row">
        <Stat label="Skills" value={live.skills.length} sub={`${live.skills.filter((s) => s.observedImported).length} observed · ${live.skills.filter((s) => s.sourceStatus === "present").length} present`} icon="lib" onClick={() => go("skills")} />
        <Stat label="Targets" value={live.targets.length} sub={`${ownership.managed ?? 0} managed · ${ownership.observed ?? 0} observed`} icon="target" onClick={() => go("targets")} />
        <Stat label="Bindings" value={live.bindings.length} sub="matcher -> target" icon="branch" onClick={() => go("bindings")} />
        <Stat label="Projections" value={live.projections.length} sub={`${health.healthy ?? 0} healthy · ${counts.drifted} drift · ${counts.pending} pending`} icon="graph" hot={counts.drifted === 0} onClick={() => go("projections")} />
      </section>
      {live.error && <div className="dash-attn"><div className="da-text"><span className="da-fail"><b>API</b>{live.error}</span></div></div>}
      {live.warnings.length > 0 && <div className="dash-attn"><div className="da-text">{live.warnings.slice(0, 3).map((warning) => <span key={warning}><b>warning</b>{warning}</span>)}</div></div>}
      {(counts.pending || failed || counts.drifted) ? (
        <div className="dash-attn">
          <span className="da-text">
            {counts.drifted ? <span><b>{counts.drifted}</b> 投影漂移</span> : null}
            {counts.pending ? <span><b>{counts.pending}</b> pending</span> : null}
            {failed ? <span className="da-fail"><b>{failed}</b> 失败</span> : null}
          </span>
          <div className="da-acts">
            <button className="da-link" onClick={() => go("ops")}>Activity {"->"}</button>
            <button className="da-link" onClick={() => go("doctor")}>Doctor {"->"}</button>
          </div>
        </div>
      ) : null}
      <div className="dash-grid">
        <section className="panel">
          <div className="panel-head"><h3><Icon d="bolt" />用量活跃度</h3><span className="panel-hint">ops / pending · 近 26 周</span></div>
          <Heatmap ops={live.ops} queued={live.queuedWriteCount} />
        </section>
        <section className="panel">
          <div className="panel-head"><h3><Icon d="lib" />Top Skills</h3><button className="link-btn" onClick={() => go("skills")}>查看 {"->"}</button></div>
          <div className="ov-topskills">
            {live.skills.slice(0, 6).map((skill, index) => (
              <button className="ovts-row" key={skill.name} onClick={() => go("skills")}>
                <span className="ovts-rank">{index + 1}</span>
                <span className="ovts-name">{skill.name}</span>
                <span className="ovts-bar"><i style={{ width: `${Math.max(4, Math.min(100, (skill.projectionCount + skill.bindingCount + 1) * 10))}%`, background: "var(--grad)" }} /></span>
                <span className="ovts-n">{skill.projectionCount} edges</span>
              </button>
            ))}
            {live.skills.length === 0 && <div className="panel-empty">No skills from live registry yet.</div>}
          </div>
        </section>
        <section className="panel">
          <div className="panel-head"><h3><Icon d="graph" />投影健康 / 方式</h3><button className="link-btn" onClick={() => go("projections")}>查看 {"->"}</button></div>
          <HealthBars health={health} total={Math.max(1, live.projections.length)} />
          <div className="ov-methods">
            {(["symlink", "copy", "materialize"] as const).map((method) => <div className="ovm" key={method}><MethodTag method={method} /><b>{methods[method] ?? 0}</b></div>)}
          </div>
        </section>
        <section className="panel">
          <div className="panel-head"><h3><Icon d="target" />Target 归属</h3><button className="link-btn" onClick={() => go("targets")}>查看 {"->"}</button></div>
          <div className="ov-own">
            {(["managed", "observed", "external"] as const).map((own) => (
              <div className="ovo-row" key={own}>
                <span className="ovo-badge" style={{ "--oc": own === "managed" ? "var(--ok)" : own === "observed" ? "var(--acc3)" : "var(--faint)" } as CSSProperties}>{own}</span>
                <span className="ovo-hint">{own === "managed" ? "loom 可写" : own === "observed" ? "只读监控" : "外部目录"}</span>
                <span className="ovo-n">{ownership[own] ?? 0}</span>
              </div>
            ))}
          </div>
          <div className="ov-own-note"><Icon d="eye" size={13} />当前 registry 只有 observed target 时，binding/projection 为空是正常状态。</div>
        </section>
      </div>
    </div>
  );
}

function Heatmap({ ops, queued }: { ops: Op[]; queued: number }) {
  const cells = Array.from({ length: 26 * 7 }, (_, i) => {
    const op = ops[i % Math.max(ops.length, 1)];
    if (i < Math.min(ops.length + queued, 26 * 7)) return op?.status === "err" ? 3 : op?.status === "pending" ? 2 : 1;
    return 0;
  });
  return (
    <div className="hm-wrap">
      <div className="hm-months">{["1月", "2月", "3月", "4月", "5月", "6月"].map((m) => <span key={m}>{m}</span>)}</div>
      <div className="hm-grid">{Array.from({ length: 26 }, (_, col) => <div className="hm-col" key={col}>{Array.from({ length: 7 }, (_, row) => {
        const value = cells[col * 7 + row] ?? 0;
        const colors = ["var(--hm0)", "color-mix(in oklch,var(--acc3) 32%,var(--bg2))", "color-mix(in oklch,var(--acc3) 65%,var(--bg2))", "var(--warn)"];
        return <i className="hm-cell" key={row} style={{ background: colors[value] }} />;
      })}</div>)}</div>
      <div className="hm-foot"><span>近 26 周 · 共 <b>{ops.length + queued}</b> 次操作 / 队列记录</span><span className="hm-leg">少 <i /><i style={{ background: "color-mix(in oklch,var(--acc3) 32%,var(--bg2))" }} /><i style={{ background: "color-mix(in oklch,var(--acc3) 65%,var(--bg2))" }} /><i style={{ background: "var(--warn)" }} /> 多</span></div>
    </div>
  );
}

function HealthBars({ health, total }: { health: Record<string, number>; total: number }) {
  const rows = [["healthy", "healthy", "var(--ok)"], ["drift", "drift", "var(--warn)"], ["pending", "pending", "var(--acc3)"]] as const;
  return (
    <div className="ov-health">
      {rows.map(([key, label, color]) => {
        const n = health[key] ?? 0;
        return <div key={key} className="ovh-row"><span className="ovh-label" style={{ color }}>{label}</span><span className="ovh-track"><i style={{ width: `${(n / total) * 100}%`, background: color, boxShadow: n ? `0 0 8px ${color}` : "none" }} /></span><b>{n}</b></div>;
      })}
    </div>
  );
}

function MethodTag({ method }: { method: string }) {
  return <span className={`method-tag m-${method}`}><Icon d={method === "materialize" ? "forge" : method === "copy" ? "lib" : "branch"} size={11} />{method}</span>;
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
  const [source, setSource] = useState("all");
  const [sort, setSort] = useState("name");
  const tags = Array.from(new Set(skills.map((skill) => skill.tag))).slice(0, 8);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return skills
      .filter((skill) => !q || `${skill.name} ${skill.description ?? ""} ${skill.tag}`.toLowerCase().includes(q))
      .filter((skill) => source === "all" || sourceLabel(skill) === source || skill.sourceStatus === source)
      .sort((a, b) => sort === "edges" ? b.projectionCount - a.projectionCount : sort === "bindings" ? b.bindingCount - a.bindingCount : a.name.localeCompare(b.name));
  }, [query, skills, sort, source]);
  return (
    <div className="view view-lib">
      <header className="view-head">
        <div><h1>技能库</h1><p>{skills.length} 个 skill · live registry inventory · 真实后端数据</p></div>
        <div className="lib-head-right"><div className="searchbox"><Icon d="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索 skill…（名称 / 描述 / 标签）" /><kbd>⌘K</kbd></div><button className="btn-grad sm disabled"><Icon d="plus" size={14} />skill add</button></div>
      </header>
      <div className="filter-bar">
        <div className="chip-group">{["all", "observed", "present", "missing", "non-compliant"].map((item) => <button key={item} className={`chip ${source === item ? "on" : ""}`} onClick={() => setSource(item)}>{item === "all" ? "全部来源" : item}</button>)}</div>
        <div className="chip-group">{tags.map((tag) => <button key={tag} className="chip" onClick={() => setQuery(tag)}>#{tag}</button>)}</div>
        <div className="sort-group"><span className="sort-label">排序</span>{[["name", "名称"], ["edges", "投影"], ["bindings", "绑定"]].map(([id, label]) => <button key={id} className={`sort-pill ${sort === id ? "on" : ""}`} onClick={() => setSort(id)}>{label}</button>)}</div>
      </div>
      <div className="lib-grid">
        {shown.map((skill, index) => (
          <article key={skill.name} className={`skill-card ${selected?.name === skill.name ? "sel" : ""}`} onClick={() => setSelectedSkill(skill.name)}>
            <div className="sc-head"><Glyph>{skill.name}</Glyph><div className="sc-title"><h3>{skill.name}</h3><span className="sc-meta">{sourceLabel(skill)} · {skill.changed}</span></div><Switch on={(skill.observedTargetIds?.length ?? 0) > 0 || skill.projectionCount > 0} onChange={() => undefined} /></div>
            <p className="sc-desc">{skill.description || "No description from backend."}</p>
            <div className="sc-signals"><span className="sc-q">质量 {skill.sourceStatus === "present" ? 100 : 62}</span><span className={`sec-badge small ${skill.sourceStatus === "present" ? "verified" : "caution"}`}>{skill.sourceStatus}</span><span className="sc-cat">{skill.tag}</span></div>
            <div className="sc-tools">{supportedAgents.slice(0, 3).map((agent) => <span key={agent} className={`tool-pill ${skill.observedTargetIds?.some((id) => id.includes(agent)) ? "on" : ""}`} style={{ "--tc": agentMeta[agent]?.color ?? "var(--acc2)" } as CSSProperties}><i />{agentMeta[agent]?.short ?? agent.slice(0, 2)}</span>)}<span className="sc-scope">{skill.observedImported ? "◎ Observed" : "◉ Registry"}</span></div>
            <div className="sc-foot"><span className="sc-tags"><span className="sm-tag">#{skill.tag}</span><span className="sm-tag">{skill.latestRev}</span></span><Spark seed={index} /><span className="sc-calls">{skill.projectionCount} edges</span></div>
          </article>
        ))}
        {shown.length === 0 && <div className="lib-empty"><Icon d="lib" size={28} /><p>没有匹配当前筛选的 skill</p></div>}
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
  const drifts = live.projections.filter((p) => p.observed_drift || p.health === "drift").length;
  const pending = live.queuedWriteCount + live.ops.filter((op) => op.status === "pending").length;
  return (
    <div className="view view-plane">
      <header className="view-head"><div><h1>控制平面</h1><p>把注册表里的 skill 通过 binding 投影到各 agent 目录 · symlink / copy / materialize</p></div><button className="btn-grad sm disabled"><Icon d="branch" size={14} />应用全部投影</button></header>
      <div className="reg-strip"><span className="rs-git"><Icon d="branch" size={14} />Git 注册表</span><code>{registryLabel(live.registryRoot)}</code><span className="rs-div" /><span className="rs-stat"><b>{live.skills.length}</b> skills · <b>{live.targets.length}</b> targets</span><span className="rs-div" /><span className="rs-guard"><Icon d="bolt" size={12} />硬写保护 已开</span><span className="rs-flex" /><button className="rs-panel"><Icon d="eye" size={13} />localhost:5173</button></div>
      <div className="plane-stats">
        <button className="pstat" onClick={() => go("targets")}><span className="pstat-l">Targets</span><span className="pstat-n">{live.targets.length}</span></button>
        <button className="pstat" onClick={() => go("bindings")}><span className="pstat-l">Bindings</span><span className="pstat-n">{live.bindings.length}</span></button>
        <button className="pstat" onClick={() => go("projections")}><span className="pstat-l">Projections</span><span className="pstat-n">{live.projections.length}</span></button>
        <div className={`pstat ${drifts ? "warn" : ""}`}><span className="pstat-l">漂移</span><span className="pstat-n">{drifts}</span></div>
        <div className={`pstat ${pending ? "acc" : ""}`}><span className="pstat-l">待投影</span><span className="pstat-n">{pending}</span></div>
      </div>
      <nav className="plane-tabs">
        {([["projections", "投影关系图", "graph"], ["targets", "Targets", "target"], ["bindings", "Bindings", "branch"]] as const).map(([id, label, icon]) => <button key={id} className={`det-tab ${tab === id ? "on" : ""}`} onClick={() => go(id)}><Icon d={icon} size={14} />{label}</button>)}
        <span className="tab-flex" />
        {tab === "targets" ? <button className="btn-ghost sm disabled"><Icon d="plus" size={13} />target add</button> : null}
        {tab === "bindings" ? <button className="btn-ghost sm disabled"><Icon d="plus" size={13} />binding add</button> : null}
      </nav>
      {tab === "targets" && <div className="targets-grid">{live.targets.map((t) => <TargetCard key={t.id} target={t} />)}{live.targets.length === 0 && <EmptyPanel text="No target rows from backend." />}</div>}
      {tab === "bindings" && <BindingsTable bindings={live.bindings} />}
      {tab === "projections" && (
        <section className="panel">
          <div className="panel-head"><h3><Icon d="graph" />Projection graph</h3><span className="panel-hint">{live.projections.length} edges</span></div>
          <ProjectionGraph skills={live.skills} targets={live.targets} projections={live.projections} />
          <DataGrid columns={["skill", "target", "method", "health", "rev"]} rows={live.projections.map((p) => [p.skill_id, shortName(p.target_id), p.method, p.health, p.last_applied_rev?.slice(0, 8) || "—"])} />
        </section>
      )}
    </div>
  );
}

function TargetCard({ target }: { target: Target }) {
  const meta = agentMeta[target.agent] ?? { name: target.agent, short: target.agent.slice(0, 2).toUpperCase(), color: "var(--acc2)" };
  return <article className="target-card" style={{ "--ac": meta.color } as CSSProperties}><div className="tc-head"><span className="tc-agent" style={{ background: meta.color }}>{meta.short}</span><div className="tc-title"><h3>{meta.name}</h3><code>{target.path}</code></div><OwnBadge ownership={target.ownership} /></div><div className="tc-meta"><span>profile <b>{target.profile}</b></span><span>{target.projectedSkills ?? 0} 个投影</span><span className="tc-ok"><Icon d="check" size={11} />同步</span></div><div className="tc-actions"><button className="btn-ghost xs disabled">verify</button><button className="btn-ghost xs disabled">{target.ownership === "observed" ? "转 managed" : "capture"}</button></div></article>;
}

function OwnBadge({ ownership }: { ownership: string }) {
  const color = ownership === "managed" ? "var(--ok)" : ownership === "observed" ? "var(--acc3)" : "var(--faint)";
  return <span className={`own-badge own-${ownership}`} style={{ "--oc": color } as CSSProperties}><Icon d={ownership === "managed" ? "check" : "eye"} size={12} />{ownership}</span>;
}

function BindingsTable({ bindings }: { bindings: ReturnType<typeof usePanelData>["bindings"] }) {
  return <div className="bindings-table"><div className="bt-head"><span>Skill</span><span>Policy</span><span>Matcher</span><span>Target</span><span>方式</span><span /></div>{bindings.map((b) => <div className="bt-row" key={b.id}><span className="bt-skill"><Glyph>{b.skill}</Glyph>{b.skill}</span><span className="bt-agent"><i />{b.policy}</span><span className="bt-matcher"><b>{b.matcher.split(":")[0]}</b><code>{b.matcher.split(":").slice(1).join(":") || "—"}</code></span><span className="bt-target"><code>{shortName(b.target)}</code></span><span><MethodTag method={b.method} /></span><span className="bt-act"><button className="btn-icon disabled"><Icon d="branch" size={14} /></button></span></div>)}{bindings.length === 0 && <div className="panel-empty">No bindings yet. Create a real binding before Loom can materialize projections.</div>}</div>;
}

function DataGrid({ columns, rows }: { columns: string[]; rows: Array<Array<string | number>> }) {
  return (
    <div className="skillm-table panel">
      <table><thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead><tbody>{rows.map((row, index) => <tr key={index}>{row.map((cell, cellIndex) => <td key={cellIndex}>{cell}</td>)}</tr>)}</tbody></table>
      {rows.length === 0 && <div className="panel-empty">No live rows.</div>}
    </div>
  );
}

function ProjectionGraph({ skills, targets, projections = [] }: { skills: Skill[]; targets: Target[]; projections?: ReturnType<typeof usePanelData>["projections"] }) {
  const shownSkills = projections.length > 0 ? skills.filter((skill) => projections.some((p) => p.skill_id === skill.name)).slice(0, 8) : skills.slice(0, 8);
  const shownTargets = projections.length > 0 ? targets.filter((target) => projections.some((p) => p.target_id === target.id)).slice(0, 7) : targets.slice(0, 7);
  return (
    <div className="plane-graph skillm-mini-graph">
      <div className="pg-cols"><span>注册表 Skill 源</span><span>投影方式</span><span>Target 目录</span></div>
      <div className="skillm-graph-grid">
        <div>{shownSkills.map((skill) => <div className="pg-node-row" key={skill.name}><Glyph>{skill.name}</Glyph><span>{skill.name}</span></div>)}</div>
        <svg viewBox="0 0 500 260" className="proj-svg" aria-hidden="true">
          {projections.slice(0, 12).map((projection) => {
            const skillIndex = Math.max(0, shownSkills.findIndex((skill) => skill.name === projection.skill_id));
            const targetIndex = Math.max(0, shownTargets.findIndex((target) => target.id === projection.target_id));
            const y = 24 + skillIndex * 29;
            const ty = 24 + Math.max(targetIndex, 0) * 34;
            return <path key={projection.instance_id} d={`M20 ${y} C180 ${y}, 300 ${ty}, 480 ${ty}`} className="proj-edge" stroke={methodTone(projection.method)} />;
          })}
        </svg>
        <div>{shownTargets.map((target) => <div className="pg-node-row target" key={target.id}><span className="tc-agent">{target.agent.slice(0, 2).toUpperCase()}</span><span>{target.path}</span></div>)}</div>
      </div>
      {projections.length === 0 && <div className="panel-empty">No live projection edges yet. The graph is showing inventory columns only.</div>}
    </div>
  );
}

function Spark({ seed }: { seed: number }) {
  const pts = Array.from({ length: 8 }, (_, i) => `${i * 9},${16 - ((seed + i * 3) % 10)}`).join(" ");
  return <svg width="64" height="18" className="sm-spark"><polyline points={pts} fill="none" stroke="var(--acc3)" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" /></svg>;
}

function EmptyPanel({ text }: { text: string }) {
  return <div className="panel"><div className="panel-empty">{text}</div></div>;
}

function Ops({ live, history, runAction }: { live: ReturnType<typeof usePanelData>; history: boolean; runAction: (label: string, fn: () => Promise<unknown>) => void }) {
  const failed = live.ops.filter((op) => op.status === "err").length;
  const pending = live.ops.filter((op) => op.status === "pending").length + live.queuedWriteCount;
  const queue = live.ops.filter((op) => op.status !== "ok");
  const rows = history ? live.ops : queue;
  return (
    <div className="view view-ops">
      <header className="view-head">
        <div><h1>Ops &amp; 审计</h1><p>每条命令都来自 live API · 可重放、可诊断、可清理</p></div>
        <div className="ops-head-actions"><button className="btn-ghost sm" onClick={() => runAction("Purge ops", api.opsPurge)}><Icon d="x" />purge</button><button className="btn-grad sm" onClick={() => runAction("Replay queued ops", api.opsRetry)}><Icon d="sync" />replay 队列</button></div>
      </header>
      <div className="ops-stats"><div className={`pstat ${pending ? "acc" : ""}`}><span className="pstat-l">待处理</span><span className="pstat-n">{pending}</span></div><div className={`pstat ${failed ? "warn" : ""}`}><span className="pstat-l">失败 / 漂移</span><span className="pstat-n">{failed}</span></div><div className="pstat"><span className="pstat-l">已完成</span><span className="pstat-n">{live.ops.filter((op) => op.status === "ok").length}</span></div><div className="pstat"><span className="pstat-l">审计事件</span><span className="pstat-n">{live.ops.length}</span></div></div>
      <nav className="plane-tabs">{([["ops", "待处理队列"], ["history", "审计历史"]] as const).map(([id, label]) => <button key={id} className={`det-tab ${(history ? "history" : "ops") === id ? "on" : ""}`}><Icon d={id === "history" ? "clock" : "ops"} size={14} />{label}{id === "ops" && queue.length ? <span className="tab-count">{queue.length}</span> : null}</button>)}<span className="tab-flex" /></nav>
      <section className="ops-table">{rows.map((op) => <OpLine key={op.id} op={op} />)}{rows.length === 0 && <div className="ops-empty"><Icon d="check" size={26} /><p>{history ? "No operation history returned by API." : "队列已清空 · 没有待处理或失败的操作"}</p></div>}</section>
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
      <header className="view-head"><div><h1>注册表同步</h1><p>Git 支撑 · push / pull / replay · remote 为空时保持 local-only</p></div><div className="ops-head-actions"><button className="btn-ghost sm disabled"><Icon d="dl" size={14} />sync pull</button><button className="btn-grad sm" onClick={() => runAction("Sync replay", api.syncReplay)}><Icon d="sync" />replay</button></div></header>
      <div className="reg-strip"><span className="rs-git"><Icon d="branch" size={14} />remote origin</span><code>{remote?.url || "not configured"}</code><span className="rs-div" /><span className="rs-stat">state <b>{remote?.sync_state ?? "local_only"}</b></span><span className="rs-flex" /><button className="rs-panel" onClick={() => runAction("Sync replay", api.syncReplay)}><Icon d="sync" size={13} />replay</button></div>
      <div className="sync-grid">
        <section className="panel sync-topo-panel"><div className="panel-head"><h3><Icon d="sync" />注册表拓扑</h3><span className="panel-hint">local {"->"} origin</span></div><svg viewBox="0 0 640 300" className="sync-topo"><path className="beam on" stroke="var(--acc3)" d="M120 220 C120 110 320 130 320 86" /><path className="beam" stroke="var(--line2)" d="M520 220 C520 110 320 130 320 86" /><circle className="topo-cloud" cx="320" cy="78" r="34" /><text x="320" y="82" textAnchor="middle" className="topo-name">origin</text><circle className="topo-node self" cx="120" cy="220" r="38" /><text x="120" y="218" textAnchor="middle" className="topo-name">local</text><text x="120" y="235" textAnchor="middle" className="topo-sub">{live.queuedWriteCount} queued</text><circle className="topo-node" cx="520" cy="220" r="38" /><text x="520" y="218" textAnchor="middle" className="topo-name">team</text><text x="520" y="235" textAnchor="middle" className="topo-sub">pending remote</text></svg></section>
        <section className="panel"><div className="panel-head"><h3><Icon d="clock" />事件流</h3><span className="panel-hint">{live.ops.length} events</span></div><div className="ev-stream">{live.ops.slice(0, 6).map((op) => <div key={op.id} className={`ev-row ev-${operationTone(op.status)}`}><span className="ev-ic"><Icon d={op.status === "ok" ? "check" : op.status === "err" ? "bolt" : "sync"} size={13} /></span><span className="ev-time">{op.time}</span><span className="ev-text">{op.kind} <b>{op.skill}</b></span><span className="ev-dev">{op.target}</span></div>)}{live.ops.length === 0 && <div className="panel-empty">No sync activity yet.</div>}</div></section>
      </div>
    </div>
  );
}

function Doctor({ live }: { live: ReturnType<typeof usePanelData> }) {
  const checks = [
    { id: "schema", label: "Registry schema", rows: [[live.live, `mode ${live.mode}`]] },
    { id: "api", label: "Panel API", rows: [[live.apiReachable, live.error ?? "health endpoint reachable"]] },
    { id: "state", label: "State files", rows: [[!!live.registryRoot, live.registryRoot ?? "registry root unavailable"]] },
    { id: "targets", label: "Targets", rows: [[live.targets.length > 0, `${live.targets.length} target rows`]] },
    { id: "bindings", label: "Bindings", rows: [[live.bindings.length > 0, `${live.bindings.length} binding rows`]] },
    { id: "projections", label: "Projections", rows: [[live.projections.length > 0, `${live.projections.length} projection rows`]] },
    { id: "ops", label: "Operations", rows: [[live.queuedWriteCount === 0, `${live.queuedWriteCount} queued writes`]] },
    { id: "sync", label: "Sync & history", rows: [[!!live.remote?.url, live.remote?.url ?? "remote origin not configured"]] },
    { id: "perf", label: "Performance summary", rows: [[true, `last poll ${live.lastUpdated ? "fresh" : "pending"}`]] },
  ];
  const total = checks.reduce((sum, section) => sum + section.rows.length, 0);
  const passed = checks.reduce((sum, section) => sum + section.rows.filter(([ok]) => ok).length, 0);
  return (
    <div className="view view-doctor">
      <header className="view-head"><div><h1>Doctor</h1><p>映射 loom workspace doctor · 每个失败项给出可执行下一步</p></div><button className="btn-grad sm disabled"><Icon d="shield" size={14} />重新体检</button></header>
      <div className="doc-summary"><div className={`doc-verdict ${passed === total ? "ok" : "warn"}`}><div className="dv-ring" style={{ "--p": passed / total } as CSSProperties}><span>{passed}/{total}</span></div><div className="dv-text"><strong>{passed === total ? "注册表健康" : `${total - passed} 项需要处理`}</strong><span>{live.live ? "registry live" : "registry degraded or offline"}</span></div></div><div className="doc-leg"><span className="dl ok"><i />通过 {passed}</span><span className="dl bad"><i />需处理 {total - passed}</span><span className="dl">9 个检查区</span></div></div>
      <div className="doc-secs">{checks.map((section) => { const ok = section.rows.every(([rowOk]) => rowOk); return <section key={section.id} className={`doc-sec ${ok ? "ok" : "warn"}`}><div className="ds-head"><span className={`ds-dot ${ok ? "ok" : "warn"}`} /><h3>{section.label}</h3><span className="ds-count">{section.rows.filter(([rowOk]) => rowOk).length}/{section.rows.length}</span></div><div className="ds-checks">{section.rows.map(([rowOk, text], index) => <div key={index} className={`ds-check ${rowOk ? "ok" : "bad"}`}><Icon d={rowOk ? "check" : "bolt"} size={13} /><div className="dsc-body"><span className="dsc-text">{text}</span>{!rowOk && <div className="dsc-fix"><code className="dsc-code">{section.id.toUpperCase()}</code><span>需要真实配置或写入后才会出现数据</span><button className="btn-ghost xs disabled">处理 {"->"}</button></div>}</div></div>)}</div></section>; })}</div>
    </div>
  );
}

function Settings({ live, dark, setDark, density, setDensity, accent, setAccent }: { live: ReturnType<typeof usePanelData>; dark: boolean; setDark: (value: boolean) => void; density: "compact" | "regular" | "comfy"; setDensity: (value: "compact" | "regular" | "comfy") => void; accent: string[]; setAccent: (value: string[]) => void }) {
  const themes = [["#ff0080", "#7928ca", "#00d9ff"], ["#34d399", "#0ea5e9", "#a3e635"], ["#ff6b35", "#f43f5e", "#fbbf24"]];
  return (
    <div className="view view-settings">
      <header className="view-head"><div><h1>Settings</h1><p>注册表根、远端、写保护与外观 · 与 loom workspace 配置一致</p></div></header>
      <section className="set-card"><div className="set-row"><div className="set-k"><h4>Registry root</h4><p>Git 支撑的注册表所在目录</p></div><code className="set-v">{registryLabel(live.registryRoot)}</code></div><div className="set-row"><div className="set-k"><h4>Remote origin</h4><p>团队注册表推送地址</p></div><code className="set-v">{live.remote?.url ?? "not configured"}</code></div><div className="set-row"><div className="set-k"><h4>写保护</h4><p>--root 指向工具仓库时拒绝写入</p></div><Switch on={true} onChange={() => undefined} /></div></section>
      <section className="set-card"><div className="set-cardhead"><h3>支持的 Agent（10）</h3><span>scan / status / 投影目标一致</span></div><div className="set-agents">{supportedAgents.map((agent) => <span key={agent} className="set-agent"><span className="tc-agent" style={{ background: agentMeta[agent]?.color }}>{agentMeta[agent]?.short}</span>{agentMeta[agent]?.name ?? agent}</span>)}</div></section>
      <section className="set-card"><div className="set-cardhead"><h3>外观</h3><span>本机偏好</span></div>
        <div className="set-row"><div className="set-k"><h4>Theme</h4><p>深色模式</p></div><Switch on={dark} onChange={setDark} /></div>
        <div className="set-row"><div className="set-k"><h4>Accent</h4><p>Neon / Aurora / Sunset</p></div><div className="twk-chips">{themes.map((theme) => <button key={theme.join("")} className="twk-chip" data-on={theme.join("") === accent.join("") ? "1" : "0"} onClick={() => setAccent(theme)}>{theme.map((color) => <i key={color} style={{ background: color }} />)}</button>)}</div></div>
        <div className="set-row"><div className="set-k"><h4>Density</h4><p>Layout spacing</p></div><div className="twk-radio">{(["compact", "regular", "comfy"] as const).map((value) => <button key={value} data-on={density === value ? "1" : "0"} onClick={() => setDensity(value)}>{value}</button>)}</div></div>
      </section>
    </div>
  );
}

function Switch({ on, onChange }: { on: boolean; onChange: (value: boolean) => void }) {
  return <button className={`sm-switch ${on ? "on" : ""}`} role="switch" aria-checked={on} onClick={() => onChange(!on)}><span className="knob" /></button>;
}

function Market({ live }: { live: ReturnType<typeof usePanelData> }) {
  return <div className="view view-market"><header className="view-head"><div><h1>市场</h1><p>SkillM marketplace surface · Loom V1 当前没有 marketplace backend contract</p></div><div className="searchbox"><Icon d="search" /><input disabled placeholder="等待 catalog API" /></div></header><div className="reg-banner"><div className="reg-stat"><b>0</b><span>catalog rows</span></div><span className="reg-div" /><div className="reg-stat"><b>{live.skills.length}</b><span>local skills</span></div><span className="reg-div" /><div className="reg-stat"><b>API</b><span>未声明</span></div><span className="reg-flex" /><span className="reg-src">来源：live registry only</span></div><div className="mk-cats">{["全部分类", "团队仓库", "精选", "趋势"].map((item, i) => <button key={item} className={`mk-cat ${i === 0 ? "on" : ""}`}><Icon d={i === 1 ? "layers" : "market"} size={14} />{item}<span className="mk-cat-n">0</span></button>)}</div><div className="mk-grid">{["catalog API", "install flow", "security scan"].map((name) => <article key={name} className="mk-card disabled"><div className="mk-head"><Glyph>{name}</Glyph><div className="mk-title"><h3>{name}</h3><span>backend contract missing</span></div></div><p className="mk-desc">保留 SkillM 页面骨架，但不展示假市场数据。接入真实 catalog API 后才能启用安装流。</p><div className="mk-actions"><button className="btn-ghost disabled">审查源码</button><button className="btn-grad sm disabled">安装</button></div></article>)}</div></div>;
}

function Forge({ live }: { live: ReturnType<typeof usePanelData> }) {
  return <div className="view view-forge"><header className="view-head"><div><h1>Forge</h1><p>创建 skill 的参考向导 · 当前只读，因为没有 create-wizard backend contract</p></div></header><div className="forge-steps">{["选择起点", "基本信息", "编写指令", "目标 & 发布"].map((step, i) => <><button key={step} className={`fstep ${i === 0 ? "on" : ""}`}><span className="fstep-n">{i + 1}</span>{step}</button>{i < 3 && <span className="fstep-line" />}</>)}</div><div className="forge-body panel"><div className="forge-starts">{[["layers", "从模板开始", `${live.skills.length} local skills 可参考`], ["eye", "从官方文档生成", "需要 docs ingestion API"], ["forge", "从对话提炼", "需要 transcript API"], ["bolt", "AI 辅助生成", "需要 write contract"]].map(([ic, title, body]) => <button key={title} className="fstart disabled"><Icon d={ic} size={22} /><b>{title}</b><p>{body}</p></button>)}</div><div className="forge-foot"><span /><button className="btn-grad sm disabled">继续<Icon d="arrow" size={14} /></button></div></div></div>;
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
