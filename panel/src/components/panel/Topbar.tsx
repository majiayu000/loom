import { useEffect, useMemo, useState } from "react";
import type { CommandItem, PanelPageKey } from "../../lib/types";
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
  commandItems: CommandItem[];
  onCommand: (item: CommandItem) => void | Promise<void>;
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
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [commandError, setCommandError] = useState<string | null>(null);

  const results = useMemo(() => {
    const trimmed = query.trim().toLowerCase();
    const source = props.commandItems;
    if (!trimmed) return source.slice(0, 8);
    return source
      .filter((item) => `${item.label} ${item.hint} ${item.kind}`.toLowerCase().includes(trimmed))
      .slice(0, 8);
  }, [props.commandItems, query]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen(true);
      }
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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

  const runCommand = async (item: CommandItem) => {
    setCommandError(null);
    try {
      await props.onCommand(item);
      setOpen(false);
      setQuery("");
    } catch (err) {
      setCommandError(err instanceof Error ? err.message : String(err));
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
      <div className="searchbar" style={{ width: 280, position: "relative" }}>
        <SearchIcon />
        <input
          placeholder="Jump to page, skill, target…"
          value={query}
          onFocus={() => setOpen(true)}
          onChange={(e) => {
            setQuery(e.target.value);
            setOpen(true);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && results[0]) {
              void runCommand(results[0]);
            }
          }}
        />
        <kbd>⌘K</kbd>
        {open && results.length > 0 && (
          <div
            style={{
              position: "absolute",
              top: "calc(100% + 8px)",
              left: 0,
              right: 0,
              background: "var(--bg-1)",
              border: "1px solid var(--line)",
              borderRadius: 12,
              overflow: "hidden",
              boxShadow: "0 18px 40px rgba(0,0,0,0.24)",
              zIndex: 120,
            }}
          >
            {results.map((item) => (
              <button
                key={item.id}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => void runCommand(item)}
                style={{
                  display: "grid",
                  gap: 2,
                  width: "100%",
                  textAlign: "left",
                  padding: "10px 12px",
                  background: "transparent",
                  border: "none",
                  borderBottom: "1px solid var(--line-soft)",
                }}
              >
                <span style={{ color: "var(--ink-0)", fontSize: 12.5 }}>{item.label}</span>
                <span style={{ color: "var(--ink-3)", fontSize: 11 }}>
                  {item.kind} · {item.hint}
                </span>
              </button>
            ))}
          </div>
        )}
        {commandError && (
          <div
            style={{
              position: "absolute",
              top: "calc(100% + 8px)",
              left: 0,
              right: 0,
              padding: "8px 10px",
              borderRadius: 10,
              border: "1px solid rgba(216,90,90,0.25)",
              background: "rgba(216,90,90,0.08)",
              color: "var(--err)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              zIndex: 120,
            }}
          >
            {commandError}
          </div>
        )}
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
