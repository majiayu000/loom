import { useEffect, useMemo, useState } from "react";
import type { PanelDataMode } from "../../lib/api/usePanelData";
import { api, type OpsHistoryDiagnosePayload, type OpsPayload, type RegistryOperationRecord } from "../../lib/api/client";
import { MutationBanner } from "../../components/panel/MutationBanner";
import {
  bucketRegistryOperation,
  describeRegistryOperation,
  registryOperationDisplayId,
} from "../../lib/operation_labels";
import { useMutation } from "../../lib/useMutation";
import { useApiQuery } from "../../lib/useApiQuery";
import { COUNT_TERMS, filterLabel, summarizeOps } from "../../lib/count_labels";
import { SearchIcon } from "../../components/icons/nav_icons";

type FilterKey = "all" | "pending" | "ok" | "err";
type DiagnoseData = NonNullable<OpsHistoryDiagnosePayload["data"]>;
const HISTORY_PAGE_SIZE = 100;
type HistoryFilters = {
  source: string;
  intent: string;
  skill: string;
  target: string;
  binding: string;
  id: string;
};

const EMPTY_HISTORY_FILTERS: HistoryFilters = {
  source: "",
  intent: "",
  skill: "",
  target: "",
  binding: "",
  id: "",
};

type HistoryQueryPayload = {
  ops: NonNullable<OpsPayload["data"]>;
  diagnose: DiagnoseData | null;
  diagnoseError: string | null;
};

interface HistoryPageProps {
  live: boolean;
  mode: PanelDataMode;
  mutationVersion: number;
  refreshKey?: string | null;
  readOnly?: boolean;
  readOnlyReason?: string;
  onMutation?: () => void;
}

