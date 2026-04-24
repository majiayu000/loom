import type { RemotePayload } from "../../types";
import { PlayIcon, RefreshIcon, SyncIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface SyncPageProps {
  remote: RemotePayload | null;
  pendingCount: number;
  registryRoot: string | null;
  readOnly: boolean;
  onMutation: () => void;
}

export function SyncPage({ remote, pendingCount, registryRoot, readOnly, onMutation }: SyncPageProps) {
  const push = useMutation();
  const pull = useMutation();
  const replay = useMutation();
  const syncBusy = push.busy || pull.busy || replay.busy;
  const configured = remote?.configured === true;
  const state = remote?.sync_state ?? (configured ? "unknown" : "not configured");
  const rootDisplay = registryRoot ? registryRoot.replace(/^\/Users\/[^/]+/, "~") : "—";

  const banner =
    push.error ?? pull.error ?? replay.error ??
    push.success ?? pull.success ?? replay.success ?? null;
  const bannerType = push.error || pull.error || replay.error ? "err" : banner ? "ok" : null;

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
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12, marginBottom: 18 }}>
          <Kpi label="Sync state" value={state} />
          <Kpi label="Ahead" value={remote?.ahead ?? 0} />
          <Kpi label="Behind" value={remote?.behind ?? 0} />
          <Kpi
            label="Pending writes"
            value={pendingCount}
            tone={pendingCount > 0 ? "pending" : undefined}
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
                ⚠ Local-only — no upstream tracking branch configured.
              </div>
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
              title={readOnly ? "registry offline" : `replay ${pendingCount} pending op${pendingCount === 1 ? "" : "s"}`}
            >
              <PlayIcon /> {replay.busy ? "replaying…" : `Replay pending (${pendingCount})`}
            </button>
            <button
              className="btn ghost"
              disabled={readOnly}
              onClick={onMutation}
              title="re-fetch remote status + pending writes"
            >
              <RefreshIcon /> Refresh
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

function Kpi({ label, value, tone }: { label: string; value: string | number; tone?: "pending" | "err" }) {
  const color = tone === "pending" ? "var(--pending)" : tone === "err" ? "var(--err)" : "var(--ink-0)";
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value" style={{ color }}>
        {value}
      </div>
    </div>
  );
}
