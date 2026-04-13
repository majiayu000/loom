import { useMemo, useState } from "react";
import { formatTime, pick } from "../i18n";
import type { Locale } from "../i18n";
import { Icon } from "../components/icon";
import {
  detailLabel,
  explainPendingOp,
  explainPendingQueue,
  opToneClass,
  pendingFactEntries,
  pendingNextStep,
  queueStateLabel,
  scheduledOpIcon,
  truncateText,
} from "../lib/panel_data";
import { filterPendingOps } from "../lib/panel_selectors";
import type { PanelData, PendingOp } from "../types";

function OperationPayload({
  op,
  locale,
}: {
  op: PendingOp;
  locale: Locale;
}) {
  const facts = pendingFactEntries(op.details);

  return (
    <>
      <p className="stitch-op-meaning">{explainPendingOp(op, locale)}</p>
      {facts.length > 0 ? (
        <div className="stitch-op-facts">
          {facts.map((fact) => (
            <article className="stitch-op-fact" key={fact.key}>
              <span>{detailLabel(fact.key, locale)}</span>
              <strong>{fact.value}</strong>
            </article>
          ))}
        </div>
      ) : null}
    </>
  );
}

export function OpsPage({ data, locale, query }: { data: PanelData; locale: Locale; query: string }) {
  const filteredOps = useMemo(() => filterPendingOps(data.pending.ops, query), [data.pending.ops, query]);
  const [selectedOpId, setSelectedOpId] = useState<string>(filteredOps[0]?.request_id ?? "");
  const activeOp = filteredOps.find((op) => op.request_id === selectedOpId) ?? filteredOps[0] ?? null;
  const queuedOps = filteredOps.slice(0, 6);
  const scheduledOps = filteredOps.slice(6, 10);
  const activeOpIndex = activeOp ? filteredOps.findIndex((op) => op.request_id === activeOp.request_id) + 1 : 0;

  return (
    <div className="stitch-screen-column">
      <div className="ops-grid stitch-ops-layout">
        <section className="panel stitch-queue-panel">
          <div className="stitch-queue-head">
            <h2 className="panel-title">{pick(locale, "Unsynced Local Changes", "待同步的本地改动")}</h2>
            <span className="minor-chip is-primary">
              {locale === "zh-CN" ? `${filteredOps.length} 条匹配` : `${filteredOps.length} matched`}
            </span>
          </div>
          <p className="stitch-ops-explainer">{explainPendingQueue({ ...data, pending: { ...data.pending, count: filteredOps.length, ops: filteredOps } }, locale)}</p>
          <div className="stitch-queue-body">
            {queuedOps.length === 0 ? (
              <div className="empty-state">{pick(locale, "queue empty", "队列为空")}</div>
            ) : (
              queuedOps.map((op, index) => {
                const tone = opToneClass(op, data.remote.sync_state, index);
                return (
                  <button
                    className={`stitch-op-card ${tone} ${activeOp?.request_id === op.request_id ? "active" : ""}`}
                    key={op.request_id}
                    onClick={() => setSelectedOpId(op.request_id)}
                    type="button"
                  >
                    <div className="stitch-op-card-meta">
                      <span>{op.op_id ?? op.request_id}</span>
                      <span>{formatTime(locale, op.created_at)}</span>
                    </div>
                    <h3>{op.command.toUpperCase().split(" ").join("_")}</h3>
                    <OperationPayload locale={locale} op={op} />
                    <div className="stitch-op-card-tags">
                      <span className="stitch-status-tag">{queueStateLabel(index, data.remote.sync_state, locale)}</span>
                    </div>
                  </button>
                );
              })
            )}

            {scheduledOps.length > 0 ? (
              <>
                <div className="stitch-queue-divider">
                  <span>{pick(locale, "More queued records", "后续待同步记录")}</span>
                </div>
                {scheduledOps.map((op, index) => (
                  <div className="stitch-scheduled-card" key={op.request_id}>
                    <div className="stitch-scheduled-card-main">
                      <Icon name={scheduledOpIcon(index)} />
                      <div>
                        <h4>{op.command.toUpperCase().split(" ").join("_")}</h4>
                        <p>{pick(locale, "Waiting behind earlier matched records", "等待前面匹配到的记录先处理")}</p>
                      </div>
                    </div>
                    <span className="stitch-status-tag">{queueStateLabel(index + queuedOps.length, data.remote.sync_state, locale)}</span>
                  </div>
                ))}
              </>
            ) : null}
          </div>
        </section>

        <section className="panel stitch-terminal-panel">
          <div className="stitch-terminal-metadata">
            <div>
              <span>{pick(locale, "Selected Record", "当前记录")}</span>
              <strong>{activeOp ? activeOp.command.toUpperCase().split(" ").join("_") : "IDLE"}</strong>
            </div>
            <div>
              <span>{pick(locale, "Queue Position", "队列位置")}</span>
              <strong>{activeOp ? `${activeOpIndex}/${filteredOps.length}` : "—"}</strong>
            </div>
            <div>
              <span>{pick(locale, "Queued At", "入队时间")}</span>
              <strong>{activeOp ? formatTime(locale, activeOp.created_at) : "—"}</strong>
            </div>
            <div>
              <span>{pick(locale, "Internal Id", "内部 ID")}</span>
              <strong className="mono-copy">{truncateText(activeOp?.op_id ?? activeOp?.request_id ?? "—", 28)}</strong>
            </div>
          </div>

          <div className="stitch-terminal-output">
            {activeOp ? (
              <div className="stitch-terminal-detail-card">
                <div className="stitch-terminal-detail-head">
                  <span>{pick(locale, "What This Record Means", "这条记录表示什么")}</span>
                  <span>{pick(locale, "Local unsynced entry", "本地未同步记录")}</span>
                </div>
                <OperationPayload locale={locale} op={activeOp} />
              </div>
            ) : null}
            <div className="stitch-terminal-row stitch-terminal-head-row">
              <span>{pick(locale, "Time", "时间")}</span>
              <span>{pick(locale, "Operation", "操作")}</span>
              <span>{pick(locale, "Meaning", "含义")}</span>
            </div>
            {(activeOp ? [activeOp, ...filteredOps.filter((op) => op.request_id !== activeOp.request_id)] : filteredOps)
              .slice(0, 6)
              .map((op) => (
                <div className="stitch-terminal-row" key={op.request_id}>
                  <span>{formatTime(locale, op.created_at)}</span>
                  <span className={opToneClass(op, data.remote.sync_state)}>{`[${op.command.split(" ")[0].toUpperCase()}]`}</span>
                  <span className="stitch-terminal-message">{explainPendingOp(op, locale)}</span>
                </div>
              ))}
          </div>

          <div className="stitch-terminal-footer">
            <div className="stitch-terminal-next-step">
              <span>{pick(locale, "Next Step", "下一步")}</span>
              <strong>{pendingNextStep({ ...data, pending: { ...data.pending, count: filteredOps.length, ops: filteredOps } }, locale)}</strong>
            </div>
            <div className="stitch-terminal-owner">
              <span className="stitch-status-tag">{pick(locale, "Read-only queue view", "只读队列视图")}</span>
              <span className="stitch-status-tag">{pick(locale, `Queue size: ${filteredOps.length}`, `队列数量: ${filteredOps.length}`)}</span>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
