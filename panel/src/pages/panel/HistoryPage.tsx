import { useEffect, useMemo, useState } from "react";
import type { PanelDataMode } from "../../lib/api/usePanelData";
import { api, ApiError, type OpsHistoryDiagnosePayload, type OpsPayload, type RegistryOperationRecord } from "../../lib/api/client";
import {
  bucketRegistryOperation,
  describeRegistryOperation,
  registryOperationDisplayId,
} from "../../lib/operation_labels";
import { useMutation } from "../../lib/useMutation";
import { COUNT_TERMS, filterLabel, summarizeOps } from "../../lib/count_labels";
import { SearchIcon } from "../../components/icons/nav_icons";

type FilterKey = "all" | "pending" | "ok" | "err";
type DiagnoseData = NonNullable<OpsHistoryDiagnosePayload["data"]>;
const HISTORY_PAGE_SIZE = 100;

type LoadState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: NonNullable<OpsPayload["data"]> }
  | { kind: "error"; message: string };

interface HistoryPageProps {
  live: boolean;
  mode: PanelDataMode;
  mutationVersion: number;
  refreshKey?: string | null;
  onMutation?: () => void;
}

export function HistoryPage({ live, mode, mutationVersion, refreshKey, onMutation }: HistoryPageProps) {
  const [state, setState] = useState<LoadState>({ kind: "idle" });
  const [diagnose, setDiagnose] = useState<DiagnoseData | null>(null);
  const [diagnoseError, setDiagnoseError] = useState<string | null>(null);
  const [filter, setFilter] = useState<FilterKey>("all");
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [repairVersion, setRepairVersion] = useState(0);
  const repair = useMutation();

  useEffect(() => {
    if (!live) {
      setState({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ kind: "loading" });
    setDiagnose(null);
    setDiagnoseError(null);
    Promise.allSettled([
      api.ops({ limit: HISTORY_PAGE_SIZE, offset }, controller.signal),
      api.opsHistoryDiagnose(controller.signal),
    ]).then(([opsResult, diagnoseResult]) => {
      if (controller.signal.aborted) return;

      if (opsResult.status === "rejected") {
        const err = opsResult.reason;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setState({ kind: "error", message });
        return;
      }

      const res = opsResult.value;
      if (!res.ok || !res.data) {
        setState({ kind: "error", message: res.error?.message ?? "activity fetch returned ok=false" });
        return;
      }
      setState({ kind: "ready", payload: res.data });

      if (diagnoseResult.status === "fulfilled") {
        if (diagnoseResult.value.data) {
          setDiagnose(diagnoseResult.value.data);
        } else if (!diagnoseResult.value.ok) {
          setDiagnoseError(diagnoseResult.value.error?.message ?? "history diagnose returned ok=false");
        }
      } else {
        const err = diagnoseResult.reason;
        setDiagnoseError(err instanceof Error ? err.message : String(err));
      }
    });
    return () => controller.abort();
  }, [live, mutationVersion, refreshKey, offset, repairVersion]);

  const runRepair = (strategy: "local" | "remote") => {
    repair.run(`history repair ${strategy}`, () => api.opsHistoryRepair({ strategy }), () => {
      setRepairVersion((value) => value + 1);
      onMutation?.();
    });
  };

  const offlineHint =
    mode === "offline-stale"
      ? "Activity history is offline. Showing the last overview snapshot."
      : "Activity history needs the live panel API. Start `loom panel`.";

  const payload = state.kind === "ready" ? state.payload : null;
  const operations = payload?.operations ?? [];

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return operations.filter((op) => {
      if (filter !== "all" && bucket(op) !== filter) return false;
      if (!needle) return true;
      return (
        registryOperationDisplayId(op).toLowerCase().includes(needle) ||
        describeRegistryOperation(op).title.toLowerCase().includes(needle) ||
        op.intent.toLowerCase().includes(needle) ||
        (op.last_error?.message ?? "").toLowerCase().includes(needle)
      );
    });
  }, [operations, filter, query]);

  const counts = useMemo(
    () => summarizeOps(operations.map((op) => ({ status: bucket(op) }))),
    [operations],
  );

  const checkpoint = payload?.checkpoint;
  const historySummary =
    payload && payload.count > payload.loaded_count
      ? `Showing ${payload.loaded_count} of ${payload.count} audit changes.`
      : payload
      ? `${payload.loaded_count} loaded audit change${payload.loaded_count === 1 ? "" : "s"}.`
      : null;

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Activity history</h1>
          <div className="subtitle">
            Every audit change Loom has recorded. Replayable writes also appear in Activity; failed work points to a replay with{" "}
            <span className="mono">loom sync replay</span>.
          </div>
        </div>
        <div className="header-actions">
          <div className="searchbar" style={{ width: 260 }}>
            <SearchIcon />
            <input
              placeholder="Filter by id / intent / error…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>
      </div>
      {(repair.error || repair.success) && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: repair.error ? "var(--err)" : "var(--ok)",
            background: repair.error ? "rgba(216,90,90,0.08)" : "rgba(111,183,138,0.08)",
          }}
        >
          {repair.error ?? `✓ ${repair.success}`}
        </div>
      )}
      <div className="page-body">
        {state.kind === "error" && (
          <div
            style={{
              padding: "6px 28px",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              borderBottom: "1px solid var(--line)",
              color: "var(--err)",
              background: "rgba(216,90,90,0.08)",
            }}
          >
            {state.message}
          </div>
        )}
        {!live && <div className="empty" style={{ marginBottom: 18 }}>{offlineHint}</div>}
        {diagnoseError && (
          <div className="mono" style={{ color: "var(--warn)", fontSize: 11, marginBottom: 12 }}>
            History diagnose: {diagnoseError}
          </div>
        )}
        {diagnose?.local_branch && (
          <div style={{ display: "flex", gap: 12, padding: "4px 0", marginBottom: 12, fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--ink-3)", alignItems: "center", flexWrap: "wrap" }}>
            <span>{diagnose.local_segments} segment{diagnose.local_segments === 1 ? "" : "s"}</span>
            {diagnose.remote_tracking && diagnose.ahead > 0 && <span style={{ color: "var(--ok)" }}>↑ {diagnose.ahead} ahead</span>}
            {diagnose.remote_tracking && diagnose.behind > 0 && <span style={{ color: "var(--pending)" }}>↓ {diagnose.behind} behind</span>}
            {diagnose.remote_tracking && diagnose.ahead === 0 && diagnose.behind === 0 && <span>in sync</span>}
            {diagnose.conflicts.length > 0 && (
              <>
                <span style={{ color: "var(--err)" }}>
                  {diagnose.conflicts.length} conflict{diagnose.conflicts.length === 1 ? "" : "s"}
                </span>
                <span style={{ color: "var(--ink-2)" }}>{diagnose.conflicts[0]?.path}</span>
                <button className="btn sm" onClick={() => runRepair("local")} disabled={repair.busy}>
                  Repair from local
                </button>
                <button className="btn sm" onClick={() => runRepair("remote")} disabled={repair.busy}>
                  Repair from remote
                </button>
              </>
            )}
          </div>
        )}
        <div className="kpi-row">
          <Kpi label={COUNT_TERMS.loadedAuditChanges} value={payload?.loaded_count ?? counts.all} />
          <Kpi label={COUNT_TERMS.replayableWrites} value={counts.pending} tone={counts.pending > 0 ? "pending" : undefined} />
          <Kpi label={COUNT_TERMS.succeeded} value={counts.ok} />
          <Kpi label={COUNT_TERMS.failed} value={counts.err} tone={counts.err > 0 ? "err" : undefined} />
        </div>

        <div style={{ display: "flex", gap: 12, marginBottom: 12, justifyContent: "space-between", flexWrap: "wrap" }}>
          <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
            {(["all", "pending", "ok", "err"] as FilterKey[]).map((k) => (
              <button
                key={k}
                className="btn sm"
                onClick={() => setFilter(k)}
                style={{
                  background: filter === k ? "var(--bg-2)" : "transparent",
                  borderColor: filter === k ? "var(--line-hi)" : "transparent",
                  border: "1px solid",
                  color: filter === k ? "var(--ink-0)" : "var(--ink-2)",
                }}
              >
                {filterLabel(k)}{" "}
                <span className="mono" style={{ color: "var(--ink-3)", marginLeft: 4 }}>
                  {counts[k]}
                </span>
              </button>
            ))}
          </div>
          {payload && (
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              {historySummary && (
                <span className="mono" style={{ fontSize: 11, color: "var(--ink-3)" }}>
                  {historySummary}
                </span>
              )}
              <button className="btn sm" onClick={() => setOffset((cur) => Math.max(0, cur - payload.limit))} disabled={payload.offset === 0}>
                newer
              </button>
              <button className="btn sm" onClick={() => setOffset((cur) => cur + payload.limit)} disabled={!payload.has_more}>
                older
              </button>
            </div>
          )}
        </div>

        <div
          className="history-table-wrap"
          style={{
            background: "var(--bg-1)",
            borderRadius: 10,
            border: "1px solid var(--line)",
          }}
        >
          <table className="tbl history-table mobile-cards">
            <thead>
              <tr>
                <th>Action</th>
                <th>Details</th>
                <th>Status</th>
                <th>Created</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {state.kind === "loading" && (
                <tr>
                  <td
                    colSpan={5}
                    className="mono table-empty-cell"
                    style={{ textAlign: "center", color: "var(--ink-3)", padding: 18 }}
                  >
                    loading…
                  </td>
                </tr>
              )}
              {state.kind === "ready" && filtered.length === 0 && (
                <tr>
                  <td
                    colSpan={5}
                    className="table-empty-cell"
                    style={{ textAlign: "center", color: "var(--ink-3)", padding: 18 }}
                  >
                    {operations.length === 0
                      ? "No activity recorded yet — every CLI or Panel change will show up here."
                      : "No activity matches the current filter."}
                  </td>
                </tr>
              )}
              {filtered.map((op) => (
                <OpHistoryRow key={registryOperationDisplayId(op)} op={op} />
              ))}
            </tbody>
          </table>
        </div>

        {checkpoint && (
          <div style={{ marginTop: 12, fontSize: 11, color: "var(--ink-3)" }}>
            Checkpoint: last scanned{" "}
            <span className="mono" style={{ color: "var(--ink-1)" }}>
              {checkpoint.last_scanned_op_id ?? "—"}
            </span>
            {checkpoint.last_acked_op_id && (
              <>
                {" · "}last acked{" "}
                <span className="mono" style={{ color: "var(--ink-1)" }}>
                  {checkpoint.last_acked_op_id}
                </span>
              </>
            )}
            {checkpoint.updated_at && (
              <>
                {" · updated "}
                <span className="mono">{checkpoint.updated_at}</span>
              </>
            )}
          </div>
        )}
      </div>
    </>
  );
}

