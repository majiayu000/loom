import { useState } from "react";
import type { PanelViewModel } from "../../lib/panel_view_model";
import { GitIcon, PlayIcon, SettingsIcon } from "../icons/nav_icons";

interface StatusBarProps {
  viewModel: PanelViewModel;
  themeLabel: string;
  tweaksOpen: boolean;
  onCycleTheme: () => void;
  onToggleTweaks: () => void;
  onReplayQueued: () => Promise<void> | void;
}

export function StatusBar({
  viewModel,
  themeLabel,
  tweaksOpen,
  onCycleTheme,
  onToggleTweaks,
  onReplayQueued,
}: StatusBarProps) {
  const [replaying, setReplaying] = useState(false);
  const replay = async () => {
    if (!viewModel.actions.replayQueued.enabled || replaying) return;
    setReplaying(true);
    try {
      await onReplayQueued();
    } finally {
      setReplaying(false);
    }
  };
  const replayAction = viewModel.actions.replayQueued;
  const queued = viewModel.shell.counts.queuedWrites.value ?? 0;

  return (
    <footer className="status-bar" aria-label="Panel status">
      <div className="status-left">
        <span className="status-pill" data-tone={viewModel.shell.status.tone} title={viewModel.shell.status.title}>
          <span className="status-dot" />
          {viewModel.shell.status.label}
        </span>
        <span className="status-meta" title={viewModel.shell.registryRoot.title}>
          {viewModel.shell.registryRoot.label}
        </span>
        <span className="status-meta" title={viewModel.shell.remoteState.title}>
          <GitIcon /> {viewModel.shell.remoteState.label}
        </span>
      </div>
      <div className="status-right">
        <span className="status-meta">{viewModel.shell.counts.skills.display} skills</span>
        <span className="status-meta">{viewModel.shell.counts.targets.display} targets</span>
        <button
          className="status-action"
          type="button"
          onClick={replay}
          disabled={!replayAction.enabled || replaying}
          title={replayAction.disabledReason ?? `Replay ${queued} queued write${queued === 1 ? "" : "s"}`}
        >
          <PlayIcon /> {replaying ? "replaying" : `queued ${queued}`}
        </button>
        <button className="status-action" type="button" onClick={onCycleTheme} title={`Theme: ${themeLabel}`}>
          {themeLabel}
        </button>
        <button
          className="status-action"
          type="button"
          onClick={onToggleTweaks}
          aria-pressed={tweaksOpen}
          title={tweaksOpen ? "Hide visual tweaks" : "Show visual tweaks"}
        >
          <SettingsIcon /> tweaks
        </button>
      </div>
    </footer>
  );
}
