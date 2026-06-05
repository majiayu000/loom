import { useEffect, useState } from "react";
import type { PanelDataMode } from "../../lib/api/usePanelData";
import type { InfoPayload } from "../../types";
import { api, ApiError } from "../../lib/api/client";
import { AGENT_OPTIONS } from "../../lib/agent_options";
import { CopyIcon } from "../../components/icons/nav_icons";

interface SettingsPageProps {
  live: boolean;
  mode: PanelDataMode;
  registryRoot: string | null;
}

type InfoState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; info: InfoPayload }
  | { kind: "error"; message: string };

const AGENT_LABELS = new Map(AGENT_OPTIONS.map((agent) => [agent.slug, agent.label]));

function labelForAgent(agent: string): string {
  return AGENT_LABELS.get(agent) ?? agent;
}

function agentRows(info: InfoPayload): Array<{ agent: string; path: string | undefined }> {
  if (info.agent_dirs?.length) {
    return info.agent_dirs.map((dir) => ({ agent: dir.agent, path: dir.path }));
  }

  return [
    { agent: "claude", path: info.claude_dir },
    { agent: "codex", path: info.codex_dir },
  ].filter((row) => row.path);
}

export function SettingsPage({ live, mode, registryRoot }: SettingsPageProps) {
  const [info, setInfo] = useState<InfoState>({ kind: "idle" });
  const [cleared, setCleared] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState<{ value: string; state: "copied" | "failed" } | null>(null);

  useEffect(() => {
    if (!live) {
      setInfo({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setInfo({ kind: "loading" });
    api
      .info(controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return;
        setInfo({ kind: "ready", info: payload });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setInfo({ kind: "error", message });
      });
    return () => controller.abort();
  }, [live]);

  const offlineHint =
    mode === "offline-stale"
      ? "Settings are read-only while the live API is offline."
      : "Settings need the live panel API. Start `loom panel`.";

  const resetTweaks = () => {
    localStorage.removeItem("loom.tweaks");
    setCleared(true);
    window.setTimeout(() => window.location.reload(), 400);
  };

  const canCopy = typeof navigator !== "undefined" && Boolean(navigator.clipboard?.writeText);

  const copyValue = async (value: string) => {
    if (!canCopy) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopyFeedback({ value, state: "copied" });
    } catch {
      setCopyFeedback({ value, state: "failed" });
    }
    window.setTimeout(() => setCopyFeedback((current) => (current?.value === value ? null : current)), 1200);
  };

  const rows: Array<{ label: string; value: string | undefined; mono?: boolean }> = [
    { label: "Registry root", value: registryRoot ?? undefined, mono: true },
  ];
  if (info.kind === "ready") {
    const x = info.info;
    rows.push(
      { label: "State dir", value: x.state_dir, mono: true },
      { label: "Registry targets file", value: x.registry_targets_file, mono: true },
      ...agentRows(x).map((row) => ({
        label: `${labelForAgent(row.agent)} dir`,
        value: row.path,
        mono: true,
      })),
      { label: "Remote URL", value: x.remote_url, mono: true },
    );
  }

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Settings</h1>
          <div className="subtitle">
            Where Loom keeps its state. These paths come from <span className="mono">/api/info</span> and mirror the CLI
            output of <span className="mono">loom workspace status</span>.
          </div>
        </div>
      </div>
      <div className="page-body">
        {!live && <div className="empty" style={{ marginBottom: 16 }}>{offlineHint}</div>}
        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-head">
            <h3>Registry paths</h3>
            {info.kind === "loading" && <span className="chip">loading…</span>}
            {info.kind === "error" && <span className="chip" style={{ color: "var(--err)" }}>fetch failed</span>}
          </div>
          <div className="card-body">
            {info.kind === "error" && (
              <div style={{ color: "var(--err)", fontSize: 12, marginBottom: 10 }}>{info.message}</div>
            )}
            <table className="tbl mobile-cards" style={{ fontSize: 12 }}>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.label}>
                    <td data-label="Setting" style={{ color: "var(--ink-2)", width: 160 }}>
                      {r.label}
                    </td>
                    <td
                      data-label="Value"
                      className={r.mono ? "mono settings-value-cell" : "settings-value-cell"}
                      style={{ color: r.value ? "var(--ink-0)" : "var(--ink-3)" }}
                    >
                      <SettingValue
                        value={r.value}
                        copyState={copyFeedback && r.value === copyFeedback.value ? copyFeedback.state : null}
                        canCopy={canCopy}
                        onCopy={copyValue}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <h3>UI preferences</h3>
            {cleared && <span className="chip ok">cleared · reloading…</span>}
          </div>
          <div className="card-body" style={{ fontSize: 12 }}>
            <div style={{ marginBottom: 10, color: "var(--ink-2)" }}>
              Viz mode, accent, density, font and compact toggle live in{" "}
              <span className="mono">localStorage.loom.tweaks</span>. Click below to reset to defaults and reload.
            </div>
            <button className="btn" onClick={resetTweaks} disabled={cleared}>
              Reset UI preferences
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

function SettingValue({
  value,
  copyState,
  canCopy,
  onCopy,
}: {
  value: string | undefined;
  copyState: "copied" | "failed" | null;
  canCopy: boolean;
  onCopy: (value: string) => void;
}) {
  if (!value) return <>—</>;

  return (
    <span className="setting-path-value">
      <span className="setting-path-text" title={value}>{value}</span>
      {canCopy && (
        <button className="btn sm ghost setting-copy-btn" type="button" onClick={() => void onCopy(value)} title="Copy value">
          <CopyIcon /> {copyState === "copied" ? "Copied" : copyState === "failed" ? "Failed" : "Copy"}
        </button>
      )}
    </span>
  );
}
