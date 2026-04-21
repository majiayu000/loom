import { useEffect, useMemo, useState } from "react";
import type { PanelPageKey, ProjectionLink, ProjectionMethod, TweakState, VizMode } from "../lib/types";
import { usePanelData } from "../lib/api/usePanelData";
import { Sidebar } from "../components/panel/Sidebar";
import { Topbar } from "../components/panel/Topbar";
import { TweakPanel } from "../components/panel/TweakPanel";
import { OverviewPage } from "./panel/OverviewPage";
import { SkillsPage } from "./panel/SkillsPage";
import { TargetsPage } from "./panel/TargetsPage";
import { BindingsPage } from "./panel/BindingsPage";
import { HistoryPage } from "./panel/HistoryPage";
import { OpsPage } from "./panel/OpsPage";
import { SettingsPage } from "./panel/SettingsPage";
import { SyncPage } from "./panel/SyncPage";

const DEFAULT_TWEAKS: TweakState = {
  vizMode: "loom",
  accent: "#d97736",
  density: "normal",
  compact: false,
  hero: "graph",
  displayFont: "Fraunces",
};

const PAGE_STORAGE_KEY = "loom.page";
const TWEAKS_STORAGE_KEY = "loom.tweaks";
const VALID_PAGES: PanelPageKey[] = [
  "overview",
  "skills",
  "targets",
  "bindings",
  "ops",
  "history",
  "sync",
  "settings",
];

function loadInitialPage(): PanelPageKey {
  const stored = localStorage.getItem(PAGE_STORAGE_KEY);
  return VALID_PAGES.includes(stored as PanelPageKey) ? (stored as PanelPageKey) : "overview";
}

function loadInitialTweaks(): TweakState {
  const raw = localStorage.getItem(TWEAKS_STORAGE_KEY);
  if (!raw) return DEFAULT_TWEAKS;
  try {
    const parsed = JSON.parse(raw) as Partial<TweakState>;
    return { ...DEFAULT_TWEAKS, ...parsed };
  } catch {
    return DEFAULT_TWEAKS;
  }
}

