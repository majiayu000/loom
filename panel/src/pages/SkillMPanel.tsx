import { useEffect, useMemo, useState } from "react";
import type { CSSProperties } from "react";
import { usePanelData } from "../lib/api/usePanelData";
import { api } from "../lib/api/client";
import type { Op, PanelPageKey, Skill, Target } from "../lib/types";
import { FirstRunPage } from "./panel/FirstRunPage";
import { DoctorPage } from "./panel/DoctorPage";
import { OperationLogRow } from "./OperationLogRow";
import { SkillMAuditHistory } from "./SkillMAuditHistory";
import { loadSkillMPreferences, saveSkillMPreferences } from "../lib/skillm_prefs";
import {
  operationActionLabel,
  operationDetailParts,
  operationStatusLabel,
  operationSubjectLabel,
} from "../lib/operation_labels";

type SkillMPage = PanelPageKey | "market" | "forge";
type ToastKind = "ok" | "err" | "info" | "sync";
type Confirm = { label: string; action: string; title: string; scope: string; undo: string; impact: string; count?: number; fn: () => Promise<unknown>; tone?: "danger" | "sync" };

interface Toast {
  id: string;
  kind: ToastKind;
  text: string;
}

const iconPath: Record<string, string> = {
  dash: "M3 3h7v9H3zM14 3h7v5h-7zM14 12h7v9h-7zM3 16h7v5H3z",
  lib: "M4 4h4v16H4zM10 4h4v16h-4zM16.5 4.5l4 1-3.5 15-4-1z",
  target: "M12 2v4M12 18v4M2 12h4M18 12h4M8 12a4 4 0 1 0 8 0 4 4 0 0 0-8 0",
  branch: "M6 3v12M6 15a3 3 0 103 3M18 9a3 3 0 10-3-3M18 9a9 9 0 01-9 9",
  graph: "M5 5a2 2 0 104 0 2 2 0 10-4 0M15 12a2 2 0 104 0 2 2 0 10-4 0M7 19a2 2 0 104 0 2 2 0 10-4 0M7.5 6.5l8 4.5M9.5 17.5l6-4.5",
  ops: "M4 6h10M4 12h7M4 18h10M17 7l2.5 2.5L17 12M19 16l-2.5 2.5L14 16",
  clock: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18M12 7v5l3.5 2",
  sync: "M21 12a9 9 0 01-15.5 6.2M3 12a9 9 0 0115.5-6.2M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3",
  shield: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM9 12l2 2 4-4",
  gear: "M12 8a4 4 0 100 8 4 4 0 000-8zM19 12a7 7 0 00-.1-1.2l2.1-1.6-2-3.4-2.4 1a7 7 0 00-2.1-1.2L14 3h-4l-.5 2.6a7 7 0 00-2.1 1.2l-2.4-1-2 3.4 2.1 1.6A7 7 0 005 12a7 7 0 00.1 1.2L3 14.8l2 3.4 2.4-1a7 7 0 002.1 1.2L10 21h4l.5-2.6a7 7 0 002.1-1.2l2.4 1 2-3.4-2.1-1.6A7 7 0 0019 12z",
  search: "M10.5 3a7.5 7.5 0 105.3 12.8L21 21l-1.5 1.5-5.2-5.2A7.5 7.5 0 1010.5 3z",
  term: "M4 5h16v14H4zM7 9l3 3-3 3M12 15h5",
  plus: "M12 5v14M5 12h14",
  x: "M6 6l12 12M18 6L6 18",
  check: "M5 12l4 4L19 6",
  dl: "M12 3v12M7 10l5 5 5-5M5 21h14",
  eye: "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7zm10 3a3 3 0 100-6 3 3 0 000 6z",
  bolt: "M13 2L4 14h6l-1 8 9-12h-6z",
  market: "M4 7l2-4h12l2 4M4 7h16v3a3 3 0 01-6 0 3 3 0 01-4 0 3 3 0 01-6 0V7zM5 13v8h14v-8",
  forge: "M12 2l2.4 7.2L22 12l-7.6 2.8L12 22l-2.4-7.2L2 12l7.6-2.8z",
};

