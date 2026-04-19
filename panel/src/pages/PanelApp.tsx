import { useEffect, useMemo, useState } from "react";
import type { PanelPageKey, ProjectionLink, ProjectionMethod, TweakState, VizMode } from "../lib/types";
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

  // Data source rule (cf. Codex P1 on PR #7):
  //   - live.live=true  -> ALWAYS show live data verbatim, even when lists
  //     are empty. An empty real registry is a legitimate state and must
  //     render as "0 targets" rather than silently swapping in mock rows.
  //   - live.live=false -> mock data as placeholder preview + explicit
  //     offline banner in <MockBanner>.
  // Writes are only enabled when live=true, so users can never act on
  // nonexistent mock IDs and hit confusing mutation errors.
  const usingMock = !live.live;
  const skills = usingMock ? SKILLS : live.skills;
  const targets = usingMock ? TARGETS : live.targets;
  const bindings = usingMock ? BINDINGS : live.bindings;
  const ops = usingMock ? OPS : live.ops;

  // Projection links for the graph:
  //   - live mode: use V3Projection.method verbatim (authoritative).
  //   - mock mode: derive from SKILLS.targets; the MockBanner already warns
  //     the user the data is synthetic, so a deterministic hash-based
  //     method distribution is acceptable purely for visual variety.
  const mockMethods: ProjectionMethod[] = ["symlink", "copy", "materialize"];
  const projectionLinks: ProjectionLink[] = useMemo(() => {
    if (!usingMock) {
      return live.projections.map((p) => {
        const method: ProjectionMethod =
          p.method === "symlink" || p.method === "copy" || p.method === "materialize"
            ? p.method
            : "symlink";
        return { skillId: p.skill_id, targetId: p.target_id, method };
      });
    }
    const out: ProjectionLink[] = [];
    for (const s of skills) {
      for (const tid of s.targets) {
        out.push({
          skillId: s.id,
          targetId: tid,
          method: mockMethods[(s.id.length + tid.length) % 3],
        });
      }
    }
    return out;
  }, [usingMock, live.projections, skills]);

  const densityClass = tweaks.density === "dense" ? " dense" : tweaks.density === "cozy" ? " cozy" : "";

  const readOnly = usingMock;
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
          projections={projectionLinks}
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
      view = <OpsPage ops={ops} />;
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
      view = <SettingsPage live={live.live} registryRoot={live.registryRoot} />;
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
