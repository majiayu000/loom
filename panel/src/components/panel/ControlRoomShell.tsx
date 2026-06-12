import { Suspense, lazy, useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { PanelViewModel } from "../../lib/panel_view_model";
import type { PanelPageKey } from "../../lib/types";
import { ActivityBar } from "./ActivityBar";
import { StatusBar } from "./StatusBar";
import type { ToastViewModel } from "./Toasts";

const CommandPalette = lazy(() =>
  import("./CommandPalette").then((module) => ({ default: module.CommandPalette })),
);
const Toasts = lazy(() => import("./Toasts").then((module) => ({ default: module.Toasts })));

interface ControlRoomShellProps {
  page: PanelPageKey;
  viewModel: PanelViewModel;
  className?: string;
  banners?: ReactNode;
  children: ReactNode;
  themeLabel: string;
  tweaksOpen: boolean;
  toasts: ToastViewModel[];
  onDismissToast: (id: string) => void;
  onNavigate: (page: PanelPageKey) => void;
  onSelectSkill: (id: string) => void;
  onSelectTarget: (id: string) => void;
  onReplayQueued: () => Promise<void> | void;
  onCycleTheme: () => void;
  onToggleTweaks: () => void;
}

export function ControlRoomShell({
  page,
  viewModel,
  className,
  banners,
  children,
  themeLabel,
  tweaksOpen,
  toasts,
  onDismissToast,
  onNavigate,
  onSelectSkill,
  onSelectTarget,
  onReplayQueued,
  onCycleTheme,
  onToggleTweaks,
}: ControlRoomShellProps) {
  const [paletteOpen, setPaletteOpen] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const navigate = (nextPage: PanelPageKey) => {
    onNavigate(nextPage);
    setPaletteOpen(false);
  };
  const selectSkill = (id: string) => {
    onSelectSkill(id);
    onNavigate("skills");
    setPaletteOpen(false);
  };
  const selectTarget = (id: string) => {
    onSelectTarget(id);
    onNavigate("targets");
    setPaletteOpen(false);
  };

  return (
    <div className={`control-room-shell${className ? ` ${className}` : ""}`}>
      <ActivityBar page={page} pages={viewModel.shell.pages} onNavigate={navigate} onOpenPalette={() => setPaletteOpen(true)} />
      <main className="shell-workspace">
        <div className="shell-page-frame">
          {banners && <div className="shell-banner-stack">{banners}</div>}
          {children}
        </div>
      </main>
      <StatusBar
        viewModel={viewModel}
        themeLabel={themeLabel}
        tweaksOpen={tweaksOpen}
        onCycleTheme={onCycleTheme}
        onToggleTweaks={onToggleTweaks}
        onReplayQueued={onReplayQueued}
      />
      {paletteOpen && (
        <Suspense fallback={null}>
          <CommandPalette
            open={paletteOpen}
            viewModel={viewModel}
            onClose={() => setPaletteOpen(false)}
            onNavigate={navigate}
            onSelectSkill={selectSkill}
            onSelectTarget={selectTarget}
            onReplayQueued={onReplayQueued}
          />
        </Suspense>
      )}
      {toasts.length > 0 && (
        <Suspense fallback={null}>
          <Toasts toasts={toasts} onDismiss={onDismissToast} />
        </Suspense>
      )}
    </div>
  );
}
