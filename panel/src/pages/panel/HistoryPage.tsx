import { useEffect, useMemo, useState } from "react";
import { api, ApiError, type OpsPayload, type V3OperationRecord } from "../../lib/api/client";

type FilterKey = "all" | "pending" | "ok" | "err";

type LoadState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: NonNullable<OpsPayload["data"]> }
  | { kind: "error"; message: string };

interface HistoryPageProps {
  live: boolean;
}

const REFRESH_INTERVAL_MS = 3000;

export function HistoryPage({ live }: HistoryPageProps) {
  const [state, setState] = useState<LoadState>({ kind: "idle" });
  const [filter, setFilter] = useState<FilterKey>("all");
  const [query, setQuery] = useState("");
  const [refreshTick, setRefreshTick] = useState(0);

  useEffect(() => {
    if (!live) return;
    const id = setInterval(() => setRefreshTick((t) => t + 1), REFRESH_INTERVAL_MS);
    return () => clearInterval(id);
  }, [live]);

  useEffect(() => {
    const controller = new AbortController();
    setState((prev) => (prev.kind === "idle" ? { kind: "loading" } : prev));
    api
      .ops(controller.signal)
      .then((res) => {
        if (controller.signal.aborted) return;
        if (!res.ok || !res.data) {
          setState({ kind: "error", message: res.error?.message ?? "ops fetch returned ok=false" });
          return;
        }
        setState({ kind: "ready", payload: res.data });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setState({ kind: "error", message });
      });
    return () => controller.abort();
  }, [live, refreshTick]);

  const operations = state.kind === "ready" ? state.payload.operations : [];
  const ordered = useMemo(
    () =>
      [...operations].sort((a, b) =>
        (b.created_at ?? "").localeCompare(a.created_at ?? ""),
      ),
    [operations],
  );

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return ordered.filter((op) => {
      if (filter !== "all" && bucket(op) !== filter) return false;
      if (!needle) return true;
      return (
        op.op_id.toLowerCase().includes(needle) ||
        op.intent.toLowerCase().includes(needle) ||
        (op.last_error?.message ?? "").toLowerCase().includes(needle)
      );
    });
  }, [ordered, filter, query]);

  const counts = useMemo(() => {
    const c = { all: ordered.length, pending: 0, ok: 0, err: 0 };
    for (const op of ordered) {
      const b = bucket(op);
      if (b === "pending") c.pending += 1;
      else if (b === "ok") c.ok += 1;
      else if (b === "err") c.err += 1;
    }
    return c;
  }, [ordered]);

  const checkpoint = state.kind === "ready" ? state.payload.checkpoint : undefined;

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Ops history</h1>
          <div className="subtitle">
            Every mutation Loom has applied to the registry, oldest writes first on disk. Pending ops also surface in the
            Ops tab; failed ops point to a replay with <span className="mono">loom sync replay</span>.
          </div>
        </div>
        <div className="header-actions">
          <div className="searchbar" style={{ width: 260 }}>
            <input
              placeholder="Filter by id / intent / error…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>
      </div>
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
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12, marginBottom: 18 }}>
          <Kpi label="Tracked ops" value={counts.all} />
          <Kpi label="Pending" value={counts.pending} tone={counts.pending > 0 ? "pending" : undefined} />
          <Kpi label="Succeeded" value={counts.ok} />
          <Kpi label="Failed" value={counts.err} tone={counts.err > 0 ? "err" : undefined} />
        </div>

        <div style={{ display: "flex", gap: 4, marginBottom: 12 }}>
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
              {k === "err" ? "failed" : k}{" "}
              <span className="mono" style={{ color: "var(--ink-3)", marginLeft: 4 }}>
                {counts[k]}
              </span>
            </button>
          ))}
        </div>

        <div
          style={{
            background: "var(--bg-1)",
            borderRadius: 10,
            overflow: "hidden",
            border: "1px solid var(--line)",
          }}
        >
          <table className="tbl">
            <thead>
              <tr>
                <th>Op id</th>
                <th>Intent</th>
                <th>Status</th>
                <th>ack</th>
                <th>Created</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {state.kind === "loading" && (
                <tr>
                  <td colSpan={6} className="mono" style={{ textAlign: "center", color: "var(--ink-3)", padding: 18 }}>
                    loading…
                  </td>
                </tr>
              )}
              {state.kind === "ready" && filtered.length === 0 && (
                <tr>
                  <td colSpan={6} style={{ textAlign: "center", color: "var(--ink-3)", padding: 18 }}>
                    {counts.all === 0
                      ? "No operations recorded yet — every mutation via CLI or Panel will show up here."
                      : "No operations match the current filter."}
                  </td>
                </tr>
              )}
              {filtered.map((op) => (
                <OpHistoryRow key={op.op_id} op={op} />
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

function bucket(op: V3OperationRecord): "pending" | "ok" | "err" {
  if (op.last_error) return "err";
  const s = op.status.toLowerCase();
  if (s === "pending" || s === "enqueued" || s === "in_flight" || s === "retrying") return "pending";
  if (s === "ok" || s === "applied" || s === "completed" || s === "done") return "ok";
  if (s === "err" || s === "error" || s === "failed") return "err";
  return op.ack ? "ok" : "pending";
}

function OpHistoryRow({ op }: { op: V3OperationRecord }) {
  const kind = bucket(op);
  const color = kind === "err" ? "var(--err)" : kind === "pending" ? "var(--pending)" : "var(--ok)";
  return (
    <tr>
      <td className="mono dim">{op.op_id}</td>
      <td className="name">{op.intent}</td>
      <td>
        <span className="chip" style={{ color }}>
          {op.last_error ? op.last_error.code : op.status}
        </span>
      </td>
      <td className="mono dim">{op.ack ? "✓" : "—"}</td>
      <td className="mono dim" style={{ fontSize: 10.5 }}>
        {op.created_at}
      </td>
      <td className="mono dim" style={{ fontSize: 10.5 }}>
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
