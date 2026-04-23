import type { CSSProperties, ReactNode } from "react";
import { useMemo, useState } from "react";
import type { Op, OpStatus } from "../../lib/types";
import { OpRow } from "../../components/panel/OpRow";
import { RefreshIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";

const SPARK = [3, 5, 2, 6, 4, 7, 9, 5, 8, 6, 4, 3, 5, 7, 11, 6, 4, 5, 8, 9, 6, 4, 3, 5];
type FilterKey = "all" | OpStatus;
type RepairStrategy = "local" | "remote";

interface OpsPageProps {
  ops: Op[];
  onMutation: () => void;
  readOnly: boolean;
}

interface HistoryConflict {
  scope?: string;
  path?: string;
  local_blob?: string;
  remote_blob?: string;
  local_rename_path?: string;
  remote_rename_path?: string;
}

interface HistoryDiagnoseResult {
  local_branch?: boolean;
  remote_tracking?: boolean;
  ahead?: number;
  behind?: number;
  local_segments?: number;
  local_archives?: number;
  remote_segments?: number;
  remote_archives?: number;
  local_snapshot?: boolean;
  remote_snapshot?: boolean;
  compact_after_segments?: number;
  retain_recent_segments?: number;
  retain_archives?: number;
  conflicts?: HistoryConflict[];
}

interface ActionState {
  busy: string | null;
  error: string | null;
  success: string | null;
}

const INITIAL_ACTION_STATE: ActionState = {
  busy: null,
  error: null,
  success: null,
};

function uniqueSorted(values: string[]) {
  return Array.from(new Set(values.filter(Boolean))).sort((a, b) => a.localeCompare(b));
}

export function OpsPage({ ops, onMutation, readOnly }: OpsPageProps) {
  const [filter, setFilter] = useState<FilterKey>("all");
  const [skillFilter, setSkillFilter] = useState("all");
  const [targetFilter, setTargetFilter] = useState("all");
  const [kindFilter, setKindFilter] = useState("all");
  const [selectedId, setSelectedId] = useState<string | null>(ops[0]?.id ?? null);
  const [diagnose, setDiagnose] = useState<HistoryDiagnoseResult | null>(null);
  const [repairStrategy, setRepairStrategy] = useState<RepairStrategy>("local");
  const [action, setAction] = useState<ActionState>(INITIAL_ACTION_STATE);

  const counts = {
    all: ops.length,
    pending: ops.filter((o) => o.status === "pending").length,
    ok: ops.filter((o) => o.status === "ok").length,
    err: ops.filter((o) => o.status === "err").length,
  };

  const skillOptions = useMemo(() => uniqueSorted(ops.map((o) => o.skill)), [ops]);
  const targetOptions = useMemo(
    () => uniqueSorted(ops.filter((o) => o.target !== "—").map((o) => o.target)),
    [ops],
  );
  const kindOptions = useMemo(() => uniqueSorted(ops.map((o) => o.kind)), [ops]);

  const filtered = useMemo(() => {
    return ops.filter((o) => {
      if (filter !== "all" && o.status !== filter) return false;
      if (skillFilter !== "all" && o.skill !== skillFilter) return false;
      if (targetFilter !== "all" && o.target !== targetFilter) return false;
      if (kindFilter !== "all" && o.kind !== kindFilter) return false;
      return true;
    });
  }, [filter, kindFilter, ops, skillFilter, targetFilter]);

  const selected = filtered.find((o) => o.id === selectedId) ?? filtered[0] ?? null;

  async function runAction(label: string, fn: () => Promise<unknown>, refresh = true) {
    setAction({ busy: label, error: null, success: null });
    try {
      await fn();
      setAction({ busy: null, error: null, success: label });
      if (refresh) onMutation();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setAction({ busy: null, error: `${label}: ${message}`, success: null });
    }
  }

  const diagnoseHistory = async () => {
    setAction({ busy: "diagnose history", error: null, success: null });
    try {
      const res = await api.opsHistoryDiagnose();
      setDiagnose((res.data ?? {}) as HistoryDiagnoseResult);
      setAction({ busy: null, error: null, success: "diagnose history" });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setAction({ busy: null, error: `diagnose history: ${message}`, success: null });
    }
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Ops</h1>
          <div className="subtitle">Every state change is an op. Retry pending work, purge queue residue, and repair history divergence from one surface.</div>
        </div>
        <div className="header-actions" style={{ flexWrap: "wrap" }}>
          <button
            className="btn ghost"
            disabled={readOnly || action.busy !== null}
            onClick={() => runAction("retry failed", api.opsRetry)}
            title={readOnly ? "registry offline" : undefined}
          >
            <RefreshIcon /> Retry failed
          </button>
          <button
            className="btn ghost"
            disabled={readOnly || action.busy !== null}
            onClick={() => runAction("purge completed", api.opsPurge)}
            title={readOnly ? "registry offline" : undefined}
          >
            Purge completed
          </button>
          <button className="btn ghost" disabled={action.busy !== null} onClick={diagnoseHistory}>
            Diagnose history
          </button>
          <select
            value={repairStrategy}
            onChange={(e) => setRepairStrategy(e.target.value as RepairStrategy)}
            style={miniSelectStyle}
          >
            <option value="local">repair: keep local</option>
            <option value="remote">repair: keep remote</option>
          </select>
          <button
            className="btn primary"
            disabled={readOnly || action.busy !== null}
            onClick={() => runAction("repair history", () => api.opsHistoryRepair({ strategy: repairStrategy }))}
            title={readOnly ? "registry offline" : undefined}
          >
            Repair history
          </button>
        </div>
      </div>
      {(action.error || action.success || action.busy) && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: action.error ? "var(--err)" : action.busy ? "var(--ink-2)" : "var(--ok)",
            background: action.error
              ? "rgba(216,90,90,0.08)"
              : action.busy
              ? "var(--bg-2)"
              : "rgba(111,183,138,0.08)",
          }}
        >
          {action.busy ? `${action.busy}…` : action.error ?? `✓ ${action.success}`}
        </div>
      )}
      <div className="page-body">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, marginBottom: 18 }}>
          <MetricCard label="Last 24h" value={`${ops.length} ops`} sub="Panel-visible queue + projection events">
            <div className="spark" style={{ marginTop: 10 }}>
              {SPARK.map((v, i) => (
                <div key={i} className={`bar ${v > 7 ? "hi" : ""}`} style={{ height: `${v * 2.4}px` }} />
              ))}
            </div>
          </MetricCard>
          <MetricCard
            label="Success rate"
            value={`${ops.length === 0 ? 100 : Math.round((counts.ok / Math.max(ops.length, 1)) * 1000) / 10}%`}
            accent="var(--ok)"
            sub={`${counts.ok} clean · ${counts.err} failed · ${counts.pending} pending`}
          />
          <MetricCard label="Pending" value={`${counts.pending}`} accent="var(--pending)" sub="retry or replay unresolved work from here" />
        </div>

        <div style={{ display: "flex", gap: 4, marginBottom: 12, flexWrap: "wrap" }}>
          {(["all", "pending", "ok", "err"] as FilterKey[]).map((k) => (
            <button
              key={k}
              className="btn sm"
              onClick={() => setFilter(k)}
              style={filterButtonStyle(filter === k)}
            >
              {k === "err" ? "failed" : k}{" "}
              <span className="mono" style={{ color: "var(--ink-3)", marginLeft: 4 }}>
                {counts[k]}
              </span>
            </button>
          ))}
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 8, marginBottom: 14 }}>
          <FilterSelect label="skill" value={skillFilter} onChange={setSkillFilter} options={skillOptions} />
          <FilterSelect label="target" value={targetFilter} onChange={setTargetFilter} options={targetOptions} />
          <FilterSelect label="kind" value={kindFilter} onChange={setKindFilter} options={kindOptions} />
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.4fr) minmax(320px, 0.9fr)", gap: 16 }}>
          <div className="card">
            <div className="card-head">
              <h3>Visible Ops</h3>
              <span className="badge">{filtered.length}</span>
            </div>
            <div style={{ padding: 8 }}>
              {filtered.length === 0 ? (
                <div className="empty" style={{ padding: "28px 16px" }}>No ops match the current filters.</div>
              ) : (
                filtered.map((o) => (
                  <button
                    key={o.id}
                    onClick={() => setSelectedId(o.id)}
                    style={{
                      display: "block",
                      width: "100%",
                      textAlign: "left",
                      padding: 0,
                      background: selected?.id === o.id ? "rgba(217,119,54,0.06)" : "transparent",
                      border: selected?.id === o.id ? "1px solid rgba(217,119,54,0.24)" : "1px solid transparent",
                      borderRadius: 10,
                      marginBottom: 6,
                    }}
                  >
                    <OpRow op={o} />
                  </button>
                ))
              )}
            </div>
          </div>

          <div style={{ display: "grid", gap: 16, alignSelf: "start" }}>
            <div className="card">
              <div className="card-head">
                <h3>Op Detail</h3>
                {selected && <span className={`chip method ${selected.method !== "—" ? selected.method : "symlink"}`}>{selected.status}</span>}
              </div>
              <div className="card-body" style={{ fontSize: 12 }}>
                {selected ? (
                  <div className="kv" style={{ gridTemplateColumns: "110px 1fr" }}>
                    <div className="k">id</div>
                    <div className="v mono">{selected.id}</div>
                    <div className="k">kind</div>
                    <div className="v">{selected.kind}</div>
                    <div className="k">skill</div>
                    <div className="v">{selected.skill}</div>
                    <div className="k">target</div>
                    <div className="v mono">{selected.target}</div>
                    <div className="k">method</div>
                    <div className="v">{selected.method}</div>
                    <div className="k">time</div>
                    <div className="v">{selected.time}</div>
                    <div className="k">reason</div>
                    <div className="v">{selected.reason ?? "—"}</div>
                  </div>
                ) : (
                  <div className="empty" style={{ padding: "24px 12px" }}>Select an op to inspect its detail.</div>
                )}
              </div>
            </div>

            <div className="card">
              <div className="card-head">
                <h3>History Diagnose</h3>
                {diagnose?.conflicts && diagnose.conflicts.length > 0 ? (
                  <span className="badge" style={{ color: "var(--err)" }}>{diagnose.conflicts.length} conflicts</span>
                ) : (
                  <span className="badge ok">clean</span>
                )}
              </div>
              <div className="card-body" style={{ fontSize: 12 }}>
                {diagnose ? (
                  <>
                    <div className="kv" style={{ gridTemplateColumns: "140px 1fr" }}>
                      <div className="k">ahead / behind</div>
                      <div className="v mono">{diagnose.ahead ?? 0} / {diagnose.behind ?? 0}</div>
                      <div className="k">local segments</div>
                      <div className="v">{diagnose.local_segments ?? 0}</div>
                      <div className="k">local archives</div>
                      <div className="v">{diagnose.local_archives ?? 0}</div>
                      <div className="k">remote segments</div>
                      <div className="v">{diagnose.remote_segments ?? 0}</div>
                      <div className="k">remote archives</div>
                      <div className="v">{diagnose.remote_archives ?? 0}</div>
                      <div className="k">snapshots</div>
                      <div className="v">
                        local {diagnose.local_snapshot ? "yes" : "no"} · remote {diagnose.remote_snapshot ? "yes" : "no"}
                      </div>
                    </div>
                    {diagnose.conflicts && diagnose.conflicts.length > 0 && (
                      <div style={{ marginTop: 14, display: "grid", gap: 10 }}>
                        {diagnose.conflicts.slice(0, 5).map((conflict, index) => (
                          <div key={`${conflict.path ?? "conflict"}-${index}`} style={conflictStyle}>
                            <div className="mono" style={{ color: "var(--ink-0)", marginBottom: 4 }}>
                              {conflict.scope}:{conflict.path}
                            </div>
                            <div style={{ color: "var(--ink-2)" }}>
                              local {conflict.local_blob} · remote {conflict.remote_blob}
                            </div>
                            <div style={{ color: "var(--ink-3)", marginTop: 4 }}>
                              rename suggestions:
                              {" "}
                              <span className="mono">{conflict.local_rename_path}</span>
                              {" / "}
                              <span className="mono">{conflict.remote_rename_path}</span>
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </>
                ) : (
                  <div className="empty" style={{ padding: "24px 12px" }}>Run diagnose to inspect loom-history divergence and path conflicts.</div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

function FilterSelect({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: string[];
}) {
  return (
    <label style={{ display: "grid", gap: 6 }}>
      <span style={sectionLabel}>{label}</span>
      <select value={value} onChange={(e) => onChange(e.target.value)} style={filterSelectStyle}>
        <option value="all">all</option>
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </label>
  );
}

function MetricCard({
  label,
  value,
  sub,
  accent,
  children,
}: {
  label: string;
  value: string;
  sub: string;
  accent?: string;
  children?: ReactNode;
}) {
  return (
    <div className="card">
      <div className="card-body">
        <div style={sectionLabel}>{label}</div>
        <div style={{ fontFamily: "var(--font-display)", fontSize: 24, color: accent ?? "var(--ink-0)" }}>{value}</div>
        <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 10 }}>{sub}</div>
        {children}
      </div>
    </div>
  );
}

function filterButtonStyle(active: boolean): CSSProperties {
  return {
    background: active ? "var(--bg-2)" : "transparent",
    borderColor: active ? "var(--line-hi)" : "transparent",
    border: "1px solid",
    color: active ? "var(--ink-0)" : "var(--ink-2)",
  };
}

const sectionLabel = {
  fontSize: 10.5,
  color: "var(--ink-3)",
  letterSpacing: "0.1em",
  textTransform: "uppercase" as const,
  fontWeight: 500,
};

const filterSelectStyle: CSSProperties = {
  padding: "7px 10px",
  borderRadius: 8,
  border: "1px solid var(--line)",
  background: "var(--bg-1)",
  color: "var(--ink-0)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
};

const miniSelectStyle: CSSProperties = {
  ...filterSelectStyle,
  width: 160,
};

const conflictStyle: CSSProperties = {
  border: "1px solid var(--line)",
  background: "var(--bg-1)",
  borderRadius: 10,
  padding: "10px 12px",
};
