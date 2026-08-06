import type { Op } from "../../lib/types";
import { describeActivityOperation, statusWord } from "../../lib/operation_labels";

function StatusRing({ status }: { status: Op["status"] }) {
  return <span className={`ring ${status}`} />;
}

export function OpRow({ op }: { op: Op }) {
  const label = describeActivityOperation(op);
  return (
    <span className="op-row">
      <StatusRing status={op.status} />
      <span className="op-main">
        <span className="op-title">
          <span className="op-kind" title={`source command: ${op.kind}`}>
            {label.category}
          </span>
          <span style={{ color: "var(--ink-0)" }}>{label.title}</span>
          {op.method !== "—" && (
            <span
              className={`chip method ${op.method}`}
              style={{ marginLeft: 8, padding: "0 6px", fontSize: 10 }}
            >
              {op.method}
            </span>
          )}
        </span>
        <span className="op-sub">
          <span className={`op-status ${op.status}`}>{statusWord(op.status)}</span>
          {label.details.map((detail) => (
            <span key={detail} className="op-meta" title={detail.startsWith("id ") ? label.technicalId : undefined}>
              {detail}
            </span>
          ))}
        </span>
      </span>
      <span className="op-time">{op.time}</span>
    </span>
  );
}