export function HistoryPage({
  live,
  mode,
  mutationVersion,
  refreshKey,
  readOnly = false,
  readOnlyReason,
  onMutation,
}: HistoryPageProps) {
  const [filter, setFilter] = useState<FilterKey>("all");
  const [query, setQuery] = useState("");
  const [filters, setFilters] = useState<HistoryFilters>(EMPTY_HISTORY_FILTERS);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [repairVersion, setRepairVersion] = useState(0);
  const state = useApiQuery<HistoryQueryPayload>(
    async (signal) => {
      const [opsResult, diagnoseResult] = await Promise.allSettled([
        api.ops({ limit: HISTORY_PAGE_SIZE, offset }, signal),
        api.opsHistoryDiagnose(signal),
      ]);

      if (opsResult.status === "rejected") {
        throw opsResult.reason;
      }

      const res = opsResult.value;
      if (!res.ok || !res.data) {
        throw new Error(res.error?.message ?? "activity fetch returned ok=false");
      }

      let diagnose: DiagnoseData | null = null;
      let diagnoseError: string | null = null;
      if (diagnoseResult.status === "fulfilled") {
        if (diagnoseResult.value.data) {
          diagnose = diagnoseResult.value.data;
        } else if (!diagnoseResult.value.ok) {
          diagnoseError = diagnoseResult.value.error?.message ?? "history diagnose returned ok=false";
        }
      } else {
        diagnoseError = diagnoseResult.reason instanceof Error ? diagnoseResult.reason.message : String(diagnoseResult.reason);
      }

      return { ops: res.data, diagnose, diagnoseError };
    },
    [mutationVersion, refreshKey, offset, repairVersion],
    { enabled: live },
  );
  const repair = useMutation();

  const runRepair = (strategy: "local" | "remote") => {
    if (readOnly || repair.busy) return;
    repair.run(`history repair ${strategy}`, () => api.opsHistoryRepair({ strategy }), () => {
      setRepairVersion((value) => value + 1);
      onMutation?.();
    });
  };

  const offlineHint =
    mode === "offline-stale"
      ? "Activity history is offline. Showing the last overview snapshot."
      : "Activity history needs the live panel API. Start `loom panel`.";

  const payload = state.kind === "ready" ? state.payload.ops : null;
  const diagnose = state.kind === "ready" ? state.payload.diagnose : null;
  const diagnoseError = state.kind === "ready" ? state.payload.diagnoseError : null;
  const operations = payload?.operations ?? [];

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return operations.filter((op) => {
      if (filter !== "all" && bucket(op) !== filter) return false;
      if (!matchesField(op.source, filters.source)) return false;
      if (!matchesField(op.intent, filters.intent)) return false;
      if (!matchesField(op.skill, filters.skill)) return false;
      if (!matchesField(op.target, filters.target)) return false;
      if (!matchesField(op.binding, filters.binding)) return false;
      if (!matchesField(registryOperationDisplayId(op), filters.id)) return false;
      if (!needle) return true;
      return (
        registryOperationDisplayId(op).toLowerCase().includes(needle) ||
        describeRegistryOperation(op).title.toLowerCase().includes(needle) ||
        op.intent.toLowerCase().includes(needle) ||
        (op.source ?? "").toLowerCase().includes(needle) ||
        (op.skill ?? "").toLowerCase().includes(needle) ||
        (op.target ?? "").toLowerCase().includes(needle) ||
        (op.binding ?? "").toLowerCase().includes(needle) ||
        (op.last_error?.message ?? "").toLowerCase().includes(needle)
      );
    });
  }, [operations, filter, filters, query]);

  useEffect(() => {
    if (selectedId && !filtered.some((op) => registryOperationDisplayId(op) === selectedId)) {
      setSelectedId(null);
    }
  }, [filtered, selectedId]);

  const counts = useMemo(
    () => summarizeOps(operations.map((op) => ({ status: bucket(op) }))),
    [operations],
  );

  const checkpoint = payload?.checkpoint;
  const repairDisabled = readOnly || repair.busy;
  const repairTitle = readOnly
    ? readOnlyReason ?? "registry offline"
    : repair.busy
    ? "history repair already running"
    : "repair conflicting history files";
  const historySummary =
    payload && payload.count > payload.loaded_count
      ? `Showing ${payload.loaded_count} of ${payload.count} audit changes.`
      : payload
      ? `${payload.loaded_count} loaded audit change${payload.loaded_count === 1 ? "" : "s"}.`
      : null;
  const selectedOperation = filtered.find((op) => registryOperationDisplayId(op) === selectedId) ?? null;
  const setField = (key: keyof HistoryFilters, value: string) => {
    setFilters((current) => ({ ...current, [key]: value }));
  };
  const hasStructuredFilters = Object.values(filters).some((value) => value.trim().length > 0);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Activity history</h1>
          <div className="subtitle">
            Every audit change Loom has recorded. Replayable writes also appear in Activity; failed work points to a replay with{" "}
            <span className="mono">loom sync replay</span>.
          </div>
        </div>
        <div className="header-actions">
          <div className="searchbar" style={{ width: 260 }}>
            <SearchIcon />
            <input
              placeholder="Filter by id / intent / error…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>
      </div>
      <MutationBanner state={repair} variant="bar" />
      <div className="page-body">
        {state.kind === "error" && (
          <MutationBanner message={state.message} tone="err" variant="bar" />
        )}
        {!live && <div className="empty" style={{ marginBottom: 18 }}>{offlineHint}</div>}
        {diagnoseError && (
          <div className="mono" style={{ color: "var(--warn)", fontSize: 11, marginBottom: 12 }}>
            History diagnose: {diagnoseError}
          </div>
        )}
        {diagnose?.local_branch && (
          <div style={{ display: "flex", gap: 12, padding: "4px 0", marginBottom: 12, fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--ink-3)", alignItems: "center", flexWrap: "wrap" }}>
            <span>{diagnose.local_segments} segment{diagnose.local_segments === 1 ? "" : "s"}</span>
            {diagnose.remote_tracking && diagnose.ahead > 0 && <span style={{ color: "var(--ok)" }}>↑ {diagnose.ahead} ahead</span>}
            {diagnose.remote_tracking && diagnose.behind > 0 && <span style={{ color: "var(--pending)" }}>↓ {diagnose.behind} behind</span>}
            {diagnose.remote_tracking && diagnose.ahead === 0 && diagnose.behind === 0 && <span>in sync</span>}
            {diagnose.conflicts.length > 0 && (
              <>
                <span style={{ color: "var(--err)" }}>
                  {diagnose.conflicts.length} conflict{diagnose.conflicts.length === 1 ? "" : "s"}
                </span>
                <span style={{ color: "var(--ink-2)" }}>{diagnose.conflicts[0]?.path}</span>
                <button
                  className="btn sm"
                  onClick={() => runRepair("local")}
                  disabled={repairDisabled}
                  title={repairTitle}
                >
                  Repair from local
                </button>
                <button
                  className="btn sm"
                  onClick={() => runRepair("remote")}
                  disabled={repairDisabled}
                  title={repairTitle}
                >
                  Repair from remote
                </button>
              </>
            )}
          </div>
        )}
        <div className="kpi-row">
          <Kpi label={COUNT_TERMS.loadedAuditChanges} value={payload?.loaded_count ?? counts.all} />
          <Kpi label={COUNT_TERMS.replayableWrites} value={counts.pending} tone={counts.pending > 0 ? "pending" : undefined} />
          <Kpi label={COUNT_TERMS.succeeded} value={counts.ok} />
          <Kpi label={COUNT_TERMS.failed} value={counts.err} tone={counts.err > 0 ? "err" : undefined} />
        </div>

        <div style={{ display: "flex", gap: 12, marginBottom: 12, justifyContent: "space-between", flexWrap: "wrap" }}>
          <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
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
          {hasStructuredFilters && (
            <button className="btn sm" onClick={() => setFilters(EMPTY_HISTORY_FILTERS)}>
              Clear operation filters
            </button>
          )}
          {payload && (
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              {historySummary && (
                <span className="mono" style={{ fontSize: 11, color: "var(--ink-3)" }}>
                  {historySummary}
                </span>
              )}
              <button className="btn sm" onClick={() => setOffset((cur) => Math.max(0, cur - payload.limit))} disabled={payload.offset === 0}>
                newer
              </button>
              <button className="btn sm" onClick={() => setOffset((cur) => cur + payload.limit)} disabled={!payload.has_more}>
                older
              </button>
            </div>
          )}
        </div>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
            gap: 8,
            marginBottom: 12,
          }}
        >
          <FilterInput label="Source filter" value={filters.source} onChange={(value) => setField("source", value)} />
          <FilterInput label="Command or intent filter" value={filters.intent} onChange={(value) => setField("intent", value)} />
          <FilterInput label="Skill filter" value={filters.skill} onChange={(value) => setField("skill", value)} />
          <FilterInput label="Target filter" value={filters.target} onChange={(value) => setField("target", value)} />
          <FilterInput label="Binding filter" value={filters.binding} onChange={(value) => setField("binding", value)} />
          <FilterInput label="Request/audit/op id filter" value={filters.id} onChange={(value) => setField("id", value)} />
        </div>

        <div
          className="history-table-wrap"
          style={{
            background: "var(--bg-1)",
            borderRadius: 10,
            border: "1px solid var(--line)",
          }}
        >
          <table className="tbl history-table mobile-cards">
            <thead>
              <tr>
                <th>Action</th>
                <th>Details</th>
                <th>Status</th>
                <th>Created</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {state.kind === "loading" && (
                <tr>
                  <td
                    colSpan={5}
                    className="mono table-empty-cell"
                    style={{ textAlign: "center", color: "var(--ink-3)", padding: 18 }}
                  >
                    loading…
                  </td>
                </tr>
              )}
              {state.kind === "ready" && filtered.length === 0 && (
                <tr>
                  <td
                    colSpan={5}
                    className="table-empty-cell"
                    style={{ textAlign: "center", color: "var(--ink-3)", padding: 18 }}
                  >
                    {operations.length === 0
                      ? "No activity recorded yet — every CLI or Panel change will show up here."
                      : "No activity matches the current filter."}
                  </td>
                </tr>
              )}
              {filtered.map((op) => (
                <OpHistoryRow
                  key={registryOperationDisplayId(op)}
                  op={op}
                  selected={selectedId === registryOperationDisplayId(op)}
                  onSelect={() => setSelectedId(registryOperationDisplayId(op))}
                />
              ))}
            </tbody>
          </table>
        </div>

        {selectedOperation && (
          <HistoryDetailDrawer op={selectedOperation} onClose={() => setSelectedId(null)} />
        )}

        {checkpoint && (
          <div style={{ marginTop: 12, fontSize: 11, color: "var(--ink-3)" }}>
            Checkpoint: last scanned{" "}
            <span className="mono" style={{ color: "var(--ink-1)" }}>
              {checkpoint.last_scanned_op_id ?? "—"}
            </span>
            {checkpoint.last_acked_op_id && (
              <>
                {" · "}last acked{" "}
                <span className="mono" style={{ color: "var(--ink-1)" }}>
                  {checkpoint.last_acked_op_id}
                </span>
              </>
            )}
            {checkpoint.updated_at && (
              <>
                {" · updated "}
                <span className="mono">{checkpoint.updated_at}</span>
              </>
            )}
          </div>
        )}
      </div>
    </>
  );
}

