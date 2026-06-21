import { useEffect, useState } from "react";
import { api, type OpsPayload, type RegistryOperationRecord } from "../lib/api/client";
import {
  operationActionLabel,
  bucketRegistryOperation,
  registryOperationDetailParts,
  registryOperationDisplayId,
  registryOperationStatusLabel,
  registryOperationSubjectLabel,
  registryOperationTargetLabel,
  splitOperationSkills,
} from "../lib/operation_labels";

const HISTORY_PAGE_SIZE = 100;

type HistoryData = NonNullable<OpsPayload["data"]>;

interface SkillMAuditHistoryProps {
  live: boolean;
  refreshKey: string | null;
}

interface HistoryState {
  loading: boolean;
  error: string | null;
  data: HistoryData | null;
}

const INITIAL_HISTORY_STATE: HistoryState = {
  loading: false,
  error: null,
  data: null,
};

export function SkillMAuditHistory({ live, refreshKey }: SkillMAuditHistoryProps) {
  const [offset, setOffset] = useState(0);
  const [state, setState] = useState<HistoryState>(INITIAL_HISTORY_STATE);

  useEffect(() => {
    if (!live) {
      setState(INITIAL_HISTORY_STATE);
      return;
    }

    const controller = new AbortController();
    setState((current) => ({ ...current, loading: true, error: null }));

    api.ops({ limit: HISTORY_PAGE_SIZE, offset }, controller.signal)
      .then((response) => {
        if (controller.signal.aborted) return;
        if (!response.ok || !response.data) {
          setState({
            loading: false,
            error: response.error?.message ?? "audit history fetch returned ok=false",
            data: null,
          });
          return;
        }
        setState({ loading: false, error: null, data: response.data });
      })
      .catch((error) => {
        if (controller.signal.aborted) return;
        setState({
          loading: false,
          error: error instanceof Error ? error.message : String(error),
          data: null,
        });
      });

    return () => controller.abort();
  }, [live, offset, refreshKey]);

  if (!live) {
    return <div className="ops-empty">Audit history needs the live panel API.</div>;
  }

  const data = state.data;
  const operations = data?.operations ?? [];
  const summary = data
    ? data.count > data.loaded_count
      ? `Showing ${data.loaded_count} of ${data.count} audit changes.`
      : `${data.loaded_count} loaded audit change${data.loaded_count === 1 ? "" : "s"}.`
    : null;

  return (
    <section className="ops-table">
      <div className="op-row">
        <span className="op-pill op-done">history</span>
        <span className="op-detail">
          {summary ?? (state.loading ? "Loading audit history..." : "Audit history")}
        </span>
        <span className="op-note">{state.error ?? "Fetched from /api/v1/ops, not the overview snapshot."}</span>
        <span />
        <button
          className="btn-ghost xs"
          onClick={() => setOffset((value) => Math.max(0, value - (data?.limit ?? HISTORY_PAGE_SIZE)))}
          disabled={!data || data.offset === 0}
        >
          newer
        </button>
        <button
          className="btn-ghost xs"
          onClick={() => setOffset((value) => value + (data?.limit ?? HISTORY_PAGE_SIZE))}
          disabled={!data?.has_more}
        >
          older
        </button>
      </div>
      {state.error && <div className="ops-empty">{state.error}</div>}
      {!state.error && operations.length === 0 && !state.loading && (
        <div className="ops-empty">No audit history returned by API.</div>
      )}
      {operations.map((op) => <AuditHistoryLine key={historyId(op)} op={op} />)}
    </section>
  );
}

function AuditHistoryLine({ op }: { op: RegistryOperationRecord }) {
  const rowClass = historyClass(op);
  const details = registryOperationDetailParts(op);
  const summaryDetails = details.filter((part) => !part.startsWith("id ")).slice(0, 2);
  const skills = op.skill ? splitOperationSkills(op.skill) : [];
  const target = registryOperationTargetLabel(op);

  return (
    <details className={`op-row op-log-row audit-history-row op-row-${rowClass}`}>
      <summary className="op-row-main">
        <span className={`op-pill op-${rowClass}`}>{registryOperationStatusLabel(op)}</span>
        <span className="op-time" title={op.updated_at || op.created_at}>{historyTime(op)}</span>
        <span className="op-verb">{operationActionLabel(op.intent)}</span>
        <span className="op-main-subject">
          <b>{registryOperationSubjectLabel(op)}</b>
          <code>{target}</code>
        </span>
        <span className="op-count-chip">{summaryDetails[0] ?? "详情"}</span>
        <span className="op-more-chip">展开</span>
      </summary>
      <div className="op-expanded">
        <div className="op-kv-grid">
          <span>intent</span><code>{op.intent}</code>
          <span>source</span><code>{op.source ?? "registry"}</code>
          <span>status</span><code>{op.status}</code>
          <span>id</span><code>{registryOperationDisplayId(op)}</code>
        </div>
        {details.length > 0 ? <div className="op-detail-line">{details.join(" · ")}</div> : null}
        {skills.length > 0 ? (
          <div className="op-skill-block">
            <span className="op-skill-label">skills</span>
            <div className="op-skill-chips">
              {skills.map((name) => <span key={name}>{name}</span>)}
            </div>
          </div>
        ) : null}
      </div>
    </details>
  );
}

function historyId(op: RegistryOperationRecord): string {
  return registryOperationDisplayId(op);
}

function historyClass(op: RegistryOperationRecord): "done" | "pending" | "failed" {
  const status = bucketRegistryOperation(op);
  if (status === "err") return "failed";
  if (status === "pending") return "pending";
  return "done";
}

function historyTime(op: RegistryOperationRecord): string {
  const iso = op.updated_at || op.created_at;
  const timestamp = Date.parse(iso);
  if (!Number.isFinite(timestamp)) return iso;
  const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}
