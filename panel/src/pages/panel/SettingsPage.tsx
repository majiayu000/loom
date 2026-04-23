import type { InfoPayload, WorkspaceStatusPayload } from "../../types";

interface SettingsPageProps {
  info: InfoPayload | null;
  workspaceStatus: WorkspaceStatusPayload["data"] | null;
  workspaceWarnings: string[];
}

function compactHome(path: string | undefined) {
  if (!path) return "—";
  return path.replace(/^\/Users\/[^/]+/, "~");
}

export function SettingsPage({ info, workspaceStatus, workspaceWarnings }: SettingsPageProps) {
  const v3Available = workspaceStatus?.v3?.available ?? false;
  const remote = workspaceStatus?.remote;

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Settings</h1>
          <div className="subtitle">
            Current registry wiring, agent defaults, and backend status exposed from the live workspace.
          </div>
        </div>
      </div>
      <div className="page-body">
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          <div className="card">
            <div className="card-head">
              <h3>Workspace</h3>
              <span className={`badge ${v3Available ? "ok" : ""}`}>{v3Available ? "v3 ready" : "v3 missing"}</span>
            </div>
            <div className="card-body">
              <div className="kv" style={{ gridTemplateColumns: "140px 1fr" }}>
                <div className="k">registry root</div>
                <div className="v mono">{compactHome(info?.root)}</div>
                <div className="k">state dir</div>
                <div className="v mono">{compactHome(info?.state_dir)}</div>
                <div className="k">backup dir</div>
                <div className="v mono">{compactHome(workspaceStatus?.backup_dir)}</div>
                <div className="k">v3 targets file</div>
                <div className="v mono">{compactHome(info?.v3_targets_file)}</div>
                <div className="k">registered targets</div>
                <div className="v">{workspaceStatus?.registered_targets?.count ?? 0}</div>
                <div className="k">source dirs</div>
                <div className="v">{workspaceStatus?.skill_sources?.count ?? 0}</div>
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-head">
              <h3>Agent Defaults</h3>
            </div>
            <div className="card-body">
              <div className="kv" style={{ gridTemplateColumns: "140px 1fr" }}>
                <div className="k">Claude</div>
                <div className="v mono">{compactHome(info?.claude_dir ?? workspaceStatus?.agent_dir_defaults?.claude_dir)}</div>
                <div className="k">Codex</div>
                <div className="v mono">{compactHome(info?.codex_dir ?? workspaceStatus?.agent_dir_defaults?.codex_dir)}</div>
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-head">
              <h3>Git</h3>
            </div>
            <div className="card-body">
              <div className="kv" style={{ gridTemplateColumns: "140px 1fr" }}>
                <div className="k">branch</div>
                <div className="v">{workspaceStatus?.git?.branch ?? "—"}</div>
                <div className="k">head</div>
                <div className="v mono">{workspaceStatus?.git?.head ?? "—"}</div>
                <div className="k">status</div>
                <div className="v mono">{workspaceStatus?.git?.status_short?.trim() || "clean"}</div>
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-head">
              <h3>Remote</h3>
            </div>
            <div className="card-body">
              <div className="kv" style={{ gridTemplateColumns: "140px 1fr" }}>
                <div className="k">remote</div>
                <div className="v">{remote?.remote ?? "—"}</div>
                <div className="k">url</div>
                <div className="v mono">{remote?.url ?? info?.remote_url ?? "not configured"}</div>
                <div className="k">sync state</div>
                <div className="v">{remote?.sync_state ?? "unknown"}</div>
                <div className="k">ahead / behind</div>
                <div className="v">{remote ? `${remote.ahead ?? 0} / ${remote.behind ?? 0}` : "0 / 0"}</div>
                <div className="k">pending ops</div>
                <div className="v">{workspaceStatus?.pending_ops ?? remote?.pending_ops ?? 0}</div>
              </div>
            </div>
          </div>
        </div>

        {workspaceWarnings.length > 0 && (
          <div className="card" style={{ marginTop: 16 }}>
            <div className="card-head">
              <h3>Warnings</h3>
            </div>
            <div className="card-body" style={{ display: "grid", gap: 8 }}>
              {workspaceWarnings.map((warning, index) => (
                <div
                  key={`${warning}-${index}`}
                  style={{
                    padding: "8px 10px",
                    borderRadius: 10,
                    border: "1px solid rgba(216,90,90,0.25)",
                    background: "rgba(216,90,90,0.08)",
                    color: "var(--err)",
                    fontFamily: "var(--font-mono)",
                    fontSize: 11,
                  }}
                >
                  {warning}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </>
  );
}
