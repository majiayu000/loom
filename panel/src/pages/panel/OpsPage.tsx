import { useState } from "react";
import type { Op, OpStatus } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { RefreshIcon } from "../../components/icons/nav_icons";

const SPARK = [3, 5, 2, 6, 4, 7, 9, 5, 8, 6, 4, 3, 5, 7, 11, 6, 4, 5, 8, 9, 6, 4, 3, 5];
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

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Ops</h1>
          <div className="subtitle">Every state change is an op. Retry the pending, repair the failed, diagnose the rest.</div>
        </div>
        <div className="header-actions">
          <button className="btn ghost">
            <RefreshIcon /> Retry failed
          </button>
          <button className="btn ghost">Purge completed</button>
        </div>
      </div>
      <div className="page-body">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, marginBottom: 18 }}>
          <div className="card">
            <div className="card-body">
              <div style={section_label}>Last 24h</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24 }}>47 ops</div>
              <div className="spark" style={{ marginTop: 10 }}>
                {SPARK.map((v, i) => (
                  <div key={i} className={`bar ${v > 7 ? "hi" : ""}`} style={{ height: `${v * 2.4}px` }} />
                ))}
              </div>
            </div>
          </div>
          <div className="card">
            <div className="card-body">
              <div style={section_label}>Success rate</div>
              <div style={{ fontFamily: "var(--font-display)", fontSize: 24, color: "var(--ok)" }}>96.2%</div>
              <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>
                45 / 47 clean · 1 failed · 1 pending
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
                project refactor-patterns → claude/work
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
