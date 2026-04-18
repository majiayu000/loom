import type { Op } from "../../lib/types";

function StatusRing({ status }: { status: Op["status"] }) {
  return <span className={`ring ${status}`} />;
}

export function OpRow({ op }: { op: Op }) {
  return (
    <div className="op-row">
      <StatusRing status={op.status} />
      <div>
        <div className="op-title">
          <span className="op-kind">{op.kind}</span>
          <span style={{ color: "var(--ink-0)" }}>{op.skill}</span>
          {op.target !== "—" && <span style={{ color: "var(--ink-3)" }}> → </span>}
          {op.target !== "—" && <span className="mono" style={{ color: "var(--ink-1)" }}>{op.target}</span>}
          {op.method !== "—" && (
            <span
              className={`chip method ${op.method}`}
              style={{ marginLeft: 8, padding: "0 6px", fontSize: 10 }}
            >
              {op.method}
            </span>
          )}
        </div>
        <div className="op-sub">
          {op.id}
          {op.reason ? ` · ${op.reason}` : ""}
        </div>
      </div>
      <div className="op-time">{op.time}</div>
    </div>
  );
}
