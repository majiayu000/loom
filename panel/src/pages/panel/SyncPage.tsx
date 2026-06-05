import type { RemotePayload } from "../../types";
import type { FormEvent } from "react";
import { useEffect, useState } from "react";
import { GitIcon, PlayIcon, RefreshIcon, SyncIcon } from "../../components/icons/nav_icons";
import { api, type OpsHistoryDiagnosePayload } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import { COUNT_TERMS, formatQueuedWrites } from "../../lib/count_labels";

type DiagnoseData = NonNullable<OpsHistoryDiagnosePayload["data"]>;

interface SyncPageProps {
  remote: RemotePayload | null;
  queuedWriteCount: number;
  registryRoot: string | null;
  refreshKey?: string | null;
  readOnly: boolean;
  onMutation: () => void;
}

export function SyncPage({ remote, queuedWriteCount, registryRoot, refreshKey, readOnly, onMutation }: SyncPageProps) {
  const push = useMutation();
  const pull = useMutation();
  const replay = useMutation();
  const setRemote = useMutation();
  const historyRepair = useMutation();
  const [remoteUrl, setRemoteUrl] = useState(remote?.url ?? "");
  const [diagnose, setDiagnose] = useState<DiagnoseData | null>(null);
  const [diagnoseError, setDiagnoseError] = useState<string | null>(null);
  const [diagnoseLoading, setDiagnoseLoading] = useState(false);
  const [repairVersion, setRepairVersion] = useState(0);
  const syncBusy = push.busy || pull.busy || replay.busy || setRemote.busy || historyRepair.busy;
  const configured = remote?.configured === true;
  const state = remote?.sync_state ?? (configured ? "unknown" : "not configured");
  const stateTone = syncStateTone(state);
  const rootDisplay = registryRoot ? registryRoot.replace(/^\/Users\/[^/]+/, "~") : "—";
  const conflictCount = diagnose?.conflicts.length ?? 0;

  useEffect(() => {
    setRemoteUrl(remote?.url ?? "");
  }, [remote?.url]);

  useEffect(() => {
    if (readOnly) {
      setDiagnose(null);
      setDiagnoseError(null);
      setDiagnoseLoading(false);
      return;
    }

    const controller = new AbortController();
    setDiagnoseError(null);
    setDiagnoseLoading(true);
    api.opsHistoryDiagnose(controller.signal)
      .then((res) => {
        if (controller.signal.aborted) return;
        if (res.ok && res.data) {
          setDiagnose(res.data);
          return;
        }
        setDiagnose(null);
        setDiagnoseError(res.error?.message ?? "history diagnose returned ok=false");
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        setDiagnose(null);
        setDiagnoseError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!controller.signal.aborted) setDiagnoseLoading(false);
      });
    return () => controller.abort();
  }, [readOnly, repairVersion, refreshKey]);

  const submitRemote = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const url = remoteUrl.trim();
    if (!url || readOnly || syncBusy) return;
    setRemote.run(configured ? "remote update" : "remote set", () => api.remoteSet({ url }), onMutation);
  };

  const banner =
    setRemote.error ?? push.error ?? pull.error ?? replay.error ??
    historyRepair.error ??
    setRemote.success ?? push.success ?? pull.success ?? replay.success ?? historyRepair.success ?? null;
  const bannerType =
    setRemote.error || push.error || pull.error || replay.error || historyRepair.error ? "err" : banner ? "ok" : null;

  const runHistoryRepair = (strategy: "local" | "remote") => {
    historyRepair.run(`history repair ${strategy}`, () => api.opsHistoryRepair({ strategy }), () => {
      setRepairVersion((value) => value + 1);
      onMutation();
    });
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Git sync</h1>
          <div className="subtitle">
            Your registry is a git repo. Push/pull/replay keep its state synchronized across machines.
          </div>
        </div>
      </div>
      {banner && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: bannerType === "err" ? "var(--err)" : "var(--ok)",
            background: bannerType === "err" ? "rgba(216,90,90,0.08)" : "rgba(111,183,138,0.08)",
          }}
        >
          {banner}
        </div>
      )}
      <div className="page-body">
        <div className="kpi-row">
          <Kpi label="Sync state" value={formatSyncState(state)} tone={stateTone} valueKind="status" />
          <Kpi label="Ahead" value={remote?.ahead ?? 0} />
          <Kpi label="Behind" value={remote?.behind ?? 0} />
          <Kpi
            label={COUNT_TERMS.queuedWrites}
            value={queuedWriteCount}
            tone={queuedWriteCount > 0 ? "pending" : undefined}
          />
        </div>

        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-head">
            <h3>Remote</h3>
            <span className={`chip ${configured ? "ok" : "warn"}`}>
              {configured ? "configured" : "not configured"}
            </span>
          </div>
          <div className="card-body" style={{ fontSize: 12 }}>
            <pre className="code" style={{ marginBottom: 8 }}>
              <span className="c"># Registry root</span>
              {"\n"}
              <span className="k">--root</span> <span className="s">{rootDisplay}</span>
              {remote?.url && (
                <>
                  {"\n"}
                  <span className="c"># Remote URL</span>
                  {"\n"}
                  <span className="s">{remote.url}</span>
                </>
              )}
              {remote?.remote && (
                <>
                  {"\n"}
                  <span className="c"># Remote name</span>
                  {"\n"}
                  <span className="s">{remote.remote}</span>
                </>
              )}
            </pre>
            {remote?.tracking_ref === false && (
              <div style={{ color: "var(--warn)", fontSize: 11 }}>
                Local only: no upstream tracking branch configured.
              </div>
            )}
            <form
              onSubmit={submitRemote}
              style={{
                display: "grid",
                gridTemplateColumns: "minmax(0, 1fr) auto",
                gap: 8,
                marginTop: 12,
              }}
            >
              <input
                value={remoteUrl}
                onChange={(event) => setRemoteUrl(event.target.value)}
                placeholder="https://github.com/org/registry.git"
                disabled={readOnly || setRemote.busy}
                aria-label="Remote URL"
                style={inputStyle}
              />
              <button
                className="btn"
                disabled={readOnly || syncBusy || remoteUrl.trim().length === 0}
                title={readOnly ? "registry offline" : "set origin remote URL"}
              >
                <GitIcon /> {setRemote.busy ? "saving…" : configured ? "Update" : "Set"}
              </button>
            </form>
          </div>
        </div>

        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-head">
            <h3>History repair</h3>
            <span className={`chip ${diagnoseLoading ? "" : diagnoseError || conflictCount > 0 ? "warn" : "ok"}`}>
              {diagnoseLoading
                ? "checking"
                : diagnoseError
                  ? "diagnose failed"
                  : conflictCount > 0
                    ? `${conflictCount} conflict${conflictCount === 1 ? "" : "s"}`
                    : "clean"}
            </span>
          </div>
          <div className="card-body" style={{ fontSize: 12, color: "var(--ink-1)" }}>
            {diagnoseLoading ? (
              <span className="mono" style={{ color: "var(--ink-3)" }}>checking history branch...</span>
            ) : diagnoseError ? (
              <div style={{ color: "var(--warn)" }}>{diagnoseError}</div>
            ) : conflictCount > 0 ? (
              <>
                <div className="mono" style={{ color: "var(--ink-2)", marginBottom: 10 }}>
                  {diagnose?.conflicts[0]?.path}
                </div>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  <button className="btn" disabled={readOnly || syncBusy} onClick={() => runHistoryRepair("local")}>
                    Repair from local
                  </button>
                  <button className="btn" disabled={readOnly || syncBusy} onClick={() => runHistoryRepair("remote")}>
                    Repair from remote
                  </button>
                </div>
              </>
            ) : (
              "History branch has no path conflicts."
            )}
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <h3>Actions</h3>
          </div>
          <div className="card-body" style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            <button
              className="btn"
              disabled={readOnly || syncBusy}
              onClick={() => pull.run("sync pull", api.syncPull, onMutation)}
              title={readOnly ? "registry offline" : "fetch + fast-forward from remote"}
            >
              <SyncIcon /> {pull.busy ? "pulling…" : "Pull"}
            </button>
            <button
              className="btn"
              disabled={readOnly || syncBusy}
              onClick={() => push.run("sync push", api.syncPush, onMutation)}
              title={readOnly ? "registry offline" : "push local registry to remote"}
            >
              <SyncIcon /> {push.busy ? "pushing…" : "Push"}
            </button>
            <button
              className="btn primary"
              disabled={readOnly || syncBusy}
              onClick={() => replay.run("sync replay", api.syncReplay, onMutation)}
              title={readOnly ? "registry offline" : `replay ${formatQueuedWrites(queuedWriteCount)} against local targets`}
            >
              <PlayIcon /> {replay.busy ? "replaying…" : `Replay queued writes (${queuedWriteCount})`}
            </button>
            <button
              className="btn ghost"
              disabled={readOnly}
              onClick={onMutation}
              title="re-fetch remote status + queued writes"
            >
              <RefreshIcon /> Refresh
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