export function PanelApp() {
  const [page, setPage] = useState<PanelPageKey>(loadInitialPage);
  const [tweaks, setTweaks] = useState<TweakState>(loadInitialTweaks);
  const [tweakVisible, setTweakVisible] = useState(false);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);

  const live = usePanelData();

  useEffect(() => {
    localStorage.setItem(PAGE_STORAGE_KEY, page);
  }, [page]);

  useEffect(() => {
    localStorage.setItem(TWEAKS_STORAGE_KEY, JSON.stringify(tweaks));
  }, [tweaks]);

  useEffect(() => {
    document.documentElement.style.setProperty("--accent", tweaks.accent);
    const displayFontStack =
      tweaks.displayFont === "Inter"
        ? "'Inter', sans-serif"
        : tweaks.displayFont === "JetBrains Mono"
        ? "'JetBrains Mono', monospace"
        : "'Fraunces', serif";
    document.documentElement.style.setProperty("--font-display", displayFontStack);
  }, [tweaks.accent, tweaks.displayFont]);

  const setVizMode = (m: VizMode) => setTweaks((s) => ({ ...s, vizMode: m }));
  const patchTweaks = (patch: Partial<TweakState>) => setTweaks((s) => ({ ...s, ...patch }));

  const toggleSkill = (id: string) => {
    setSelectedSkill((cur) => (cur === id ? null : id));
    setSelectedTarget(null);
  };
  const toggleTarget = (id: string) => {
    setSelectedTarget((cur) => (cur === id ? null : id));
    setSelectedSkill(null);
  };

  // Never substitute local examples for registry data. If the API is offline,
  // render empty live state with an explicit banner so the panel cannot
  // masquerade any fabricated skills as a real registry.
  const skills = live.skills;
  const targets = live.targets;
  const bindings = live.bindings;
  const ops = live.ops;

  // Projection links for the graph:
  //   - use V3Projection.method verbatim (authoritative).
  const projectionLinks: ProjectionLink[] = useMemo(() => {
    return live.projections.map((p) => {
      const method: ProjectionMethod =
        p.method === "symlink" || p.method === "copy" || p.method === "materialize"
          ? p.method
          : "symlink";
      return { skillId: `s-${p.skill_id}`, targetId: p.target_id, method };
    });
  }, [live.projections]);

  const densityClass = tweaks.density === "dense" ? " dense" : tweaks.density === "cozy" ? " cozy" : "";

  const readOnly = !live.live;
  const onMutation = live.refetch;
  const onNewTarget = () => setPage("targets");
  const onNewBinding = () => setPage("bindings");
  const onOpenSync = () => setPage("sync");
  const onViewActivity = () => setPage("ops");

  let view: React.ReactNode;
  switch (page) {
    case "overview":
      view = (
        <OverviewPage
          skills={skills}
          targets={targets}
          ops={ops}
          projections={projectionLinks}
          vizMode={tweaks.vizMode}
          setVizMode={setVizMode}
          selectedSkill={selectedSkill}
          selectedTarget={selectedTarget}
          onSelectSkill={toggleSkill}
          onSelectTarget={toggleTarget}
          registryRoot={live.registryRoot}
          onMutation={onMutation}
          onNewTarget={onNewTarget}
          onNewBinding={onNewBinding}
          onViewActivity={onViewActivity}
          onOpenSync={onOpenSync}
          readOnly={readOnly}
        />
      );
      break;
    case "skills":
      view = (
        <SkillsPage
          skills={skills}
          targets={targets}
          bindings={bindings}
          selectedSkill={selectedSkill}
          onSelectSkill={(id) => setSelectedSkill(id)}
          onMutation={onMutation}
          readOnly={readOnly}
        />
      );
      break;
    case "targets":
      view = (
        <TargetsPage
          targets={targets}
          skills={skills}
          selectedTarget={selectedTarget}
          onSelectTarget={toggleTarget}
          onMutation={onMutation}
          readOnly={readOnly}
        />
      );
      break;
    case "bindings":
      view = <BindingsPage bindings={bindings} targets={targets} onMutation={onMutation} readOnly={readOnly} />;
      break;
    case "ops":
      view = <OpsPage ops={ops} onMutation={onMutation} readOnly={readOnly} />;
      break;
    case "history":
      view = <HistoryPage live={live.live} mode={live.mode} />;
      break;
    case "sync":
      view = (
        <SyncPage
          remote={live.remote}
          pendingCount={live.pendingCount}
          registryRoot={live.registryRoot}
          readOnly={readOnly}
          onMutation={onMutation}
        />
      );
      break;
    case "settings":
      view = <SettingsPage live={live.live} mode={live.mode} registryRoot={live.registryRoot} />;
      break;
  }

  return (
    <div className={`app ${tweaks.compact ? "compact" : ""}${densityClass}`}>
      <Topbar
        page={page}
        live={live.live}
        loading={live.loading}
        error={live.error}
        mode={live.mode}
        registryRoot={live.registryRoot}
        remoteState={live.remote?.sync_state}
        pendingCount={live.pendingCount}
        onReplay={onMutation}
        readOnly={readOnly}
      />
      <Sidebar
        page={page}
        setPage={setPage}
        compact={tweaks.compact}
        counts={{
          skills: skills.length,
          targets: targets.length,
          bindings: bindings.length,
          opsAttention: ops.filter((o) => o.status !== "ok").length,
        }}
        registryRoot={live.registryRoot}
      />
      <div className="main">
        {(live.loading || live.mode !== "live") && <LiveDataBanner error={live.error} loading={live.loading} mode={live.mode} />}
        {view}
      </div>
      <button
        onClick={() => setTweakVisible((v) => !v)}
        style={{
          position: "fixed",
          right: 16,
          top: 56,
          padding: "4px 10px",
          fontSize: 11,
          color: "var(--ink-3)",
          background: "var(--bg-1)",
          border: "1px solid var(--line)",
          borderRadius: 6,
          zIndex: 99,
        }}
      >
        {tweakVisible ? "hide tweaks" : "tweaks"}
      </button>
      {tweakVisible && (
        <TweakPanel state={tweaks} onChange={patchTweaks} onDismiss={() => setTweakVisible(false)} />
      )}
    </div>
  );
}

function LiveDataBanner({
  error,
  loading,
  mode,
}: {
  error: string | null;
  loading: boolean;
  mode: "live" | "offline-empty" | "offline-stale";
}) {
  if (loading && mode === "offline-empty") {
    return (
      <div
        style={{
          padding: "8px 28px",
          background: "var(--bg-2)",
          borderBottom: "1px solid var(--line)",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: "var(--ink-2)",
        }}
      >
        Fetching live registry state from <span style={{ color: "var(--ink-1)" }}>/api</span>…
      </div>
    );
  }

  const tone = mode === "offline-stale" ? "rgba(216,90,90,0.08)" : "rgba(230,180,80,0.08)";
  const border = mode === "offline-stale" ? "rgba(216,90,90,0.25)" : "rgba(230,180,80,0.25)";
  const label = mode === "offline-stale" ? "⚠ live API offline — showing last known data" : "⚠ live API offline";
  const body =
    mode === "offline-stale"
      ? error
        ? `/api unreachable — ${error}. The panel is keeping the last successful registry snapshot in read-only mode.`
        : "The live API is unavailable. The panel is keeping the last successful registry snapshot in read-only mode."
      : error
      ? `/api unreachable — ${error}. Start \`loom panel\` or the panel backend to see real registry data.`
      : "Registry API is unavailable. No real registry rows are being shown.";

  return (
    <div
      style={{
        padding: "8px 28px",
        background: tone,
        borderBottom: `1px solid ${border}`,
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: mode === "offline-stale" ? "var(--err)" : "var(--warn)",
      }}
    >
      <span style={{ marginRight: 6 }}>{label}</span>
      <span style={{ color: "var(--ink-2)" }}>{body}</span>
    </div>
  );
}
