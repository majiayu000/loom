import type { RemotePayload, WorkspaceStatusPayload } from "../../types";
import { RefreshIcon, SyncIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface SyncPageProps {
  remote: RemotePayload | null;
  workspaceStatus: WorkspaceStatusPayload["data"] | null;
  workspaceWarnings: string[];
  onMutation: () => void;
  readOnly: boolean;
}

export function SyncPage({
  remote,
  workspaceStatus,
  workspaceWarnings,
  onMutation,
  readOnly,
}: SyncPageProps) {
  const sync = useMutation();
  const syncState = (remote?.sync_state ?? "unknown").toLowerCase().replace(/_/g, " ");
  const ahead = remote?.ahead ?? 0;
  const behind = remote?.behind ?? 0;
  const pendingOps = workspaceStatus?.pending_ops ?? remote?.pending_ops ?? 0;
  const remoteConfigured = Boolean(remote?.configured);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Git sync</h1>
          <div className="subtitle">
            Inspect registry remote state, then push, pull, or replay pending ops without leaving the panel.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn ghost"
            disabled={readOnly || sync.busy}
            onClick={() => sync.run("sync pull", api.syncPull, onMutation)}
            title={readOnly ? "registry offline" : undefined}
          >
            <SyncIcon /> Sync pull
          </button>
          <button
            className="btn ghost"
            disabled={readOnly || sync.busy}
            onClick={() => sync.run("sync push", api.syncPush, onMutation)}
            title={readOnly ? "registry offline" : undefined}
          >
            <SyncIcon /> Sync push
          </button>
          <button
            className="btn primary"
            disabled={readOnly || sync.busy}
            onClick={() => sync.run("sync replay", api.syncReplay, onMutation)}
            title={readOnly ? "registry offline" : undefined}
          >
            <RefreshIcon /> Replay pending
          </button>
        </div>
      </div>
      {(sync.error || sync.success || sync.busy) && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: sync.error ? "var(--err)" : sync.busy ? "var(--ink-2)" : "var(--ok)",
            background: sync.error
              ? "rgba(216,90,90,0.08)"
              : sync.busy
              ? "var(--bg-2)"
              : "rgba(111,183,138,0.08)",
          }}
        >
          {sync.busy ? "…" : sync.error ?? `✓ ${sync.success}`}
        </div>
      )}
      <div className="page-body">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, marginBottom: 16 }}>
          <Metric label="Sync state" value={syncState} meta={remoteConfigured ? "remote configured" : "no remote configured"} />
          <Metric label="Ahead / behind" value={`${ahead} / ${behind}`} meta="local branch relative to remote tracking ref" />
          <Metric label="Pending ops" value={`${pendingOps}`} meta="queued work that may require replay after pull" />
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "1.15fr 0.85fr", gap: 16 }}>
          <div className="card">
            <div className="card-head">
              <h3>Remote Summary</h3>
              <span className={`badge ${syncState === "clean" ? "ok" : ""}`}>{syncState}</span>
            </div>
            <div className="card-body">
              <div className="kv" style={{ gridTemplateColumns: "140px 1fr" }}>
                <div className="k">remote</div>
                <div className="v">{remote?.remote ?? "—"}</div>
                <div className="k">url</div>
                <div className="v mono">{remote?.url ?? "not configured"}</div>
                <div className="k">tracking ref</div>
                <div className="v">{remote?.tracking_ref ? "present" : "missing"}</div>
                <div className="k">registered targets</div>
                <div className="v">{workspaceStatus?.registered_targets?.count ?? 0}</div>
                <div className="k">source dirs</div>
                <div className="v">{workspaceStatus?.skill_sources?.count ?? 0}</div>
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-head">
              <h3>Operator Notes</h3>
            </div>
            <div className="card-body" style={{ display: "grid", gap: 10, fontSize: 12 }}>
              <InfoBlock
                title="Push"
                body="Publish your local registry history and current branch to the configured remote."
              />
              <InfoBlock
                title="Pull"
                body="Fetch remote updates, rebase local state, then replay pending ops if needed."
              />
              <InfoBlock
                title="Replay"
                body="Re-attempt pending operations without changing the remote state."
              />
              {workspaceWarnings.length > 0 && (
                <div style={warningStyle}>
                  {workspaceWarnings.map((warning, index) => (
                    <div key={`${warning}-${index}`}>{warning}</div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

function Metric({ label, value, meta }: { label: string; value: string; meta: string }) {
  return (
    <div className="card">
      <div className="card-body">
        <div style={labelStyle}>{label}</div>
        <div style={{ fontFamily: "var(--font-display)", fontSize: 24, color: "var(--ink-0)" }}>{value}</div>
        <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>{meta}</div>
      </div>
    </div>
  );
}

function InfoBlock({ title, body }: { title: string; body: string }) {
  return (
    <div style={{ border: "1px solid var(--line)", borderRadius: 10, padding: "10px 12px", background: "var(--bg-1)" }}>
      <div style={{ fontSize: 11, textTransform: "uppercase", letterSpacing: "0.08em", color: "var(--ink-3)", marginBottom: 4 }}>
        {title}
      </div>
      <div style={{ color: "var(--ink-1)" }}>{body}</div>
    </div>
  );
}

const labelStyle = {
  fontSize: 10.5,
  color: "var(--ink-3)",
  letterSpacing: "0.1em",
  textTransform: "uppercase" as const,
  fontWeight: 500,
};

const warningStyle = {
  padding: "10px 12px",
  borderRadius: 10,
  border: "1px solid rgba(216,90,90,0.25)",
  background: "rgba(216,90,90,0.08)",
  color: "var(--err)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
};
