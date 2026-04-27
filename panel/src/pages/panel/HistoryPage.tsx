import { useEffect, useState } from "react";
import type { HistoryEntryPayload } from "../../types";
import { api } from "../../lib/api/client";

interface HistoryPageProps {
  readOnly: boolean;
}

interface DiagnoseSummary {
  ahead?: number;
  behind?: number;
  local_segments?: number;
  local_archives?: number;
  remote_segments?: number;
  remote_archives?: number;
  conflicts?: Array<{ path?: string; scope?: string }>;
}

function relativeTime(iso?: string | null) {
  if (!iso) return "—";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const ms = Date.now() - then;
  if (ms < 0) return "now";
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function HistoryPage({ readOnly }: HistoryPageProps) {
  const [entries, setEntries] = useState<HistoryEntryPayload[]>([]);
  const [diagnose, setDiagnose] = useState<DiagnoseSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    Promise.allSettled([api.opsHistoryList(ctrl.signal), api.opsHistoryDiagnose(ctrl.signal)])
      .then(([historyList, diagnoseEnvelope]) => {
        if (ctrl.signal.aborted) return;
        if (historyList.status === "rejected") {
          setEntries([]);
          setDiagnose(null);
          setError(errorMessage(historyList.reason));
          setLoading(false);
          return;
        }

        setEntries(historyList.value.data?.entries ?? []);
        if (diagnoseEnvelope.status === "fulfilled") {
          setDiagnose((diagnoseEnvelope.value.data ?? {}) as DiagnoseSummary);
        } else {
          setDiagnose(null);
        }
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (!(err instanceof Error && err.name === "AbortError")) {
          setError(errorMessage(err));
          setLoading(false);
        }
      });
    return () => ctrl.abort();
  }, []);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Ops history</h1>
          <div className="subtitle">
            Read-only view of the local <span className="mono">loom-history</span> branch: retained segments, archives, and current divergence state.
          </div>
        </div>
      </div>
      <div className="page-body">
        {loading && <div className="empty" style={{ padding: "40px 20px" }}>Loading history…</div>}
        {error && (
          <div
            style={{
              padding: "10px 12px",
              borderRadius: 10,
              border: "1px solid rgba(216,90,90,0.25)",
              background: "rgba(216,90,90,0.08)",
              color: "var(--err)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
            }}
          >
            {error}
          </div>
        )}
        {!loading && !error && (
          <>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12, marginBottom: 16 }}>
              <Metric label="entries" value={`${entries.length}`} meta="archives + active segments in local history branch" />
              <Metric
                label="ahead / behind"
                value={`${diagnose?.ahead ?? 0} / ${diagnose?.behind ?? 0}`}
                meta="relative to origin/loom-history"
              />
              <Metric
                label="segments / archives"
                value={`${diagnose?.local_segments ?? 0} / ${diagnose?.local_archives ?? 0}`}
                meta="local retained history units"
              />
              <Metric
                label="conflicts"
                value={`${diagnose?.conflicts?.length ?? 0}`}
                meta={readOnly ? "read-only UI mode" : "repair from Ops or Sync pages"}
              />
            </div>

            <div className="card">
              <div className="card-head">
                <h3>History Entries</h3>
                <span className={`badge ${(diagnose?.conflicts?.length ?? 0) === 0 ? "ok" : ""}`}>
                  {(diagnose?.conflicts?.length ?? 0) === 0 ? "clean" : "attention"}
                </span>
              </div>
              <table className="tbl">
                <thead>
                  <tr>
                    <th>Scope</th>
                    <th>Path</th>
                    <th>Blob</th>
                    <th>Lines</th>
                    <th>First seen</th>
                    <th>Last seen</th>
                  </tr>
                </thead>
                <tbody>
                  {entries.map((entry) => (
                    <tr key={`${entry.scope}:${entry.path}`}>
                      <td><span className="chip">{entry.scope ?? "—"}</span></td>
                      <td className="mono">{entry.path ?? "—"}</td>
                      <td className="mono">{entry.blob ?? "—"}</td>
                      <td>{entry.line_count ?? 0}</td>
                      <td>{relativeTime(entry.first_at)}</td>
                      <td>{relativeTime(entry.last_at)}</td>
                    </tr>
                  ))}
                  {entries.length === 0 && (
                    <tr>
                      <td colSpan={6} className="empty" style={{ padding: "28px 16px" }}>
                        No local loom-history entries yet.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </>
        )}
      </div>
    </>
  );
}

function Metric({ label, value, meta }: { label: string; value: string; meta: string }) {
  return (
    <div className="card">
      <div className="card-body">
        <div style={labelStyle}>{label}</div>
        <div style={{ fontFamily: "var(--font-display)", fontSize: 24 }}>{value}</div>
        <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>{meta}</div>
      </div>
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
