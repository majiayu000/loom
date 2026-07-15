import { Suspense, lazy, useEffect, useRef, useState } from "react";
import type { PanelPageKey, TweakState, VizMode } from "../lib/types";
import { usePanelData } from "../lib/api/usePanelData";
import { api } from "../lib/api/client";
import { ControlRoomShell } from "../components/panel/ControlRoomShell";
import type { ToastViewModel } from "../components/panel/Toasts";
import { OverviewPage } from "./panel/OverviewPage";
import { HistoryPage } from "./panel/HistoryPage";
import { OpsPage } from "./panel/OpsPage";
import { SettingsPage } from "./panel/SettingsPage";
import { SyncPage } from "./panel/SyncPage";
import { DoctorPage } from "./panel/DoctorPage";
import { FirstRunPage } from "./panel/FirstRunPage";
import { TelemetryPage } from "./panel/TelemetryPage";
import { selectPanelViewModel } from "../lib/panel_view_model";

const TweakPanel = lazy(() =>
  import("../components/panel/TweakPanel").then((module) => ({ default: module.TweakPanel })),
);
const SkillsPage = lazy(() => import("./panel/SkillsPage").then((module) => ({ default: module.SkillsPage })));
const ControlPlanePage = lazy(() =>
  import("./panel/ControlPlanePage").then((module) => ({ default: module.ControlPlanePage })),
);

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
const THEME_STORAGE_KEY = "loom.theme";

type PanelTheme = "dark" | "light" | "github";
const THEME_ORDER: PanelTheme[] = ["dark", "light", "github"];
const THEME_LABEL: Record<PanelTheme, string> = { dark: "Dark", light: "Warm", github: "GitHub" };
// Each theme owns its accent; switching themes resets the inline accent override
// (set by the tweaks effect) so the theme palette is not masked by it.
const THEME_ACCENT: Record<PanelTheme, string> = { dark: "#d97736", light: "#c05f23", github: "#0969da" };

function defaultTweaksForTheme(theme: PanelTheme): TweakState {
  return { ...DEFAULT_TWEAKS, accent: THEME_ACCENT[theme] };
}

function loadInitialTheme(): PanelTheme {
  const stored = localStorage.getItem(THEME_STORAGE_KEY);
  return THEME_ORDER.includes(stored as PanelTheme) ? (stored as PanelTheme) : "dark";
}
const VALID_PAGES: PanelPageKey[] = [
  "overview",
  "skills",
  "targets",
  "bindings",
  "projections",
  "telemetry",
  "ops",
  "history",
  "sync",
  "doctor",
  "settings",
];

function loadInitialPage(): PanelPageKey {
  if (hasSkillsRoute()) return "skills";
  const stored = localStorage.getItem(PAGE_STORAGE_KEY);
  return VALID_PAGES.includes(stored as PanelPageKey) ? (stored as PanelPageKey) : "overview";
}

function hasSkillsRoute(): boolean {
  if (typeof window === "undefined") return false;
  return window.location.hash === "#/skills" || window.location.hash.startsWith("#/skills/");
}

function skillRouteSelection(): string | null {
  if (typeof window === "undefined") return null;
  const prefix = "#/skills/";
  if (!window.location.hash.startsWith(prefix)) return null;
  try {
    return decodeURIComponent(window.location.hash.slice(prefix.length).split("/")[0]);
  } catch {
    return null;
  }
}

