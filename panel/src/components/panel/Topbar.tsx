import { useState } from "react";
import type { PanelPageKey } from "../../lib/types";
import { api } from "../../lib/api/client";
import { LoomMark } from "../icons/LoomMark";
import { GitIcon, PlayIcon, SearchIcon } from "../icons/nav_icons";

const CRUMBS: Record<PanelPageKey, string> = {
  overview: "Overview",
  skills: "Skills",
  targets: "Targets",
  bindings: "Bindings",
  ops: "Ops",
  history: "Ops history",
  sync: "Git sync",
  settings: "Settings",
};

interface TopbarProps {
  page: PanelPageKey;
  live: boolean;
  loading: boolean;
  error: string | null;
  registryRoot: string | null;
  remoteState?: string;
  pendingCount: number;
  onReplay: () => void;
  readOnly: boolean;
}

function statusDisplay(props: TopbarProps): { label: string; dotStyle: React.CSSProperties } {
  if (props.error) {
    return {
      label: "registry error",
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
    };
  }
  if (props.loading) {
    return {
      label: "connecting…",
      dotStyle: { background: "var(--pending)", boxShadow: "0 0 0 3px rgba(194,160,94,0.14)" },
    };
  }
  if (!props.live) {
    return {
      label: "registry offline",
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
    };
  }
  const state = (props.remoteState ?? "").toUpperCase();
  if (state === "DIVERGED" || state === "CONFLICTED") {
    return {
      label: `remote ${state.toLowerCase()}`,
      dotStyle: { background: "var(--err)", boxShadow: "0 0 0 3px rgba(216,90,90,0.18)" },
    };
  }
  if (state === "PENDING_PUSH" || state === "LOCAL_ONLY" || props.pendingCount > 0) {
    return {
      label: props.pendingCount > 0 ? `${props.pendingCount} pending` : state.toLowerCase().replace("_", " "),
      dotStyle: { background: "var(--warn)", boxShadow: "0 0 0 3px rgba(230,180,80,0.18)" },
    };
  }
  return {
    label: "registry clean",
    dotStyle: { background: "var(--ok)", boxShadow: "0 0 0 3px rgba(111,183,138,0.14)" },
  };
}

function rootLabel(root: string | null): string {
  if (!root) return "~/.loom-registry";
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
      <div className="searchbar" style={{ width: 240 }}>
        <SearchIcon />
        <input placeholder="Jump to skill or target…" />
        <kbd>⌘K</kbd>
      </div>
      <div className="top-actions">
        <button className="top-btn" title={props.error ?? undefined}>
          <span className="status-dot" style={status.dotStyle} /> {status.label}
        </button>
        <button className="top-btn">
          <GitIcon /> {props.remoteState ? props.remoteState.toLowerCase() : "main"}
        </button>
        <button
          className="top-btn primary"
          onClick={replay}
          disabled={replaying || props.readOnly}
          title={replayError ?? (props.readOnly ? "registry offline" : undefined)}
        >
          <PlayIcon /> {replaying ? "replaying…" : "Replay"}
        </button>
      </div>
    </div>
  );
}
