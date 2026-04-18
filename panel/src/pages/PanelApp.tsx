import { useEffect, useState } from "react";
import type { PanelPageKey, TweakState, VizMode } from "../lib/types";
import { usePanelData } from "../lib/api/usePanelData";
import { BINDINGS, OPS, SKILLS, TARGETS } from "../lib/mock_data";
import { Sidebar } from "../components/panel/Sidebar";
import { Topbar } from "../components/panel/Topbar";
import { TweakPanel } from "../components/panel/TweakPanel";
import { OverviewPage } from "./panel/OverviewPage";
import { SkillsPage } from "./panel/SkillsPage";
import { TargetsPage } from "./panel/TargetsPage";
import { BindingsPage } from "./panel/BindingsPage";
import { OpsPage } from "./panel/OpsPage";
import { PlaceholderPage } from "./panel/PlaceholderPage";

const DEFAULT_TWEAKS: TweakState = {
  vizMode: "loom",
  accent: "#d97736",
  density: "normal",
  compact: false,
  hero: "graph",
  displayFont: "Fraunces",
};

const PAGE_STORAGE_KEY = "loom.page";
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

export function PanelApp() {
  const [page, setPage] = useState<PanelPageKey>(loadInitialPage);
  const [tweaks, setTweaks] = useState<TweakState>(DEFAULT_TWEAKS);
  const [tweakVisible, setTweakVisible] = useState(false);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);

  const live = usePanelData();

  useEffect(() => {
    localStorage.setItem(PAGE_STORAGE_KEY, page);
  }, [page]);

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

  // Data source: live when reachable, mock fallback labeled so the UI never claims false live data.
  const skills = live.live && live.skills.length > 0 ? live.skills : SKILLS;
  const targets = live.live && live.targets.length > 0 ? live.targets : TARGETS;
  const bindings = live.live && live.bindings.length > 0 ? live.bindings : BINDINGS;
  const ops = live.live && live.ops.length > 0 ? live.ops : OPS;
  const usingMock =
    !live.live ||
    (live.skills.length === 0 && live.targets.length === 0 && live.bindings.length === 0 && live.ops.length === 0);

  const densityClass = tweaks.density === "dense" ? " dense" : tweaks.density === "cozy" ? " cozy" : "";

  const readOnly = !live.live || usingMock;
  const onMutation = live.refetch;
  const onNewBinding = () => setPage("bindings");

  let view: React.ReactNode;
  switch (page) {
    case "overview":
      view = (
        <OverviewPage
          skills={skills}
          targets={targets}
          ops={ops}
          vizMode={tweaks.vizMode}
          setVizMode={setVizMode}
          selectedSkill={selectedSkill}
          selectedTarget={selectedTarget}
          onSelectSkill={toggleSkill}
          onSelectTarget={toggleTarget}
          registryRoot={live.registryRoot}
          onMutation={onMutation}
          onNewBinding={onNewBinding}
          readOnly={readOnly}
        />
      );
      break;
    case "skills":
      view = (
        <SkillsPage
          skills={skills}
          targets={targets}
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
      view = <OpsPage ops={ops} />;
      break;
    default:
      view = <PlaceholderPage page={page} />;
  }

  return (
    <div className={`app ${tweaks.compact ? "compact" : ""}${densityClass}`}>
      <Topbar
        page={page}
        live={live.live}
        loading={live.loading}
        error={live.error}
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
        {usingMock && <MockBanner error={live.error} loading={live.loading} />}
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

function MockBanner({ error, loading }: { error: string | null; loading: boolean }) {
  if (loading) {
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
  return (
    <div
      style={{
        padding: "8px 28px",
        background: "rgba(230,180,80,0.08)",
        borderBottom: "1px solid rgba(230,180,80,0.25)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--warn)",
      }}
    >
      <span style={{ color: "var(--warn)", marginRight: 6 }}>⚠ mock data</span>
      <span style={{ color: "var(--ink-2)" }}>
        {error
          ? `/api unreachable — ${error}. Start with \`loom panel\` to see real registry.`
          : "Registry is empty or unreachable — showing sample data."}
      </span>
    </div>
  );
}
