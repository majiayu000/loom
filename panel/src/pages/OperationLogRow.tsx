import type { Op } from "../lib/types";
import {
  operationActionLabel,
  operationDetailParts,
  operationStatusLabel,
  operationSubjectLabel,
  splitOperationSkills,
} from "../lib/operation_labels";

interface OperationLogRowProps {
  op: Op;
}

function classForStatus(status: Op["status"]) {
  if (status === "ok") return "done";
  if (status === "err") return "failed";
  return "pending";
}

function methodLabel(method: Op["method"]) {
  return method === "—" ? "无方式" : method;
}

export function OperationLogRow({ op }: OperationLogRowProps) {
  const rowClass = classForStatus(op.status);
  const skills = splitOperationSkills(op.skill).filter((name) => name !== op.kind);
  const details = operationDetailParts(op);
  const countLabel = skills.length > 1 ? `批量 ${skills.length}` : methodLabel(op.method);

  return (
    <details className={`op-row op-log-row op-row-${rowClass}`}>
      <summary className="op-row-main">
        <span className={`op-pill op-${rowClass}`}>{operationStatusLabel(op.status)}</span>
        <span className="op-time">{op.time}</span>
        <span className="op-verb">{operationActionLabel(op.kind)}</span>
        <span className="op-main-subject">
          <b>{operationSubjectLabel(op)}</b>
          {op.target !== "—" ? <code>{op.target}</code> : null}
        </span>
        <span className="op-count-chip">{countLabel}</span>
        <span className="op-more-chip">详情</span>
      </summary>
      <div className="op-expanded">
        <div className="op-kv-grid">
          <span>intent</span><code>{op.kind}</code>
          <span>target</span><code>{op.target}</code>
          <span>method</span><code>{methodLabel(op.method)}</code>
          <span>id</span><code>{op.id}</code>
        </div>
        {details.length > 0 ? <div className="op-detail-line">{details.join(" · ")}</div> : null}
        {skills.length > 0 ? (
          <div className="op-skill-block">
            <span className="op-skill-label">skills</span>
            <div className="op-skill-chips">
              {skills.map((name) => (
                <span key={name}>{name}</span>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </details>
  );
}