export function bucket(op: RegistryOperationRecord): "pending" | "ok" | "err" {
  return bucketRegistryOperation(op);
}

function OpHistoryRow({
  op,
  selected,
  onSelect,
}: {
  op: RegistryOperationRecord;
  selected: boolean;
  onSelect: () => void;
}) {
  const kind = bucket(op);
  const color = kind === "err" ? "var(--err)" : kind === "pending" ? "var(--pending)" : "var(--ok)";
  const label = describeRegistryOperation(op);
  return (
    <tr onClick={onSelect} style={{ cursor: "pointer", outline: selected ? "1px solid var(--accent)" : undefined }}>
      <td className="name" data-label="Action">
        <div>{label.title}</div>
        {op.last_error && (
          <div className="mono" style={{ color: "var(--err)", fontSize: 10.5, marginTop: 3 }}>
            {op.last_error.message}
          </div>
        )}
      </td>
      <td data-label="Details">
        <div className="op-detail-stack">
          {label.details.map((detail) => (
            <span key={detail} className="op-meta" title={detail.startsWith("id ") ? label.technicalId : undefined}>
              {detail}
            </span>
          ))}
        </div>
      </td>
      <td data-label="Status">
        <span className="chip" style={{ color }}>
          {op.last_error ? op.last_error.code : op.status}
        </span>
      </td>
      <td className="mono dim" data-label="Created" style={{ fontSize: 10.5 }}>
        {op.created_at}
      </td>
      <td className="mono dim mobile-hide" data-label="Updated" style={{ fontSize: 10.5 }}>
        {op.updated_at}
      </td>
    </tr>
  );
}