const pages: Array<{ id: SkillMPage; icon: string; label: string; group: "build" | "ops"; preview?: true }> = [
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
  { id: "market", icon: "market", label: "Market", group: "ops", preview: true },
  { id: "forge", icon: "forge", label: "Forge", group: "ops", preview: true },
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

function agentsForSkill(skill: Skill, targets: Target[]) {
  const ids = new Set([...skill.targets, ...(skill.observedTargetIds ?? [])]);
  return targets.filter((target) => ids.has(target.id)).map((target) => target.agent);
}

function registryLabel(root: string | null) {
  return root?.replace(/^\/Users\/[^/]+/, "~") ?? "~/.loom-registry";
}

function panelHostLabel() {
  if (typeof window === "undefined") return "local panel";
  return window.location.host || "local panel";
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

function pendingQueueCount(live: ReturnType<typeof usePanelData>) {
  return live.operationCounts?.actionable_operations ?? Math.max(live.queuedWriteCount, live.ops.filter((op) => op.status === "pending").length);
}

function methodTone(method: string) {
  if (method === "materialize") return "var(--acc1)";
  if (method === "copy") return "var(--acc2)";
  if (method === "symlink") return "var(--acc3)";
  return "var(--faint)";
}

const PROJECTION_PAGE_SIZE = 12;
const HEATMAP_WEEKS = 26;
const HEATMAP_DAYS = HEATMAP_WEEKS * 7;
const DAY_MS = 86_400_000;

type SkillMetric = { skill: Skill; ops: number; edges: number; targets: number };

function opTimestamp(op: Op): number | null {
  const value = op.updatedAt ?? op.createdAt;
  if (!value) return null;
  const timestamp = Date.parse(value);
  return Number.isNaN(timestamp) ? null : timestamp;
}

function heatmapWindow(now = Date.now()) {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const start = new Date(today);
  start.setDate(today.getDate() - today.getDay() - (HEATMAP_WEEKS - 1) * 7);
  return { start: start.getTime(), end: start.getTime() + HEATMAP_DAYS * DAY_MS };
}

function heatmapLabels(start: number) {
  return Array.from({ length: 6 }, (_, index) => {
    const date = new Date(start + Math.round((index * (HEATMAP_WEEKS - 1)) / 5) * 7 * DAY_MS);
    return `${date.getMonth() + 1}月`;
  });
}

function opSkillKeys(op: Op) {
  return op.skill
    .split(",")
    .map((part) => part.trim().replace(/@\S+$/, ""))
    .filter(Boolean);
}

function buildSkillMetrics(skills: Skill[], ops: Op[]): SkillMetric[] {
  const knownSkills = new Set(skills.map((skill) => skill.name));
  const opCounts = new Map<string, number>();
  for (const op of ops) {
    if (opTimestamp(op) === null) continue;
    for (const name of opSkillKeys(op)) {
      if (knownSkills.has(name)) opCounts.set(name, (opCounts.get(name) ?? 0) + 1);
    }
  }
  return skills
    .map((skill) => ({
      skill,
      ops: opCounts.get(skill.name) ?? 0,
      edges: skill.projectionCount + skill.bindingCount,
      targets: skill.targets.length + (skill.observedTargetIds?.length ?? 0),
    }))
    .sort((a, b) => b.ops - a.ops || b.edges - a.edges || b.targets - a.targets || a.skill.name.localeCompare(b.skill.name));
}

export function SkillMPanel() {
  const live = usePanelData();
  const initialPrefs = useMemo(loadSkillMPreferences, []);
  const [view, setView] = useState<SkillMPage>(initialView);
  const [query, setQuery] = useState("");
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [termOpen, setTermOpen] = useState(false);
  const [tweaksOpen, setTweaksOpen] = useState(false);
  const [dark, setDark] = useState(initialPrefs.dark);
  const [density, setDensity] = useState(initialPrefs.density);
  const [accent, setAccent] = useState(initialPrefs.accent);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [confirm, setConfirm] = useState<Confirm | null>(null);

  const counts = useMemo(() => {
    const failedOps = live.ops.filter((op) => op.status === "err").length;
    const drifted = live.projections.filter((p) => p.observed_drift || p.health === "drifted").length;
    const pending = pendingQueueCount(live);
    return { failedOps, drifted, pending, attention: failedOps + drifted + pending };
  }, [live]);

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
    const id = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`;
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
  const commitAction = async () => { const action = confirm; if (!action) return; setConfirm(null); await runAction(action.label, action.fn); };

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

  useEffect(() => {
    saveSkillMPreferences({ dark, density, accent });
  }, [accent, dark, density]);

  return (
    <div
      className={`sm-app ${dark ? "dark" : "light"} den-${density}`}
      style={{ "--acc1": accent[0], "--acc2": accent[1], "--acc3": accent[2], "--glow": "0.72" } as CSSProperties}
    >
      <div className="sm-particles skillm-grid-bg" aria-hidden="true" />
      <div className="sm-frame">
        <ActivityRail view={view} counts={{ ...counts, skills: live.skills.length, targets: live.targets.length, bindings: live.bindings.length, projections: live.projections.length }} onGo={go} onTerm={() => setTermOpen((open) => !open)} />
        <main className="sm-main" data-screen-label={view}>
          {live.mode === "first-run" ? (
            <div className="view view-first-run"><FirstRunPage registryRoot={live.registryRoot} onReady={live.refetch} /></div>
          ) : (
            <>
              {view === "overview" && <Overview live={live} counts={counts} go={go} />}
              {view === "skills" && <Skills skills={live.skills} targets={live.targets} query={query} setQuery={setQuery} selected={selected} setSelectedSkill={setSelectedSkill} />}
              {(view === "targets" || view === "bindings" || view === "projections") && <Plane live={live} tab={view} go={go} />}
              {(view === "ops" || view === "history") && <Ops live={live} history={view === "history"} go={go} confirm={setConfirm} />}
              {view === "sync" && <Sync live={live} confirm={setConfirm} />}
              {view === "doctor" && <Doctor live={live} go={go} />}
              {view === "settings" && <Settings live={live} dark={dark} setDark={setDark} density={density} setDensity={setDensity} accent={accent} setAccent={setAccent} />}
              {view === "market" && <Market live={live} />}
              {view === "forge" && <Forge live={live} />}
            </>
          )}
          {termOpen && <Terminal live={live} close={() => setTermOpen(false)} />}
        </main>
      </div>
      <StatusBar live={live} counts={counts} dark={dark} setDark={setDark} onSync={() => go("sync")} onTerm={() => setTermOpen((open) => !open)} onTweaks={() => setTweaksOpen((open) => !open)} />
      {paletteOpen && <Palette skills={live.skills} go={(page) => { go(page); setPaletteOpen(false); }} openSkill={(name) => { setSelectedSkill(name); go("skills"); setPaletteOpen(false); }} close={() => setPaletteOpen(false)} />}
      {tweaksOpen && <Tweaks dark={dark} setDark={setDark} density={density} setDensity={setDensity} accent={accent} setAccent={setAccent} close={() => setTweaksOpen(false)} />}
      {confirm && <div className="sm-veil" onMouseDown={() => setConfirm(null)}><section className={`action-confirm ${confirm.tone === "danger" ? "danger" : ""}`} role="dialog" aria-modal="true" aria-label={confirm.title} onMouseDown={(event) => event.stopPropagation()}><div className="ac-head"><span className="ac-icon"><Icon d={confirm.tone === "danger" ? "x" : "sync"} /></span><div><h2>{confirm.title}</h2><p>{confirm.label}</p></div><button className="btn-icon" aria-label="关闭确认" onClick={() => setConfirm(null)}><Icon d="x" /></button></div><dl className="ac-facts"><div><dt>Affected scope</dt><dd>{confirm.scope}</dd></div>{typeof confirm.count === "number" && <div><dt>Queued count</dt><dd><b>{confirm.count}</b> queued</dd></div>}<div><dt>Reversibility</dt><dd>{confirm.undo}</dd></div><div><dt>Impact</dt><dd>{confirm.impact}</dd></div></dl><div className="ac-actions"><button className="btn-ghost sm" onClick={() => setConfirm(null)}>取消</button><button className={confirm.tone === "danger" ? "btn-ghost sm danger" : "btn-grad sm"} onClick={commitAction}>{confirm.action}</button></div></section></div>}
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
                <button key={page.id} className={`act ${view === page.id ? "on" : ""} ${page.preview ? "preview" : ""}`} title={page.preview ? `${page.label} · Preview · not connected` : page.label} aria-label={page.preview ? `${page.label} Preview not connected` : page.label} onClick={() => onGo(page.id)}>
                  <Icon d={page.icon} size={20} />
                  <span className="act-label">{page.label}</span>{page.preview && <span className="act-preview">Preview</span>}
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
  const topSkills = useMemo(() => buildSkillMetrics(live.skills, live.ops).slice(0, 6), [live.skills, live.ops]);
  const maxSkillOps = Math.max(0, ...topSkills.map((item) => item.ops));
  const maxSkillEdges = Math.max(0, ...topSkills.map((item) => item.edges));
  const skillBarBase = maxSkillOps > 0 ? maxSkillOps : maxSkillEdges;
  const skillBarValue = (item: SkillMetric) => (maxSkillOps > 0 ? item.ops : item.edges);
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
          <div className="panel-head"><h3><Icon d="bolt" />用量活跃度</h3><span className="panel-hint">ops.created_at / updated_at · 近 26 周</span></div>
          <Heatmap ops={live.ops} />
        </section>
        <section className="panel">
          <div className="panel-head"><h3><Icon d="lib" />Skill 真实统计</h3><button className="link-btn" onClick={() => go("skills")}>查看 {"->"}</button></div>
          <div className="ov-topskills">
            {live.skills.length > 0 && maxSkillOps === 0 && <div className="ovts-note">当前没有 skill usage ops；条形只按真实 registry edges 显示。</div>}
            {topSkills.map((item, index) => (
              <button className="ovts-row" key={item.skill.name} onClick={() => go("skills")}>
                <span className="ovts-rank">{index + 1}</span>
                <span className="ovts-name">{item.skill.name}</span>
                <span className="ovts-bar"><i style={{ width: `${skillBarBase > 0 ? (skillBarValue(item) / skillBarBase) * 100 : 0}%`, background: maxSkillOps > 0 ? "var(--grad)" : "color-mix(in oklch,var(--acc3) 60%,var(--bg2))" }} /></span>
                <span className="ovts-n">{item.ops} ops · {item.edges} edges</span>
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

function Heatmap({ ops }: { ops: Op[] }) {
  const { start, end } = heatmapWindow();
  const cells = Array.from({ length: HEATMAP_DAYS }, () => ({ count: 0, failed: false }));
  let stamped = 0;
  let inRange = 0;
  for (const op of ops) {
    const timestamp = opTimestamp(op);
    if (timestamp === null) continue;
    stamped += 1;
    if (timestamp < start || timestamp >= end) continue;
    const index = Math.floor((timestamp - start) / DAY_MS);
    const cell = cells[index];
    if (!cell) continue;
    cell.count += 1;
    cell.failed ||= op.status === "err";
    inRange += 1;
  }
  const max = Math.max(0, ...cells.map((cell) => cell.count));
  const labels = heatmapLabels(start);
  const colors = ["var(--hm0)", "color-mix(in oklch,var(--acc3) 32%,var(--bg2))", "color-mix(in oklch,var(--acc3) 65%,var(--bg2))", "var(--acc3)"];
  const colorFor = (count: number) => colors[count === 0 || max === 0 ? 0 : Math.max(1, Math.ceil((count / max) * 3))] ?? colors[0];
  return (
    <div className="hm-wrap">
      <div className="hm-months">{labels.map((m, index) => <span key={`${m}-${index}`}>{m}</span>)}</div>
      <div className="hm-grid">{Array.from({ length: 26 }, (_, col) => <div className="hm-col" key={col}>{Array.from({ length: 7 }, (_, row) => {
        const index = col * 7 + row;
        const cell = cells[index] ?? { count: 0, failed: false };
        const date = new Date(start + index * DAY_MS).toISOString().slice(0, 10);
        return <i className="hm-cell" key={row} title={`${date}: ${cell.count} ops`} style={{ background: colorFor(cell.count), boxShadow: cell.failed ? "0 0 0 1px var(--warn)" : "none" }} />;
      })}</div>)}</div>
      <div className="hm-foot"><span>{inRange > 0 ? <>近 26 周 · <b>{inRange}</b> 条真实 ops 时间戳</> : "近 26 周没有可统计 ops 时间戳"}</span><span className="hm-leg">有效 {stamped}/{ops.length} <i /><i style={{ background: colors[1] }} /><i style={{ background: colors[2] }} /><i style={{ background: colors[3] }} /> 多</span></div>
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

function Skills({ skills, targets, query, setQuery, selected, setSelectedSkill }: { skills: Skill[]; targets: Target[]; query: string; setQuery: (value: string) => void; selected: Skill | null; setSelectedSkill: (name: string) => void }) {
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
        <div className="lib-head-right"><div className="searchbox"><Icon d="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索 skill…（名称 / 描述 / 标签）" /><kbd>⌘K</kbd></div><span className="soon-pill"><Icon d="plus" size={14} />新增入口未接入</span></div>
      </header>
      <div className="filter-bar">
        <div className="chip-group">{["all", "observed", "present", "missing", "non-compliant"].map((item) => <button key={item} className={`chip ${source === item ? "on" : ""}`} onClick={() => setSource(item)}>{item === "all" ? "全部来源" : item}</button>)}</div>
        <div className="chip-group">{tags.map((tag) => <button key={tag} className="chip" onClick={() => setQuery(tag)}>#{tag}</button>)}</div>
        <div className="sort-group"><span className="sort-label">排序</span>{[["name", "名称"], ["edges", "投影"], ["bindings", "绑定"]].map(([id, label]) => <button key={id} className={`sort-pill ${sort === id ? "on" : ""}`} onClick={() => setSort(id)}>{label}</button>)}</div>
      </div>
      <div className="lib-layout">
        <div className="lib-grid" role="list" aria-label="Skill list">
          {shown.map((skill) => {
            const targetAgents = agentsForSkill(skill, targets);
            const active = (skill.observedTargetIds?.length ?? 0) > 0 || skill.projectionCount > 0;
            const isSelected = selected?.name === skill.name;
            return (
              <article
                key={skill.name}
                className={`skill-card ${isSelected ? "sel" : ""}`}
                role="button"
                tabIndex={0}
                aria-pressed={isSelected}
                aria-label={`查看 ${skill.name} 详情`}
                onClick={() => setSelectedSkill(skill.name)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    setSelectedSkill(skill.name);
                  }
                }}
              >
                <div className="sc-head"><Glyph>{skill.name}</Glyph><div className="sc-title"><h3>{skill.name}</h3><span className="sc-meta">{sourceLabel(skill)} · {skill.changed}</span></div><span className={`sc-state ${active ? "on" : ""}`}>{active ? "已连接" : "未连接"}</span></div>
                <p className="sc-desc">{skill.description || "No description from backend."}</p>
                <div className="sc-signals"><span className={`sec-badge small ${skill.sourceStatus === "present" ? "verified" : "caution"}`}>{skill.sourceStatus}</span><span className="sc-cat">{skill.bindingCount} bindings</span><span className="sc-cat">{skill.projectionCount} projections</span></div>
                <div className="sc-tools">{targetAgents.slice(0, 3).map((agent) => <span key={agent} className="tool-pill on" style={{ "--tc": agentMeta[agent]?.color ?? "var(--acc2)" } as CSSProperties}><i />{agentMeta[agent]?.short ?? agent.slice(0, 2).toUpperCase()}</span>)}<span className="sc-scope">{targetAgents.length > 0 ? "real target rows" : "no target rows"}</span></div>
                <div className="sc-foot"><span className="sc-tags"><span className="sm-tag">#{skill.tag}</span><span className="sm-tag">{skill.latestRev}</span></span><span className="sc-calls">查看详情</span></div>
              </article>
            );
          })}
          {shown.length === 0 && <div className="lib-empty"><Icon d="lib" size={28} /><p>没有匹配当前筛选的 skill</p></div>}
        </div>
        <SkillDetail skill={selected} />
      </div>
    </div>
  );
}

function SkillDetail({ skill }: { skill: Skill | null }) {
  if (!skill) {
    return <aside className="skill-detail panel"><div className="panel-empty">选择一个 skill 查看来源、目标和投影统计。</div></aside>;
  }
  const targetCount = skill.targets.length + (skill.observedTargetIds?.length ?? 0);
  return (
    <aside className="skill-detail panel" aria-label={`${skill.name} detail`}>
      <div className="det-head">
        <div className="det-title"><Glyph>{skill.name}</Glyph><div><h2>{skill.name}</h2><p>{skill.description || "No backend description."}</p></div></div>
        <span className="sec-badge good"><Icon d="shield" />{skill.sourceStatus}</span>
      </div>
      <div className="det-metrics">
        <div><span>Bindings</span><b>{skill.bindingCount}</b><em>routing rules</em></div>
        <div><span>Projections</span><b>{skill.projectionCount}</b><em>materialized edges</em></div>
        <div><span>Latest rev</span><b>{skill.latestRev}</b><em>backend reported</em></div>
        <div><span>Targets</span><b>{targetCount}</b><em>observed + projected</em></div>
      </div>
    </aside>
  );
}

function Plane({ live, tab, go }: { live: ReturnType<typeof usePanelData>; tab: "targets" | "bindings" | "projections"; go: (page: SkillMPage) => void }) {
  const drifts = live.projections.filter((p) => p.observed_drift || p.health === "drifted").length;
  const pending = pendingQueueCount(live);
  const panelHost = panelHostLabel();
  const [projectionPage, setProjectionPage] = useState(0);
  const projectionPageCount = Math.max(1, Math.ceil(live.projections.length / PROJECTION_PAGE_SIZE)), activeProjectionPage = Math.min(projectionPage, projectionPageCount - 1), scopedProjections = live.projections.slice(activeProjectionPage * PROJECTION_PAGE_SIZE, activeProjectionPage * PROJECTION_PAGE_SIZE + PROJECTION_PAGE_SIZE);
  const allProjectionSkills = new Set(live.projections.map((p) => p.skill_id)), allProjectionTargets = new Set(live.projections.map((p) => p.target_id)), scopedProjectionSkills = new Set(scopedProjections.map((p) => p.skill_id)), scopedProjectionTargets = new Set(scopedProjections.map((p) => p.target_id));
  const graphSkills = live.projections.length ? live.skills.filter((skill) => scopedProjectionSkills.has(skill.name)) : live.skills, graphTargets = live.projections.length ? live.targets.filter((target) => scopedProjectionTargets.has(target.id)) : live.targets;
  const projectionScopeStats = [{ key: "skills", shown: graphSkills.length, total: live.projections.length ? live.skills.filter((skill) => allProjectionSkills.has(skill.name)).length : live.skills.length }, { key: "targets", shown: graphTargets.length, total: live.projections.length ? live.targets.filter((target) => allProjectionTargets.has(target.id)).length : live.targets.length }, { key: "projections", shown: scopedProjections.length, total: live.projections.length }].filter((item) => item.shown < item.total);
  return (
    <div className="view view-plane">
      <header className="view-head"><div><h1>控制平面</h1><p>把注册表里的 skill 通过 binding 投影到各 agent 目录 · symlink / copy / materialize</p></div><span className="soon-pill"><Icon d="branch" size={14} />批量投影未接入</span></header>
      <div className="reg-strip"><span className="rs-git"><Icon d="branch" size={14} />Git 注册表</span><code>{registryLabel(live.registryRoot)}</code><span className="rs-div" /><span className="rs-stat"><b>{live.skills.length}</b> skills · <b>{live.targets.length}</b> targets</span><span className="rs-div" /><span className="rs-guard"><Icon d="bolt" size={12} />硬写保护 已开</span><span className="rs-flex" /><span className="rs-panel" title="当前 Panel 地址"><Icon d="eye" size={13} />{panelHost}</span></div>
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
        {tab === "targets" ? <span className="soon-pill"><Icon d="plus" size={13} />target 新增未接入</span> : null}
        {tab === "bindings" ? <span className="soon-pill"><Icon d="plus" size={13} />binding 新增未接入</span> : null}
      </nav>
      {tab === "targets" && <div className="targets-grid">{live.targets.map((t) => <TargetCard key={t.id} target={t} />)}{live.targets.length === 0 && <EmptyPanel text="No target rows from backend." />}</div>}
      {tab === "bindings" && <BindingsTable bindings={live.bindings} />}
      {tab === "projections" && (
        <section className="panel">
          <div className="panel-head projection-head"><h3><Icon d="graph" />Projection graph</h3><div className="projection-scope-controls"><span className="panel-hint">{scopedProjections.length} of {live.projections.length} edges</span>{projectionPageCount > 1 && <div className="scope-pager" aria-label="Projection scope pages"><button type="button" aria-label="Previous projection page" disabled={activeProjectionPage === 0} onClick={() => setProjectionPage(Math.max(0, activeProjectionPage - 1))}>Prev</button><span>Page {activeProjectionPage + 1} of {projectionPageCount}</span><button type="button" aria-label="Next projection page" disabled={activeProjectionPage >= projectionPageCount - 1} onClick={() => setProjectionPage(Math.min(projectionPageCount - 1, activeProjectionPage + 1))}>Next</button></div>}</div></div>
          {projectionScopeStats.length > 0 && <div className="scope-disclosure" aria-label="Projection scope disclosure">{projectionScopeStats.map((item) => <span key={item.key}>displaying {item.shown} of {item.total} {item.key}</span>)}</div>}
          <ProjectionGraph skills={graphSkills} targets={graphTargets} projections={scopedProjections} />
          <DataGrid columns={["skill", "target", "method", "health", "rev"]} rows={scopedProjections.map((p) => [p.skill_id, shortName(p.target_id), p.method, p.health, p.last_applied_rev?.slice(0, 8) || "—"])} />
        </section>
      )}
    </div>
  );
}

function TargetCard({ target }: { target: Target }) {
  const meta = agentMeta[target.agent] ?? { name: target.agent, short: target.agent.slice(0, 2).toUpperCase(), color: "var(--acc2)" };
  return <article className="target-card" style={{ "--ac": meta.color } as CSSProperties}><div className="tc-head"><span className="tc-agent" style={{ background: meta.color }}>{meta.short}</span><div className="tc-title"><h3>{meta.name}</h3><code>{target.path}</code></div><OwnBadge ownership={target.ownership} /></div><div className="tc-meta"><span>profile <b>{target.profile}</b></span><span>{target.projectedSkills ?? 0} 个投影</span><span className="tc-ok"><Icon d="check" size={11} />同步</span></div><div className="tc-actions"><span className="mini-state">verify 未接入</span><span className="mini-state">{target.ownership === "observed" ? "managed 转换未接入" : "capture 未接入"}</span></div></article>;
}

function OwnBadge({ ownership }: { ownership: string }) {
  const color = ownership === "managed" ? "var(--ok)" : ownership === "observed" ? "var(--acc3)" : "var(--faint)";
  return <span className={`own-badge own-${ownership}`} style={{ "--oc": color } as CSSProperties}><Icon d={ownership === "managed" ? "check" : "eye"} size={12} />{ownership}</span>;
}

function BindingsTable({ bindings }: { bindings: ReturnType<typeof usePanelData>["bindings"] }) {
  return <div className="bindings-table"><div className="bt-head"><span>Skill</span><span>Policy</span><span>Matcher</span><span>Target</span><span>方式</span><span /></div>{bindings.map((b) => <div className="bt-row" key={b.id}><span className="bt-skill"><Glyph>{b.skill}</Glyph>{b.skill}</span><span className="bt-agent"><i />{b.policy}</span><span className="bt-matcher"><b>{b.matcher.split(":")[0]}</b><code>{b.matcher.split(":").slice(1).join(":") || "—"}</code></span><span className="bt-target"><code>{shortName(b.target)}</code></span><span><MethodTag method={b.method} /></span><span className="bt-act"><span className="mini-state">只读</span></span></div>)}{bindings.length === 0 && <div className="panel-empty">No bindings yet. Create a real binding before Loom can materialize projections.</div>}</div>;
}

function DataGrid({ columns, rows }: { columns: string[]; rows: Array<Array<string | number>> }) {
  return <div className="skillm-table panel"><table><thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead><tbody>{rows.map((row, index) => <tr key={index}>{row.map((cell, cellIndex) => <td key={cellIndex}>{cell}</td>)}</tr>)}</tbody></table>{rows.length === 0 && <div className="panel-empty">No live rows.</div>}</div>;
}

function ProjectionGraph({ skills, targets, projections = [] }: { skills: Skill[]; targets: Target[]; projections?: ReturnType<typeof usePanelData>["projections"] }) {
  const graphHeight = Math.max(260, skills.length * 35 + 20, targets.length * 40 + 20);
  return (
    <div className="plane-graph skillm-mini-graph">
      <div className="pg-cols"><span>注册表 Skill 源</span><span>投影方式</span><span>Target 目录</span></div>
      <div className="skillm-graph-grid">
        <div>{skills.map((skill) => <div className="pg-node-row" key={skill.name}><Glyph>{skill.name}</Glyph><span>{skill.name}</span></div>)}</div>
        <svg viewBox={`0 0 500 ${graphHeight}`} className="proj-svg" style={{ height: graphHeight }} aria-hidden="true">
          {projections.map((projection) => {
            const skillIndex = Math.max(0, skills.findIndex((skill) => skill.name === projection.skill_id));
            const targetIndex = Math.max(0, targets.findIndex((target) => target.id === projection.target_id));
            const y = 24 + skillIndex * 35, ty = 24 + Math.max(targetIndex, 0) * 40;
            return <path key={projection.instance_id} d={`M20 ${y} C180 ${y}, 300 ${ty}, 480 ${ty}`} className="proj-edge" stroke={methodTone(projection.method)} />;
          })}
        </svg>
        <div>{targets.map((target) => <div className="pg-node-row target" key={target.id}><span className="tc-agent">{target.agent.slice(0, 2).toUpperCase()}</span><span>{target.path}</span></div>)}</div>
      </div>
      {projections.length === 0 && <div className="panel-empty">No live projection edges yet. The graph is showing inventory columns only.</div>}
    </div>
  );
}

function EmptyPanel({ text }: { text: string }) {
  return <div className="panel"><div className="panel-empty">{text}</div></div>;
}

function Ops({ live, history, go, confirm }: { live: ReturnType<typeof usePanelData>; history: boolean; go: (page: SkillMPage) => void; confirm: (action: Confirm) => void }) {
  const counts = live.operationCounts;
  const queue = counts ? live.ops.filter((op) => op.actionable) : live.ops.filter((op) => op.status !== "ok");
  const queueCount = counts?.actionable_operations ?? Math.max(queue.length, live.queuedWriteCount);
  const rows = history ? live.ops : queue;
  return (
    <div className="view view-ops">
      <header className="view-head">
        <div><h1>Ops &amp; 审计</h1><p>每条命令都来自 live API · 可重放、可诊断、可清理</p></div>
        <div className="ops-head-actions"><button className="btn-ghost sm" onClick={() => confirm({ label: "Purge ops", action: "确认清理", title: "清理 Ops 队列？", scope: `当前本地 Ops 队列 · ${queueCount} 条待处理/失败，${live.ops.length} 条可见记录`, undo: "不可自动撤销；清理后只能依赖底层审计归档或重新执行命令恢复上下文。", impact: "将调用 Ops purge API，移除当前队列上下文并刷新面板数据。", tone: "danger", fn: api.opsPurge })}><Icon d="x" />purge</button><button className="btn-grad sm" onClick={() => confirm({ label: "Replay queued ops", action: "确认重放", title: "重放 Ops 队列？", scope: "当前本地 Ops 待处理/失败队列", count: queueCount, undo: "重放调度本身不能撤销；每条结果会继续进入操作记录。", impact: "将调用 Ops retry API，重试 pending/failed 操作，可能触发后续投影、同步或写入效果。", tone: "sync", fn: api.opsRetry })}><Icon d="sync" />replay 队列</button></div>
      </header>
      <div className="ops-stats">{[["可执行操作", counts?.actionable_operations], ["本地 journal", counts?.local_journal_events], ["待推送 history", counts?.unpushed_history_events], ["仅本地 history", counts?.local_only_history_events]].map(([label, value]) => <div className="pstat" key={label}><span className="pstat-l">{label}</span><span className="pstat-n">{value ?? "—"}</span></div>)}</div>
      <nav className="plane-tabs">{([["ops", "待处理队列"], ["history", "审计历史"]] as const).map(([id, label]) => <button key={id} className={`det-tab ${(history ? "history" : "ops") === id ? "on" : ""}`} onClick={() => go(id)}><Icon d={id === "history" ? "clock" : "ops"} size={14} />{label}{id === "ops" && queueCount ? <span className="tab-count">{queueCount}</span> : null}</button>)}<span className="tab-flex" /></nav>
      {history ? <SkillMAuditHistory live={live.live} refreshKey={live.lastUpdated} /> : <section className="ops-table">{rows.map((op) => <OperationLogRow key={op.id} op={op} />)}{rows.length === 0 && <div className="ops-empty"><Icon d="check" size={26} /><p>队列已清空 · 没有待处理或失败的操作</p></div>}</section>}
    </div>
  );
}

function Sync({ live, confirm }: { live: ReturnType<typeof usePanelData>; confirm: (action: Confirm) => void }) {
  const remote = live.remote;
  const remoteConfigured = Boolean(remote?.configured || remote?.url || remote?.remote);
  const syncOps = live.ops.filter((op) => op.kind.startsWith("sync."));
  const operationBacklog = live.operationCounts?.actionable_operations ?? remote?.operation_backlog ?? live.queuedWriteCount;
  return (
    <div className="view view-sync">
      <header className="view-head"><div><h1>注册表同步</h1><p>Git 支撑 · push / pull / replay · remote 为空时保持 local-only</p></div><div className="ops-head-actions"><span className="soon-pill"><Icon d="dl" size={14} />pull 未接入</span><button className="btn-grad sm" onClick={() => confirm({ label: "Sync replay", action: "确认重放", title: "重放同步队列？", scope: `Git sync 队列 · ${remote?.url || remote?.remote || "local-only registry"}`, count: operationBacklog, undo: "重放调度本身不能撤销；同步结果会继续写入审计事件。", impact: "将调用 Sync replay API，重试同步队列并可能更新本地注册表同步状态。", tone: "sync", fn: api.syncReplay })}><Icon d="sync" />replay</button></div></header>
      <div className="reg-strip"><span className="rs-git"><Icon d="branch" size={14} />remote origin</span><code>{remote?.url || remote?.remote || "not configured"}</code><span className="rs-div" /><span className="rs-stat">state <b>{remote?.sync_state ?? "local_only"}</b></span><span className="rs-div" /><span className="rs-stat">backlog <b>{operationBacklog}</b></span><span className="rs-flex" /><button className="rs-panel" onClick={() => confirm({ label: "Sync replay", action: "确认重放", title: "重放同步队列？", scope: `Git sync 队列 · ${remote?.url || remote?.remote || "local-only registry"}`, count: operationBacklog, undo: "重放调度本身不能撤销；同步结果会继续写入审计事件。", impact: "将调用 Sync replay API，重试同步队列并可能更新本地注册表同步状态。", tone: "sync", fn: api.syncReplay })}><Icon d="sync" size={13} />replay</button></div>
      <div className="ops-stats">{[["可执行操作", live.operationCounts?.actionable_operations], ["本地 journal", live.operationCounts?.local_journal_events], ["待推送 history", live.operationCounts?.unpushed_history_events], ["仅本地 history", live.operationCounts?.local_only_history_events]].map(([label, value]) => <div className="pstat" key={label}><span className="pstat-l">{label}</span><span className="pstat-n">{value ?? "—"}</span></div>)}</div>
      <div className="sync-grid">
        <section className="panel sync-topo-panel"><div className="panel-head"><h3><Icon d="sync" />注册表拓扑</h3><span className="panel-hint">{remoteConfigured ? "local -> origin" : "local only"}</span></div><svg viewBox="0 0 640 300" className="sync-topo"><path className={`beam ${remoteConfigured ? "on" : ""}`} stroke="var(--acc3)" d="M150 220 C150 112 320 132 320 86" /><circle className="topo-cloud" cx="320" cy="78" r="34" /><text x="320" y="82" textAnchor="middle" className="topo-name">origin</text><text x="320" y="104" textAnchor="middle" className="topo-sub">{remoteConfigured ? "configured" : "not configured"}</text><circle className="topo-node self" cx="150" cy="220" r="38" /><text x="150" y="218" textAnchor="middle" className="topo-name">local</text><text x="150" y="235" textAnchor="middle" className="topo-sub">{operationBacklog} queued</text></svg></section>
        <section className="panel"><div className="panel-head"><h3><Icon d="clock" />事件流</h3><span className="panel-hint">{syncOps.length} sync events</span></div><div className="ev-stream">{syncOps.slice(0, 6).map((op) => {
          const details = operationDetailParts(op).filter((part) => !part.startsWith("id ")).slice(0, 2);
          return <div key={op.id} className={`ev-row ev-${operationTone(op.status)}`}><span className="ev-ic"><Icon d={op.status === "ok" ? "check" : op.status === "err" ? "bolt" : "sync"} size={13} /></span><span className="ev-time">{op.time}</span><span className="ev-text"><b>{operationActionLabel(op.kind)}</b><span>{operationSubjectLabel(op)}</span></span><span className="ev-dev">{details.join(" · ") || operationStatusLabel(op.status)}</span></div>;
        })}{syncOps.length === 0 && <div className="panel-empty">No sync activity yet.</div>}</div></section>
      </div>
    </div>
  );
}

function Doctor({ live, go }: { live: ReturnType<typeof usePanelData>; go: (page: SkillMPage) => void }) {
  return <div className="view view-doctor"><DoctorPage apiReachable={live.apiReachable} mode={live.mode} refreshKey={live.lastUpdated} onNavigate={(page) => go(page)} /></div>;
}

function Settings({ live, dark, setDark, density, setDensity, accent, setAccent }: { live: ReturnType<typeof usePanelData>; dark: boolean; setDark: (value: boolean) => void; density: "compact" | "regular" | "comfy"; setDensity: (value: "compact" | "regular" | "comfy") => void; accent: string[]; setAccent: (value: string[]) => void }) {
  const themes = [["#ff0080", "#7928ca", "#00d9ff"], ["#34d399", "#0ea5e9", "#a3e635"], ["#ff6b35", "#f43f5e", "#fbbf24"]];
  return (
    <div className="view view-settings">
      <header className="view-head"><div><h1>Settings</h1><p>注册表根、远端、写保护与外观 · 与 loom workspace 配置一致</p></div></header>
      <section className="set-card"><div className="set-row"><div className="set-k"><h4>Registry root</h4><p>Git 支撑的注册表所在目录</p></div><code className="set-v">{registryLabel(live.registryRoot)}</code></div><div className="set-row"><div className="set-k"><h4>Remote origin</h4><p>团队注册表推送地址</p></div><code className="set-v">{live.remote?.url ?? live.remote?.remote ?? "not configured"}</code></div><div className="set-row"><div className="set-k"><h4>写保护</h4><p>当前 API 未暴露该配置状态</p></div><code className="set-v">未暴露</code></div></section>
      <section className="set-card"><div className="set-cardhead"><h3>Agent directories ({live.agentDirs.length})</h3><span>来自 workspace/info.agent_dirs</span></div><div className="set-agents">{live.agentDirs.map((dir) => <span key={`${dir.agent}-${dir.path}`} className="set-agent" title={dir.path}><span className="tc-agent" style={{ background: agentMeta[dir.agent]?.color }}>{agentMeta[dir.agent]?.short ?? dir.agent.slice(0, 2).toUpperCase()}</span>{dir.agent}<code>{dir.env_var ?? "no env"}</code></span>)}</div>{live.agentDirs.length === 0 && <div className="panel-empty">workspace/info 没有返回 agent_dirs。</div>}</section>
      <section className="set-card"><div className="set-cardhead"><h3>外观</h3><span>本机偏好</span></div>
        <div className="set-row"><div className="set-k"><h4>Theme</h4><p>深色模式</p></div><Switch label="切换深色模式" on={dark} onChange={setDark} /></div>
        <div className="set-row"><div className="set-k"><h4>Accent</h4><p>Neon / Aurora / Sunset</p></div><div className="twk-chips">{themes.map((theme, index) => <button key={theme.join("")} className="twk-chip" aria-label={`选择配色 ${index + 1}`} data-on={theme.join("") === accent.join("") ? "1" : "0"} onClick={() => setAccent(theme)}>{theme.map((color) => <i key={color} style={{ background: color }} />)}</button>)}</div></div>
        <div className="set-row"><div className="set-k"><h4>Density</h4><p>Layout spacing</p></div><div className="twk-radio">{(["compact", "regular", "comfy"] as const).map((value) => <button key={value} data-on={density === value ? "1" : "0"} onClick={() => setDensity(value)}>{value}</button>)}</div></div>
      </section>
    </div>
  );
}

function Switch({ on, onChange, label = "切换开关" }: { on: boolean; onChange: (value: boolean) => void; label?: string }) {
  return <button className={`sm-switch ${on ? "on" : ""}`} role="switch" aria-label={label} aria-checked={on} onClick={() => onChange(!on)}><span className="knob" /></button>;
}

function Market({ live }: { live: ReturnType<typeof usePanelData> }) {
  return <div className="view view-market"><header className="view-head"><div><h1>市场</h1><p>Preview only · 当前只连接本地注册表，尚未接入市场目录服务</p></div><span className="preview-pill"><Icon d="market" size={14} />Preview · not connected</span></header><div className="reg-banner"><div className="reg-stat"><b>{live.skills.length}</b><span>本地 skills</span></div><span className="reg-div" /><div className="reg-stat"><b>未接入</b><span>市场目录</span></div><span className="reg-flex" /><span className="reg-src">来源：本地注册表</span></div><section className="preview-panel"><div><h2>现在可用</h2><p>只读查看本地 registry 中已经存在的 skills 和数量。</p></div><div><h2>尚未接入</h2><p>市场目录、搜索、分类、评分和安装 API 还没有后端连接，因此这里不展示安装按钮或模拟安装流程。</p></div></section></div>;
}

function Forge({ live }: { live: ReturnType<typeof usePanelData> }) {
  return <div className="view view-forge"><header className="view-head"><div><h1>Forge</h1><p>Preview only · 当前只读展示本地 skill，尚未接入创建向导</p></div><span className="preview-pill"><Icon d="forge" size={14} />Preview · not connected</span></header><div className="reg-banner"><div className="reg-stat"><b>{live.skills.length}</b><span>可参考 skills</span></div><span className="reg-div" /><div className="reg-stat"><b>未接入</b><span>写入流程</span></div><span className="reg-flex" /><span className="reg-src">未创建任何本地草稿</span></div><section className="preview-panel"><div><h2>现在可用</h2><p>只读参考本地 registry 中的 skill inventory，帮助确认已有命名和标签。</p></div><div><h2>尚未接入</h2><p>创建向导、模板选择、AI 生成、文档导入、发布和写入 API 还没有后端连接，因此这里不展示创建按钮或模拟草稿流程。</p></div></section></div>;
}

function Terminal({ live, close }: { live: ReturnType<typeof usePanelData>; close: () => void }) {
  return (
    <div className="sm-terminal">
      <div className="term-head"><span><Icon d="term" /> TERMINAL - read-only preview</span><button className="btn-icon" aria-label="关闭终端预览" onClick={close}><Icon d="x" /></button></div>
      <div className="term-body"><p>SkillM Terminal - read-only preview</p><p><b>$</b> loom workspace status</p><p>{live.live ? "registry live" : live.error ?? "offline"} · {live.skills.length} skills · {live.targets.length} targets · {live.queuedWriteCount} queued</p></div>
      <div className="term-note"><span>只读命令预览</span><code>help · ls · doctor · sync</code></div>
    </div>
  );
}

function Palette({ skills, go, openSkill, close }: { skills: Skill[]; go: (page: SkillMPage) => void; openSkill: (name: string) => void; close: () => void }) {
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const filteredPages = pages.filter((page) => !q || `${page.label} ${page.group} ${page.preview ? "preview not connected" : ""}`.toLowerCase().includes(q));
  const filteredSkills = skills.filter((skill) => !q || `${skill.name} ${skill.tag} ${skill.description ?? ""}`.toLowerCase().includes(q)).slice(0, 8);
  return (
    <div className="sm-veil" onMouseDown={close}>
      <div className="cmd-pal" onMouseDown={(event) => event.stopPropagation()}>
        <div className="cmd-search"><Icon d="search" /><input autoFocus value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="搜索页面或 skill" aria-label="搜索命令" /><button className="btn-icon" aria-label="关闭命令面板" onClick={close}><Icon d="x" /></button></div>
        <div className="cmd-list">
          {filteredPages.map((page) => <button key={page.id} className={`cmd-item ${page.preview ? "preview" : ""}`} aria-label={page.preview ? `Go to ${page.label} Preview not connected` : `Go to ${page.label}`} onClick={() => go(page.id)}><Icon d={page.icon} />Go to {page.label}<span>{page.preview ? "Preview · not connected" : page.group}</span></button>)}
          {filteredSkills.map((skill) => <button key={skill.name} className="cmd-item" onClick={() => openSkill(skill.name)}><Icon d="eye" />Open {skill.name}<span>{sourceLabel(skill)}</span></button>)}
          {filteredPages.length + filteredSkills.length === 0 ? <div className="panel-empty">没有匹配的命令。</div> : null}
        </div>
      </div>
    </div>
  );
}

function Tweaks({ dark, setDark, density, setDensity, accent, setAccent, close }: { dark: boolean; setDark: (value: boolean) => void; density: "compact" | "regular" | "comfy"; setDensity: (value: "compact" | "regular" | "comfy") => void; accent: string[]; setAccent: (value: string[]) => void; close: () => void }) {
  const themes = [["#ff0080", "#7928ca", "#00d9ff"], ["#34d399", "#0ea5e9", "#a3e635"], ["#ff6b35", "#f43f5e", "#fbbf24"]];
  return (
    <aside className="twk-panel skillm-tweaks">
      <div className="twk-hd"><b>Tweaks</b><button className="twk-x" aria-label="关闭 Tweaks" onClick={close}>×</button></div>
      <div className="twk-body">
        <div className="twk-sect">视觉方向</div>
        <div className="twk-row"><div className="twk-lbl"><span>配色（Neon / Aurora / Sunset）</span></div><div className="twk-chips">{themes.map((theme, index) => <button key={theme.join("")} className="twk-chip" aria-label={`选择配色 ${index + 1}`} data-on={theme.join("") === accent.join("") ? "1" : "0"} onClick={() => setAccent(theme)}>{theme.map((color) => <i key={color} style={{ background: color }} />)}</button>)}</div></div>
        <div className="twk-row twk-row-h"><div className="twk-lbl"><span>深色模式</span></div><Switch label="切换深色模式" on={dark} onChange={setDark} /></div>
        <div className="twk-sect">布局</div>
        <div className="twk-radio">{(["compact", "regular", "comfy"] as const).map((value) => <button key={value} data-on={density === value ? "1" : "0"} onClick={() => setDensity(value)}>{value}</button>)}</div>
      </div>
    </aside>
  );
}

function Toasts({ items, dismiss }: { items: Toast[]; dismiss: (id: string) => void }) {
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
