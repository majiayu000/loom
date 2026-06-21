import { useEffect, useMemo, useState } from "react";
import { api, type OpsPayload, type RegistryOperationRecord } from "../lib/api/client";
import {
  bucketRegistryOperation,
  describeRegistryOperation,
  registryOperationDetailParts,
  registryOperationDisplayId,
  registryOperationStatusLabel,
  splitOperationSkills,
} from "../lib/operation_labels";

const HISTORY_PAGE_SIZE = 100;
const STATUS_FILTERS = [
  ["all", "All statuses"],
  ["ok", "Done"],
  ["pending", "Pending"],
  ["err", "Failed"],
] as const;

type HistoryData = NonNullable<OpsPayload["data"]>;
type AuditStatusFilter = (typeof STATUS_FILTERS)[number][0];
type AuditRowLabel = ReturnType<typeof describeRegistryOperation>;

interface AuditHistoryRow {
  op: RegistryOperationRecord;
  label: AuditRowLabel;
  status: "ok" | "pending" | "err";
  searchText: string;
}

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
  const [textFilter, setTextFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<AuditStatusFilter>("all");
  const [categoryFilter, setCategoryFilter] = useState("all");
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

  const data = state.data;
  const operations = data?.operations ?? [];
  const rows = useMemo<AuditHistoryRow[]>(
    () =>
      operations.map((op) => {
        const label = describeRegistryOperation(op);
        return {
          op,
          label,
          status: bucketRegistryOperation(op),
          searchText: auditSearchText(op, label),
        };
      }),
    [operations],
  );
  const categoryOptions = useMemo(() => {
    const categories = Array.from(new Set(rows.map((row) => row.label.category)));
    categories.sort((left, right) => left.localeCompare(right));
    return categoryFilter !== "all" && !categories.includes(categoryFilter)
      ? [categoryFilter, ...categories]
      : categories;
  }, [categoryFilter, rows]);
  const normalizedTextFilter = textFilter.trim().toLowerCase();
  const filteredRows = rows.filter((row) => {
    if (statusFilter !== "all" && row.status !== statusFilter) return false;
    if (categoryFilter !== "all" && row.label.category !== categoryFilter) return false;
    return normalizedTextFilter.length === 0 || row.searchText.includes(normalizedTextFilter);
  });
  const filtersActive =
    normalizedTextFilter.length > 0 || statusFilter !== "all" || categoryFilter !== "all";
  const summary = data
    ? filtersActive
      ? `${filteredRows.length} of ${operations.length} loaded audit changes match filters.`
      : data.count > data.loaded_count
      ? `Showing ${data.loaded_count} of ${data.count} audit changes.`
      : `${data.loaded_count} loaded audit change${data.loaded_count === 1 ? "" : "s"}.`
    : null;

  if (!live) {
    return <div className="ops-empty">Audit history needs the live panel API.</div>;
  }

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
      <div className="op-row audit-history-filters" aria-label="Audit history filters">
        <input
          aria-label="Audit text filter"
          value={textFilter}
          onChange={(event) => setTextFilter(event.target.value)}
          placeholder="Filter audit history"
          className="field-input"
          style={{ minHeight: 30, minWidth: 180 }}
          disabled={!data}
        />
        <select
          aria-label="Audit status filter"
          value={statusFilter}
          onChange={(event) => setStatusFilter(event.target.value as AuditStatusFilter)}
          className="field-input"
          style={{ minHeight: 30 }}
          disabled={!data}
        >
          {STATUS_FILTERS.map(([value, label]) => (
            <option key={value} value={value}>{label}</option>
          ))}
        </select>
        <select
          aria-label="Audit operation type filter"
          value={categoryFilter}
          onChange={(event) => setCategoryFilter(event.target.value)}
          className="field-input"
          style={{ minHeight: 30 }}
          disabled={!data}
        >
          <option value="all">All operation types</option>
          {categoryOptions.map((category) => (
            <option key={category} value={category}>{category}</option>
          ))}
        </select>
        <span className="op-note">
          {filtersActive ? `${filteredRows.length} matches on this page` : "Filtering applies to the loaded page"}
        </span>
      </div>
      {state.error && <div className="ops-empty">{state.error}</div>}
      {!state.error && operations.length === 0 && !state.loading && (
        <div className="ops-empty">No audit history returned by API.</div>
      )}
      {!state.error && operations.length > 0 && filteredRows.length === 0 && (
        <div className="ops-empty">No audit history matches the current filters.</div>
      )}
      {filteredRows.map((row) => <AuditHistoryLine key={historyId(row.op)} op={row.op} label={row.label} />)}
    </section>
  );
}

function AuditHistoryLine({ op, label }: { op: RegistryOperationRecord; label: AuditRowLabel }) {
  const [expanded, setExpanded] = useState(false);
  const rowClass = historyClass(op);
  const details = registryOperationDetailParts(op);
  const skills = op.skill ? splitOperationSkills(op.skill) : [];
  const summaryChip = auditSummaryChip(op, skills);

  return (
    <details open={expanded} className={`op-row op-log-row audit-history-row op-row-${rowClass}`}>
      <summary
        className="op-row-main"
        onClick={(event) => {
          event.preventDefault();
          setExpanded((value) => !value);
        }}
      >
        <span className={`op-pill op-${rowClass}`}>{registryOperationStatusLabel(op)}</span>
        <span className="op-time">{historyTime(op)}</span>
        <span className="op-verb">{label.category}</span>
        <span className="op-main-subject">
          <b>{label.title}</b>
        </span>
        <span className="op-count-chip">{summaryChip}</span>
        <span className="op-more-chip">{expanded ? "收起" : "详情"}</span>
      </summary>
      {expanded ? (
        <div className="op-expanded">
          <div className="op-kv-grid">
            <span>intent</span><code>{op.intent}</code>
            <span>source</span><code>{op.source ?? "registry"}</code>
            <span>status</span><code>{op.status}</code>
            <span>operation id</span><code>{op.op_id ?? "—"}</code>
            <span>audit id</span><code>{op.audit_id ?? "—"}</code>
            <span>request id</span><code>{op.request_id ?? "—"}</code>
            <span>created</span><code>{op.created_at}</code>
            <span>updated</span><code>{op.updated_at}</code>
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
      ) : null}
    </details>
  );
}

function auditSearchText(op: RegistryOperationRecord, label: AuditRowLabel): string {
  return [
    label.category,
    label.title,
    ...label.details,
    registryOperationStatusLabel(op),
    op.intent,
    op.status,
    op.source,
    op.skill,
    op.target,
    op.binding,
    op.method,
    op.last_error?.code,
    op.last_error?.message,
    op.op_id,
    op.audit_id,
    op.request_id,
  ]
    .filter((value): value is string => Boolean(value))
    .join(" ")
    .toLowerCase();
}

function auditSummaryChip(op: RegistryOperationRecord, skills: string[]): string {
  if (op.last_error) return "错误详情";
  if (skills.length > 1) return `批量 ${skills.length}`;
  return op.ack ? "已同步" : "待同步";
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
