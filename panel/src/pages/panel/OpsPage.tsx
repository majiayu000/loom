import { useState } from "react";
import type { Op, OpStatus } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { RefreshIcon } from "../../components/icons/nav_icons";

type FilterKey = "all" | OpStatus;

export function OpsPage({ ops }: { ops: Op[] }) {
  const [filter, setFilter] = useState<FilterKey>("all");
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

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Ops</h1>
          <div className="subtitle">Every state change is an op. Retry the pending, repair the failed, diagnose the rest.</div>
        </div>
        <div className="header-actions">
          <button className="btn ghost" disabled title="Coming in v1.0 — use `loom ops retry` via CLI">
            <RefreshIcon /> Retry failed
          </button>
          <button className="btn ghost" disabled title="Coming in v1.0 — use `loom ops purge` via CLI">
            Purge completed
          </button>
        </div>
      </div>
      <div className="page-body">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, marginBottom: 18 }}>
          <div className="card">
            <div className="card-body">
              <div style={section_label}>Tracked ops</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24 }}>{counts.all}</div>
              <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>
                {counts.ok} ok · {counts.err} failed · {counts.pending} pending
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
                {finalized === 0 ? "no finalized ops yet" : `${counts.ok} / ${finalized} clean`}
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
                {oldestPending ? `${oldestPending.kind} ${oldestPending.skill} → ${oldestPending.target}` : "queue empty"}
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
              {k === "err" ? "failed" : k}{" "}
              <span className="mono" style={{ color: "var(--ink-3)", marginLeft: 4 }}>
                {counts[k]}
              </span>
            </button>
          ))}
        </div>

        <div>
          {filtered.map((o) => (
            <OpRow key={o.id} op={o} />
          ))}
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
