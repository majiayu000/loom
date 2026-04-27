import { useState } from "react";
import type { Op, OpStatus } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { RefreshIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

type FilterKey = "all" | OpStatus;

interface OpsPageProps {
  ops: Op[];
  onMutation: () => void;
  readOnly: boolean;
}

export function OpsPage({ ops, onMutation, readOnly }: OpsPageProps) {
  const [filter, setFilter] = useState<FilterKey>("all");
  const retry = useMutation();
  const purge = useMutation();
  const filtered = filter === "all" ? ops : ops.filter((o) => o.status === filter);
  const counts = {
    all: ops.length,
    pending: ops.filter((o) => o.status === "pending").length,
    ok: ops.filter((o) => o.status === "ok").length,
    err: ops.filter((o) => o.status === "err").length,
  };
  const finalized = counts.ok + counts.err;
  const successRate = finalized > 0 ? (counts.ok / finalized) * 100 : null;
  const oldestPending = ops.find((o) => o.status === "pending");
  const oldestPendingLabel = oldestPending
    ? `${oldestPending.kind.toLowerCase() === "project" ? "apply" : oldestPending.kind.replace(/[._-]/g, " ")} ${
        oldestPending.skill
      } → ${oldestPending.target}`
    : "queue empty";
  const actionBusy = retry.busy || purge.busy;

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Activity</h1>
          <div className="subtitle">
            Recent registry writes, projection checks, and queued sync work. Pending rows can be retried or cleared here.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn ghost"
            disabled={readOnly || actionBusy || counts.pending === 0}
            onClick={() => retry.run("retry pending", api.opsRetry, onMutation)}
            title={
              readOnly
                ? "registry offline"
                : counts.pending === 0
                ? "no pending writes to retry"
                : "retry pending writes against local targets"
            }
          >
            <RefreshIcon /> {retry.busy ? "Retrying…" : `Retry pending (${counts.pending})`}
          </button>
          <button
            className="btn ghost"
            disabled={readOnly || actionBusy || counts.pending === 0}
            onClick={() => purge.run("clear pending", api.opsPurge, onMutation)}
            title={
              readOnly
                ? "registry offline"
                : counts.pending === 0
                ? "pending queue is already empty"
                : "remove pending writes from the local queue"
            }
          >
            {purge.busy ? "Clearing…" : "Clear pending"}
          </button>
        </div>
      </div>
      {(retry.error || retry.success || retry.busy || purge.error || purge.success || purge.busy) && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: retry.error || purge.error ? "var(--err)" : retry.busy || purge.busy ? "var(--ink-2)" : "var(--ok)",
            background:
              retry.error || purge.error
                ? "rgba(216,90,90,0.08)"
                : retry.busy || purge.busy
                ? "var(--bg-2)"
                : "rgba(111,183,138,0.08)",
          }}
        >
          {retry.busy || purge.busy ? "…" : retry.error ?? purge.error ?? `✓ ${retry.success ?? purge.success}`}
        </div>
      )}
      <div className="page-body">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, marginBottom: 18 }}>
          <div className="card">
            <div className="card-body">
              <div style={section_label}>Tracked changes</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24 }}>{counts.all}</div>
              <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>
                {counts.ok} done · {counts.err} failed · {counts.pending} pending
              </div>
            </div>
          </div>
          <div className="card">
            <div className="card-body">
              <div style={section_label}>Success rate</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24, color: successRate === null ? "var(--ink-3)" : "var(--ok)" }}>
                {successRate === null ? "—" : `${successRate.toFixed(1)}%`}
              </div>
              <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>
                {finalized === 0 ? "no completed changes yet" : `${counts.ok} / ${finalized} done`}
              </div>
            </div>
          </div>
          <div className="card">
            <div className="card-body">
              <div style={section_label}>Pending</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24, color: "var(--pending)" }}>
                {counts.pending}
              </div>
              <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>
                {oldestPendingLabel}
              </div>
            </div>
          </div>
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
              {k === "err" ? "failed" : k === "ok" ? "done" : k}{" "}
              <span className="mono" style={{ color: "var(--ink-3)", marginLeft: 4 }}>
                {counts[k]}
              </span>
            </button>
          ))}
        </div>

        <div>
          {filtered.length === 0 ? (
            <div className="empty">
              {ops.length === 0 ? "No activity yet." : "No activity matches the current filter."}
            </div>
          ) : (
            filtered.map((o) => <OpRow key={o.id} op={o} />)
          )}
        </div>
      </div>
    </>
  );
}

const section_label = {
  fontSize: 10.5,
  color: "var(--ink-3)",
  letterSpacing: "0.1em",
  textTransform: "uppercase" as const,
  fontWeight: 500,
  marginBottom: 8,
};