export function bucket(op: RegistryOperationRecord): "pending" | "ok" | "err" {
  return bucketRegistryOperation(op);
}

function OpHistoryRow({ op }: { op: RegistryOperationRecord }) {
  const kind = bucket(op);
  const color = kind === "err" ? "var(--err)" : kind === "pending" ? "var(--pending)" : "var(--ok)";
  const label = describeRegistryOperation(op);
  return (
    <tr>
      <td className="name" data-label="Action">
        <div>{label.title}</div>
        {op.last_error && (
          <div className="mono" style={{ color: "var(--err)", fontSize: 10.5, marginTop: 3 }}>
            {op.last_error.message}
          </div>
        )}
      </td>
      <td data-label="Details">
        <div className="op-detail-stack">
          {label.details.map((detail) => (
            <span key={detail} className="op-meta" title={detail.startsWith("id ") ? label.technicalId : undefined}>
              {detail}
            </span>
          ))}
        </div>
      </td>
      <td data-label="Status">
        <span className="chip" style={{ color }}>
          {op.last_error ? op.last_error.code : op.status}
        </span>
      </td>
      <td className="mono dim" data-label="Created" style={{ fontSize: 10.5 }}>
        {op.created_at}
      </td>
      <td className="mono dim mobile-hide" data-label="Updated" style={{ fontSize: 10.5 }}>
        {op.updated_at}
      </td>
    </tr>
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