const inputStyle = {
  width: "100%",
  minWidth: 0,
  height: 32,
  border: "1px solid var(--line)",
  borderRadius: 6,
  background: "var(--bg)",
  color: "var(--ink-0)",
  padding: "0 10px",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  outline: "none",
};

function formatSyncState(state: string): string {
  return state.replace(/_/g, " ").toLowerCase();
}

function syncStateTone(state: string): "pending" | "err" | undefined {
  const normalized = state.toUpperCase();
  if (
    normalized.includes("CONFLICT") ||
    normalized.includes("DIVERGED") ||
    normalized.includes("ERROR") ||
    normalized.includes("FAILED")
  ) {
    return "err";
  }
  if (
    normalized.includes("LOCAL") ||
    normalized.includes("PENDING") ||
    normalized.includes("BEHIND") ||
    normalized.includes("UNKNOWN") ||
    normalized.includes("NOT CONFIGURED")
  ) {
    return "pending";
  }
  return undefined;
}

function Kpi({
  label,
  value,
  tone,
  valueKind,
}: {
  label: string;
  value: string | number;
  tone?: "pending" | "err";
  valueKind?: "status";
}) {
  const color = tone === "pending" ? "var(--pending)" : tone === "err" ? "var(--err)" : "var(--ink-0)";
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className={`value${valueKind === "status" ? " status-value" : ""}`} style={{ color }}>
        {value}
      </div>
    </div>
  );
}
