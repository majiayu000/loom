import type { Op } from "../../lib/types";

function StatusRing({ status }: { status: Op["status"] }) {
  return <span className={`ring ${status}`} />;
}

function kindLabel(kind: string): string {
  const normalized = kind.toLowerCase();
  if (normalized === "project") return "apply";
  if (normalized === "sync-push") return "sync push";
  if (normalized === "sync-pull") return "sync pull";
  if (normalized === "sync-replay") return "sync replay";
  return kind.replace(/[._-]/g, " ");
}

function statusLabel(status: Op["status"]): string {
  if (status === "ok") return "done";
  if (status === "err") return "failed";
  return "pending";
}

export function OpRow({ op }: { op: Op }) {
  return (
    <div className="op-row">
      <StatusRing status={op.status} />
      <div className="op-main">
        <div className="op-title">
          <span className="op-kind" title={`source command: ${op.kind}`}>
            {kindLabel(op.kind)}
          </span>
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
          <span className={`op-status ${op.status}`}>{statusLabel(op.status)}</span>
          <span className="mono">{op.id}</span>
          {op.reason ? ` · ${op.status === "err" ? "Blocked: " : ""}${op.reason}` : ""}
        </div>
      </div>
      <div className="op-time">{op.time}</div>
    </div>
  );
}