function writeSkillRoute(skillName: string | null) {
  if (typeof window === "undefined") return;
  if (skillName) {
    window.history.replaceState(null, "", `#/skills/${encodeURIComponent(skillName)}`);
    return;
  }
  if (hasSkillsRoute()) {
    window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}`);
  }
}

function loadInitialTweaks(theme: PanelTheme = loadInitialTheme()): TweakState {
  const defaults = defaultTweaksForTheme(theme);
  const raw = localStorage.getItem(TWEAKS_STORAGE_KEY);
  if (!raw) return defaults;
  try {
    const parsed = JSON.parse(raw) as Partial<TweakState>;
    return { ...defaults, ...parsed };
  } catch {
    return defaults;
  }
}

export function PanelApp() {
  const [page, setPage] = useState<PanelPageKey>(loadInitialPage);
  const [theme, setTheme] = useState<PanelTheme>(loadInitialTheme);
  const [tweaks, setTweaks] = useState<TweakState>(loadInitialTweaks);
  const [tweakVisible, setTweakVisible] = useState(false);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(skillRouteSelection);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [toasts, setToasts] = useState<ToastViewModel[]>([]);
  const toastIdRef = useRef(0);

  const live = usePanelData();

  useEffect(() => {
    localStorage.setItem(PAGE_STORAGE_KEY, page);
  }, [page]);

  useEffect(() => {
    localStorage.setItem(TWEAKS_STORAGE_KEY, JSON.stringify(tweaks));
  }, [tweaks]);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  }, [theme]);

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

  const cycleTheme = () => {
    const next = THEME_ORDER[(THEME_ORDER.indexOf(theme) + 1) % THEME_ORDER.length];
    setTheme(next);
    setTweaks((s) => ({ ...s, accent: THEME_ACCENT[next] }));
  };

  const toggleSkill = (id: string) => {
    setSelectedSkill((cur) => (cur === id ? null : id));
    setSelectedTarget(null);
  };
  const toggleTarget = (id: string) => {
    setSelectedTarget((cur) => (cur === id ? null : id));
    setSelectedSkill(null);
    writeSkillRoute(null);
  };

  // Never substitute local examples for registry data. If the API is offline,
  // render empty live state with an explicit banner so the panel cannot
  // masquerade any fabricated skills as a real registry.
  const skills = live.skills;
  const targets = live.targets;
  const bindings = live.bindings;
  const ops = live.ops;

  useEffect(() => {
    const routedSkill = skillRouteSelection();
    if (!routedSkill || skills.length === 0) return;
    const match = skills.find((skill) => skill.name === routedSkill || skill.id === routedSkill);
    if (match && selectedSkill !== match.id) {
      setSelectedSkill(match.id);
    }
  }, [skills, selectedSkill]);

  const densityClass = tweaks.density === "dense" ? " dense" : tweaks.density === "cozy" ? " cozy" : "";

  const [mutationVersion, setMutationVersion] = useState(0);
  // Gate: all mutation affordances in child pages receive this prop.
  // Future shortcuts, command palette, and hotkey handlers must check readOnly
  // before calling any /api/v1/* mutation route.
  const readOnly = live.mode !== "live";
  const historyReadOnly = readOnly || live.queuedWriteCount > 0;
  const historyReadOnlyReason =
    live.queuedWriteCount > 0 ? "operation backlog must be replayed or purged first" : undefined;
  const viewModel = selectPanelViewModel(live, { page, readOnly, historyReadOnly });
  const onMutation = () => {
    setMutationVersion((cur) => cur + 1);
    live.refetch();
  };
  const pushToast = (toast: Omit<ToastViewModel, "id">) => {
    const id = `toast-${++toastIdRef.current}`;
    setToasts((current) => [...current.slice(-2), { ...toast, id }]);
  };
  const dismissToast = (id: string) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  };
  const replayQueued = async () => {
    const action = viewModel.actions.replayQueued;
    if (!action.enabled) {
      pushToast({
        tone: "warn",
        title: action.label,
        detail: action.disabledReason ?? "action unavailable",
      });
      return;
    }
    try {
      await api.syncReplay();
      pushToast({
        tone: "success",
        title: "Queued writes replayed",
        detail: "Live registry data is refreshing.",
      });
      onMutation();
    } catch (error) {
      pushToast({
        tone: "error",
        title: "Replay failed",
        detail: error instanceof Error ? error.message : String(error),
      });
    }
  };
  const onRemoveTarget = (id: string) => {
    setSelectedTarget((cur) => (cur === id ? null : cur));
  };
  const navigatePage = (nextPage: PanelPageKey) => {
    setPage(nextPage);
    if (nextPage !== "skills") writeSkillRoute(null);
  };
  const selectSkillFromSkillsPage = (id: string | null) => {
    setSelectedSkill(id);
    setSelectedTarget(null);
    writeSkillRoute(id ? skills.find((skill) => skill.id === id)?.name ?? id : null);
  };
  const onNewTarget = () => navigatePage("targets");
  const onNewBinding = () => navigatePage("bindings");
  const onOpenSync = () => navigatePage("sync");
  const onViewActivity = () => navigatePage("ops");
  const selectSkillFromShell = (id: string) => {
    setSelectedSkill(id);
    setSelectedTarget(null);
    writeSkillRoute(skills.find((skill) => skill.id === id)?.name ?? id);
  };
  const selectTargetFromShell = (id: string) => {
    setSelectedTarget(id);
    setSelectedSkill(null);
    writeSkillRoute(null);
  };
  const controlPlane = (initialTab: "targets" | "bindings" | "projections") => (
    <Suspense fallback={null}>
      <ControlPlanePage
        initialTab={initialTab}
        skills={skills}
        targets={targets}
        bindings={bindings}
        projections={live.projections}
        selectedTarget={selectedTarget}
        onSelectTarget={toggleTarget}
        onRemoveTarget={onRemoveTarget}
        onMutation={onMutation}
        onNavigate={navigatePage}
        readOnly={readOnly}
        mutationVersion={mutationVersion}
      />
    </Suspense>
  );

  let view: React.ReactNode;
  if (live.mode === "first-run") {
    view = <FirstRunPage registryRoot={live.registryRoot} onReady={live.refetch} />;
  } else {
    switch (page) {
      case "overview":
        view = (
          <OverviewPage
            skills={skills}
            targets={targets}
            bindings={bindings}
            ops={ops}
            projections={viewModel.graphLinks}
            registryProjections={live.projections}
            remoteState={live.remote?.sync_state ?? null}
            convergence={live.convergence}
            queuedWriteCount={live.queuedWriteCount}
            operationCounts={live.operationCounts}
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
            onOpenSkills={() => navigatePage("skills")}
            onViewActivity={onViewActivity}
            onOpenSync={onOpenSync}
            readOnly={readOnly}
          />
        );
        break;
      case "skills":
        view = (
          <Suspense fallback={null}>
            <SkillsPage
              skills={skills}
              targets={targets}
              bindings={bindings}
              projections={live.projections}
              selectedSkill={selectedSkill}
              onSelectSkill={selectSkillFromSkillsPage}
              onMutation={onMutation}
              readOnly={readOnly}
            />
          </Suspense>
        );
        break;
      case "targets":
        view = controlPlane("targets");
        break;
      case "bindings":
        view = controlPlane("bindings");
        break;
      case "projections":
        view = controlPlane("projections");
        break;
      case "telemetry":
        view = <TelemetryPage apiReachable={live.apiReachable} mode={live.mode} refreshKey={live.lastUpdated} />;
        break;
      case "ops":
        view = <OpsPage ops={ops} operationCounts={live.operationCounts} onMutation={onMutation} readOnly={readOnly} />;
        break;
      case "history":
        view = (
          <HistoryPage
            live={live.live}
            mode={live.mode}
            mutationVersion={mutationVersion}
            refreshKey={live.lastUpdated}
            readOnly={historyReadOnly}
            readOnlyReason={historyReadOnlyReason}
            onMutation={onMutation}
          />
        );
        break;
      case "sync":
        view = (
          <SyncPage
            remote={live.remote}
            queuedWriteCount={live.queuedWriteCount}
            operationCounts={live.operationCounts}
            registryRoot={live.registryRoot}
            refreshKey={live.lastUpdated}
            readOnly={readOnly}
            onMutation={onMutation}
          />
        );
        break;
      case "doctor":
        view = <DoctorPage apiReachable={live.apiReachable} mode={live.mode} refreshKey={live.lastUpdated} onNavigate={navigatePage} />;
        break;
      case "settings":
        view = <SettingsPage live={live.live} mode={live.mode} registryRoot={live.registryRoot} />;
        break;
    }
  }

  const banners = (
    <>
      <PanelWarningsBanner warnings={live.warnings} />
      {live.mode !== "live" && <LiveDataBanner error={live.error} loading={live.loading} mode={live.mode} />}
    </>
  );

  return (
    <ControlRoomShell
      className={`${tweaks.compact ? "compact" : ""}${densityClass}`}
      page={page}
      viewModel={viewModel}
      banners={banners}
      themeLabel={THEME_LABEL[theme]}
      tweaksOpen={tweakVisible}
      toasts={toasts}
      onDismissToast={dismissToast}
      onNavigate={navigatePage}
      onSelectSkill={selectSkillFromShell}
      onSelectTarget={selectTargetFromShell}
      onReplayQueued={replayQueued}
      onCycleTheme={cycleTheme}
      onToggleTweaks={() => setTweakVisible((value) => !value)}
    >
      {view}
      {tweakVisible && (
        <Suspense fallback={null}>
          <TweakPanel state={tweaks} onChange={patchTweaks} onDismiss={() => setTweakVisible(false)} />
        </Suspense>
      )}
    </ControlRoomShell>
  );
}

export function PanelWarningsBanner({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null;

  const visible = warnings.slice(0, 3);
  const extra = warnings.length - visible.length;

  return (
    <div
      role="status"
      aria-label="Backend warnings"
      style={{
        padding: "8px 28px",
        background: "rgba(230,180,80,0.08)",
        borderBottom: "1px solid rgba(230,180,80,0.25)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--warn)",
      }}
    >
      <span style={{ marginRight: 6 }}>{warnings.length === 1 ? "Backend warning" : "Backend warnings"}:</span>
      <span style={{ color: "var(--ink-2)" }}>
        {visible.join(" · ")}
        {extra > 0 ? ` · ${extra} more` : ""}
      </span>
    </div>
  );
}

export function LiveDataBanner({
  error,
  loading,
  mode,
}: {
  error: string | null;
  loading: boolean;
  mode: "live" | "first-run" | "offline-empty" | "offline-stale";
}) {
  if (mode === "live" || mode === "first-run") return null;

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
        ? `/api unreachable — ${error}. Showing the last registry snapshot.`
        : "Live API unavailable. Showing the last registry snapshot."
      : error
      ? `/api unreachable — ${error}. Start \`loom panel\` to see live data.`
      : "Registry API unavailable. No real rows shown.";

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