function FilterInput({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <label style={{ display: "grid", gap: 4, minWidth: 0 }}>
      <span style={{ color: "var(--ink-3)", fontSize: 10.5, textTransform: "uppercase" }}>{label}</span>
      <input
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="field-input"
        style={{ minHeight: 30, fontSize: 11 }}
      />
    </label>
  );
}

function HistoryDetailDrawer({ op, onClose }: { op: RegistryOperationRecord; onClose: () => void }) {
  const payload = summarizePayload(op.payload);
  const effects = summarizePayload(op.effects);
  return (
    <div className="card" style={{ marginTop: 14 }}>
      <div className="card-head">
        <h3>Audit detail</h3>
        <button className="btn sm" onClick={onClose}>Close</button>
      </div>
      <div className="card-body">
        <div className="kv" style={{ margin: 0 }}>
          <div className="k">Operation id</div>
          <div className="v mono">{op.op_id ?? "—"}</div>
          <div className="k">Audit id</div>
          <div className="v mono">{op.audit_id ?? "—"}</div>
          <div className="k">Request id</div>
          <div className="v mono">{op.request_id ?? "—"}</div>
          <div className="k">Source</div>
          <div className="v mono">{op.source ?? "—"}</div>
          <div className="k">Reason/error</div>
          <div className="v">{op.last_error ? `${op.last_error.code}: ${op.last_error.message}` : "—"}</div>
          <div className="k">Payload</div>
          <div className="v mono">{payload}</div>
          <div className="k">Effects</div>
          <div className="v mono">{effects}</div>
          <div className="k">Related objects</div>
          <div className="v mono">{relatedObjects(op).join(" · ") || "—"}</div>
        </div>
      </div>
    </div>
  );
}

function matchesField(value: string | null | undefined, filter: string): boolean {
  const needle = filter.trim().toLowerCase();
  return !needle || (value ?? "").toLowerCase().includes(needle);
}

function summarizePayload(value: unknown): string {
  if (value == null) return "—";
  if (typeof value !== "object") return String(value);
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length === 0) return "{}";
  return entries.slice(0, 4).map(([key, item]) => `${key}:${payloadValue(item)}`).join(" · ");
}

function payloadValue(value: unknown): string {
  if (Array.isArray(value)) return `[${value.length}]`;
  if (value && typeof value === "object") return "{...}";
  return String(value);
}

function relatedObjects(op: RegistryOperationRecord): string[] {
  return [
    op.skill ? `skill ${op.skill}` : null,
    op.target ? `target ${op.target}` : null,
    op.binding ? `binding ${op.binding}` : null,
  ].filter((item): item is string => Boolean(item));
}

function Kpi({ label, value, tone }: { label: string; value: string | number; tone?: "pending" | "err" }) {
  const color = tone === "pending" ? "var(--pending)" : tone === "err" ? "var(--err)" : "var(--ink-0)";
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value" style={{ color }}>
        {value}
      </div>
    </div>
  );
}
