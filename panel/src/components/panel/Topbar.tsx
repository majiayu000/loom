import { useState } from "react";
import type { PanelDataMode } from "../../lib/api/usePanelData";
import type { PanelPageKey } from "../../lib/types";
import { api } from "../../lib/api/client";
import { LoomMark } from "../icons/LoomMark";
import { GitIcon, PlayIcon } from "../icons/nav_icons";

const CRUMBS: Record<PanelPageKey, string> = {
  overview: "Overview",
  skills: "Skills",
  targets: "Targets",
  bindings: "Bindings",
  projections: "Projections",
  ops: "Activity",
  history: "Audit log",
  sync: "Git sync",
  doctor: "Doctor",
  settings: "Settings",
};

interface TopbarProps {
  page: PanelPageKey;
  live: boolean;
  loading: boolean;
  error: string | null;
  mode: PanelDataMode;
  registryRoot: string | null;
  remoteState?: string;
  pendingCount: number;
  onReplay: () => void;
  onToggleTweaks: () => void;
  readOnly: boolean;
  tweaksOpen: boolean;
}

interface StatusDisplay {
  label: string;
  dotStyle: React.CSSProperties;
  title: string;
}

function statusDisplay(props: TopbarProps): StatusDisplay {
  if (props.mode === "offline-stale") {
    return {
      label: "live API offline · stale snapshot",
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
      title: "Last-known registry snapshot — live API is unreachable",
    };
  }
  if (props.error) {
    return {
      label: "registry error",
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
      title: props.error,
    };
  }
  if (props.loading) {
    return {
      label: "connecting…",
      dotStyle: { background: "var(--pending)", boxShadow: "0 0 0 3px rgba(194,160,94,0.14)" },
      title: "Connecting to the registry API",
    };
  }
  if (!props.live) {
    return {
      label: "registry offline",
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
      title: "Registry API is offline",
    };
  }
  if (props.mode === "first-run") {
    return {
      label: "registry setup required",
      dotStyle: { background: "var(--warn)", boxShadow: "0 0 0 3px rgba(230,180,80,0.18)" },
      title: "Registry has not been initialized yet — run `loom init`",
    };
  }
  const state = (props.remoteState ?? "").toUpperCase();
  if (state === "DIVERGED" || state === "CONFLICTED") {
    return {
      label: `remote ${state.toLowerCase()}`,
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
      title: "Remote and local history differ — run `loom sync pull` to reconcile",
    };
  }
  if (state === "PENDING_PUSH" || state === "LOCAL_ONLY" || props.pendingCount > 0) {
    const opNoun = props.pendingCount === 1 ? "operation" : "operations";
    const title =
      state === "LOCAL_ONLY"
        ? `${props.pendingCount} ${opNoun} queued — no Git remote configured. Run \`loom workspace remote set\` to enable push.`
        : `${props.pendingCount} ${opNoun} queued for push to the Git remote`;
    return {
      label: props.pendingCount > 0 ? `${props.pendingCount} pending` : state.toLowerCase().replace("_", " "),
      dotStyle: { background: "var(--warn)", boxShadow: "0 0 0 3px rgba(230,180,80,0.18)" },
      title,
    };
  }
  return {
    label: "registry clean",
    dotStyle: { background: "var(--ok)", boxShadow: "0 0 0 3px rgba(111,183,138,0.14)" },
    title: "Registry is in sync with the Git remote",
  };
}

function rootLabel(root: string | null): string {
  if (!root) return "not connected";
  const home = root.replace(/^\/Users\/[^/]+/, "~");
  return home;
}

export function Topbar(props: TopbarProps) {
  const status = statusDisplay(props);
  const [replaying, setReplaying] = useState(false);
  const [replayError, setReplayError] = useState<string | null>(null);

  const replay = async () => {
    setReplaying(true);
    setReplayError(null);
    try {
      await api.syncReplay();
      props.onReplay();
    } catch (e) {
      setReplayError(e instanceof Error ? e.message : String(e));
    } finally {
      setReplaying(false);
    }
  };

  return (
    <div className="topbar">
      <div className="brand">
        <div className="mark">
          <LoomMark size={20} />
        </div>
        <span className="brand-text">loom</span>
      </div>
      <div className="crumbs">
        <span className="registry">{rootLabel(props.registryRoot)}</span>
        <span className="sep">/</span>
        <span className="cur">{CRUMBS[props.page]}</span>
      </div>
      <div className="spacer" />
      <div className="top-actions">
        <span className="top-btn" title={status.title}>
          <span className="status-dot" style={status.dotStyle} /> {status.label}
        </span>
        <span className="top-btn" title={props.live ? "remote sync state" : "registry offline"}>
          <GitIcon /> {props.live ? (props.remoteState ? props.remoteState.toLowerCase() : "local only") : "offline"}
        </span>
        {(props.pendingCount > 0 || replaying || replayError) && (
          <button
            className="top-btn"
            onClick={replay}
            disabled={replaying || props.readOnly}
            title={
              replayError ??
              (props.readOnly
                ? "registry offline"
                : `Push ${props.pendingCount} queued operation${props.pendingCount === 1 ? "" : "s"} to the Git remote`)
            }
          >
            <PlayIcon /> {replaying ? "replaying…" : `Replay ${props.pendingCount}`}
          </button>
        )}
        <button
          className="top-btn"
          onClick={props.onToggleTweaks}
          title={props.tweaksOpen ? "hide visual tweaks" : "show visual tweaks"}
          aria-pressed={props.tweaksOpen}
        >
          Tweaks
        </button>
      </div>
    </div>
  );
}
