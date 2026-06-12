import { useState } from "react";
import type { Op, OpStatus } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { MutationBanner } from "../../components/panel/MutationBanner";
import { RefreshIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import { COUNT_TERMS, filterLabel, formatReplayableWrites, summarizeOps } from "../../lib/count_labels";

type FilterKey = "all" | OpStatus;

interface OpsPageProps {
  ops: Op[];
  onMutation: () => void;
  readOnly: boolean;
}

export function OpsPage({ ops, onMutation, readOnly }: OpsPageProps) {
  const [filter, setFilter] = useState<FilterKey>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const retry = useMutation();
  const purge = useMutation();
  const filtered = prioritizeActionNeeded(filter === "all" ? ops : ops.filter((o) => o.status === filter));
  const selected = filtered.find((op) => op.id === selectedId) ?? null;
  const counts = summarizeOps(ops);
  const finalized = counts.ok + counts.err;
  const successRate = finalized > 0 ? (counts.ok / finalized) * 100 : null;
  const oldestPending = ops.find((o) => o.status === "pending");
  const oldestPendingLabel = oldestPending
    ? `${oldestPending.kind.toLowerCase() === "project" ? "apply" : oldestPending.kind.replace(/[._-]/g, " ")} ${
        oldestPending.skill
      } → ${oldestPending.target}`
    : "queue empty";
  const actionBusy = retry.busy || purge.busy;
  const bannerError = retry.error ?? purge.error;
  const bannerSuccess = retry.success ?? purge.success;

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Activity</h1>
          <div className="subtitle">
            Recent registry writes, projection checks, and queued sync work. Replayable rows can be retried or cleared here.
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
                ? "no replayable writes to retry"
                : "retry replayable writes against local targets"
            }
          >
            <RefreshIcon /> {retry.busy ? "Retrying…" : `Retry replayable (${counts.pending})`}
          </button>
          <button
            className="btn ghost"
            disabled={readOnly || actionBusy || counts.pending === 0}
            onClick={() => purge.run("clear pending", api.opsPurge, onMutation)}
            title={
              readOnly
                ? "registry offline"
                : counts.pending === 0
                ? "no replayable writes to clear"
                : "remove replayable writes from the local queue"
            }
          >
            {purge.busy ? "Clearing…" : "Clear replayable"}
          </button>
        </div>
      </div>
      <MutationBanner
        message={actionBusy ? "…" : undefined}
        error={bannerError}
        success={bannerSuccess}
        tone={actionBusy ? "muted" : undefined}
        variant="bar"
      />
      <div className="page-body">
        <div className="ops-summary-grid">
          <div
            className="card"
            title="Visible Activity rows. Queued writes come from /api/v1/ops/pending; audit rows come from /api/v1/ops."
          >
            <div className="card-body">
              <div style={section_label}>{COUNT_TERMS.activityRows}</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24 }}>{counts.all}</div>
              <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>
                {counts.ok} done · {counts.err} failed · {formatReplayableWrites(counts.pending)}
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
              <div style={section_label}>{COUNT_TERMS.replayableWrites}</div>
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
              {filterLabel(k)}{" "}
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
            filtered.map((o) => (
              <div
                key={o.id}
                role="button"
                tabIndex={0}
                onClick={() => setSelectedId(o.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") setSelectedId(o.id);
                }}
                style={{ cursor: "pointer" }}
              >
                <OpRow op={o} />
              </div>
            ))
          )}
        </div>
        {selected && <ActivityDetail op={selected} onClose={() => setSelectedId(null)} />}
      </div>
    </>
  );
}

function prioritizeActionNeeded(ops: Op[]): Op[] {
  return [...ops].sort((a, b) => statusPriority(a.status) - statusPriority(b.status));
}

function statusPriority(status: OpStatus): number {
  if (status === "pending") return 0;
  if (status === "err") return 1;
  return 2;
}

function ActivityDetail({ op, onClose }: { op: Op; onClose: () => void }) {
  return (
    <div className="card" style={{ marginTop: 16 }}>
      <div className="card-head">
        <h3>Activity detail</h3>
        <button className="btn sm" onClick={onClose}>Close</button>
      </div>
      <div className="card-body">
        <div className="kv" style={{ margin: 0 }}>
          <div className="k">Raw id</div>
          <div className="v mono">{op.id}</div>
          <div className="k">Command</div>
          <div className="v mono">{op.kind}</div>
          <div className="k">Skill</div>
          <div className="v mono">{op.skill}</div>
          <div className="k">Target</div>
          <div className="v mono">{op.target}</div>
          <div className="k">Method</div>
          <div className="v mono">{op.method}</div>
          <div className="k">Reason</div>
          <div className="v">{op.reason ?? "—"}</div>
        </div>
      </div>
    </div>
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
