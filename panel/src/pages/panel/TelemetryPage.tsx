import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { RefreshIcon } from "../../components/icons/nav_icons";
import {
  api,
  ApiError,
  type TelemetryAggregate,
  type TelemetryReportPayload,
} from "../../lib/api/client";
import type { PanelDataMode } from "../../lib/api/usePanelData";

interface TelemetryPageProps {
  apiReachable: boolean;
  mode: PanelDataMode;
  refreshKey: string | null;
}

type TelemetryState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; report: TelemetryReportPayload; warnings: string[] }
  | { kind: "error"; message: string };

interface SkillTelemetryRow {
  skill: string;
  aggregate: TelemetryAggregate;
  costUnits: number;
  riskEvents: number;
  staleDays: number | null;
}

interface ScatterPoint {
  skill: string;
  x: number;
  y: number;
  label: string;
}

export function TelemetryPage({ apiReachable, mode, refreshKey }: TelemetryPageProps) {
  const [state, setState] = useState<TelemetryState>({ kind: "idle" });
  const [manualTick, setManualTick] = useState(0);

  useEffect(() => {
    if (!apiReachable) {
      setState({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ kind: "loading" });
    api
      .telemetryReportWithWarnings(controller.signal)
      .then((result) => {
        if (controller.signal.aborted) return;
        setState({ kind: "ready", report: result.data, warnings: result.warnings });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setState({ kind: "error", message });
      });
    return () => controller.abort();
  }, [apiReachable, mode, refreshKey, manualTick]);

  const report = state.kind === "ready" ? state.report : null;
  const rows = useMemo(() => (report ? skillRows(report) : []), [report]);
  const scatterPoints = useMemo(() => usageValuePoints(rows), [rows]);
  const staleRows = rows
    .filter((row) => row.staleDays !== null)
    .sort((a, b) => (b.staleDays ?? 0) - (a.staleDays ?? 0))
    .slice(0, 5);
  const riskRows = rows
    .filter((row) => row.riskEvents > 0)
    .sort((a, b) => b.riskEvents - a.riskEvents)
    .slice(0, 5);
  const costRows = rows
    .filter((row) => row.costUnits > 0)
    .sort((a, b) => b.costUnits - a.costUnits)
    .slice(0, 5);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Telemetry</h1>
          <div className="subtitle">Local usage, value, cost, drift, and risk from the telemetry report API.</div>
        </div>
        <div className="header-actions">
          <button className="btn ghost" onClick={() => setManualTick((cur) => cur + 1)} disabled={!apiReachable || state.kind === "loading"}>
            <RefreshIcon /> {state.kind === "loading" ? "Refreshing..." : "Refresh"}
          </button>
        </div>
      </div>
      <div className="page-body">
        {!apiReachable && (
          <div className="empty" style={{ marginBottom: 16 }}>
            {mode === "offline-stale" ? "Telemetry is paused while the live API is offline." : "Telemetry needs the live panel API."}
          </div>
        )}
        {state.kind === "error" && (
          <div className="empty" style={{ marginBottom: 16, color: "var(--err)" }}>
            {state.message}
          </div>
        )}
        {state.kind === "loading" && <div className="empty mono">loading...</div>}
        {state.kind === "ready" && (
          <>
            {state.warnings.length > 0 && (
              <div className="empty" style={{ marginBottom: 16, color: "var(--warn)" }}>
                {state.warnings.slice(0, 3).join(" · ")}
              </div>
            )}
            <div className="kpi-row" style={{ marginBottom: 16 }}>
              <Kpi label="Telemetry" value={report!.enabled ? "enabled" : "disabled"} meta={report!.mode} tone={report!.enabled ? "ok" : "warn"} />
              <Kpi label="Matched events" value={report!.matched_events} meta={`${report!.events_total} stored`} />
              <Kpi label="Eval pass rate" value={formatPercent(report!.summary.value.pass_rate)} meta={`${report!.summary.value.eval_runs} eval runs`} tone={toneForStatus(report!.summary.value.status)} />
              <Kpi label="Cost" value={formatNumber(report!.summary.cost.tokens_in + report!.summary.cost.tokens_out)} meta={`${formatNumber(report!.summary.cost.commands)} commands`} tone={toneForStatus(report!.summary.cost.status)} />
              <Kpi label="Risk" value={formatNumber(report!.summary.risk.safety_findings + report!.summary.risk.dependency_findings)} meta={`${formatNumber(report!.summary.risk.safety_events)} safety events`} tone={toneForStatus(report!.summary.risk.status)} />
              <Kpi label="Drift" value={formatDays(report!.summary.drift.stale_eval_days)} meta={report!.summary.drift.last_successful_eval_at ?? "last eval missing"} tone={toneForStatus(report!.summary.drift.status)} />
              <Kpi label="Feedback" value={formatNumber(feedbackTotal(report!.summary))} meta={feedbackMeta(report!.summary)} tone={toneForStatus(report!.summary.recommendation_feedback.status)} />
            </div>

            {rows.length === 0 ? (
              <div className="empty">No telemetry events matched the current report.</div>
            ) : (
              <div style={{ display: "grid", gap: 16 }}>
                <div className="card">
                  <div className="card-head">
                    <h3>Skill health</h3>
                    <span className="chip">{rows.length} skills</span>
                  </div>
                  <div className="card-body" style={{ padding: 0 }}>
                    <table className="tbl mobile-cards" style={{ fontSize: 12 }}>
                      <thead>
                        <tr>
                          <th>Skill</th>
                          <th>Events</th>
                          <th>Value</th>
                          <th>Cost</th>
                          <th>Risk</th>
                          <th>Drift</th>
                        </tr>
                      </thead>
                      <tbody>
                        {rows.map((row) => (
                          <SkillTelemetryTableRow key={row.skill} row={row} />
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>

                <div className="card">
                  <div className="card-head">
                    <h3>Usage vs value</h3>
                    <span className="chip">{scatterPoints.length} points</span>
                  </div>
                  <div className="card-body">
                    {scatterPoints.length === 0 ? (
                      <div className="empty" style={{ padding: "24px 12px" }}>No eval delta telemetry in this report.</div>
                    ) : (
                      <div
                        role="img"
                        aria-label="Usage vs eval delta scatterplot"
                        style={{
                          position: "relative",
                          height: 220,
                          border: "1px solid var(--line)",
                          borderRadius: "var(--radius)",
                          background: "var(--bg-1)",
                          overflow: "hidden",
                        }}
                      >
                        <div style={{ position: "absolute", left: 12, top: 10, color: "var(--ink-3)", fontSize: 11 }}>higher value</div>
                        <div style={{ position: "absolute", right: 12, bottom: 10, color: "var(--ink-3)", fontSize: 11 }}>more usage</div>
                        <div style={{ position: "absolute", inset: "34px 18px 28px 34px", borderLeft: "1px solid var(--line-hi)", borderBottom: "1px solid var(--line-hi)" }} />
                        {scatterPoints.map((point) => (
                          <span
                            key={point.skill}
                            title={point.label}
                            style={{
                              position: "absolute",
                              left: `${point.x}%`,
                              bottom: `${point.y}%`,
                              transform: "translate(-50%, 50%)",
                              maxWidth: 120,
                              padding: "3px 6px",
                              border: "1px solid var(--line-hi)",
                              borderRadius: "var(--radius-sm)",
                              background: "var(--bg-3)",
                              color: "var(--ink-0)",
                              fontFamily: "var(--font-mono)",
                              fontSize: 10.5,
                              whiteSpace: "nowrap",
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                            }}
                          >
                            {point.skill}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                </div>

                <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))", gap: 16 }}>
                  <AttentionCard title="High-risk active skills" rows={riskRows} empty="No risk telemetry in this report." metric={(row) => `${row.riskEvents} risk signals`} />
                  <AttentionCard title="Stale evals" rows={staleRows} empty="No successful eval age telemetry." metric={(row) => formatDays(row.staleDays)} />
                  <AttentionCard title="High overhead" rows={costRows} empty="No cost telemetry in this report." metric={(row) => `${formatNumber(row.costUnits)} units`} />
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </>
  );
}

function skillRows(report: TelemetryReportPayload): SkillTelemetryRow[] {
  return Object.entries(report.skills ?? {})
    .map(([skill, aggregate]) => ({
      skill,
      aggregate,
      costUnits: aggregate.cost.tokens_in + aggregate.cost.tokens_out + aggregate.cost.commands,
      riskEvents: aggregate.risk.safety_events + aggregate.risk.safety_findings + aggregate.risk.dependency_findings,
      staleDays: aggregate.drift.stale_eval_days,
    }))
    .sort((a, b) => b.aggregate.events - a.aggregate.events || a.skill.localeCompare(b.skill));
}

function usageValuePoints(rows: SkillTelemetryRow[]): ScatterPoint[] {
  const withValue = rows
    .map((row) => ({ row, value: row.aggregate.value.baseline_delta_avg }))
    .filter((entry): entry is { row: SkillTelemetryRow; value: number } => typeof entry.value === "number");
  if (withValue.length === 0) return [];
  const maxEvents = Math.max(...withValue.map((entry) => entry.row.aggregate.events), 1);
  return withValue.slice(0, 12).map(({ row, value }) => {
    const x = 12 + (row.aggregate.events / maxEvents) * 76;
    const y = 12 + ((Math.max(-1, Math.min(1, value)) + 1) / 2) * 76;
    return {
      skill: row.skill,
      x,
      y,
      label: `${row.skill} · ${row.aggregate.events} events · delta ${formatDelta(value)}`,
    };
  });
}

function SkillTelemetryTableRow({ row }: { row: SkillTelemetryRow }) {
  return (
    <tr>
      <td data-label="Skill">
        <span className="name">{row.skill}</span>
      </td>
      <td data-label="Events" className="mono">{row.aggregate.events}</td>
      <td data-label="Value">
        <MetricStatus status={row.aggregate.value.status}>
          {row.aggregate.value.eval_runs} evals · {formatPercent(row.aggregate.value.pass_rate)}
        </MetricStatus>
      </td>
      <td data-label="Cost">
        <MetricStatus status={row.aggregate.cost.status}>
          {formatNumber(row.aggregate.cost.tokens_in + row.aggregate.cost.tokens_out)} tokens · {formatNumber(row.aggregate.cost.commands)} commands
        </MetricStatus>
      </td>
      <td data-label="Risk">
        <MetricStatus status={row.aggregate.risk.status}>
          {formatNumber(row.riskEvents)} signals
        </MetricStatus>
      </td>
      <td data-label="Drift">
        <MetricStatus status={row.aggregate.drift.status}>{formatDays(row.staleDays)}</MetricStatus>
      </td>
    </tr>
  );
}

function AttentionCard({
  title,
  rows,
  empty,
  metric,
}: {
  title: string;
  rows: SkillTelemetryRow[];
  empty: string;
  metric: (row: SkillTelemetryRow) => string;
}) {
  return (
    <div className="card">
      <div className="card-head">
        <h3>{title}</h3>
        <span className="chip">{rows.length}</span>
      </div>
      <div className="card-body" style={{ display: "grid", gap: 8 }}>
        {rows.length === 0 ? (
          <div className="empty" style={{ padding: "24px 12px" }}>{empty}</div>
        ) : (
          rows.map((row) => (
            <div key={`${title}-${row.skill}`} className="row-flex" style={{ justifyContent: "space-between", gap: 12 }}>
              <span className="mono" style={{ minWidth: 0, overflowWrap: "anywhere" }}>{row.skill}</span>
              <span className="chip">{metric(row)}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function MetricStatus({ status, children }: { status: string; children: ReactNode }) {
  return (
    <span className="row-flex" style={{ gap: 6, alignItems: "center" }}>
      <span className={`chip ${status === "available" ? "ok" : "warn"}`}>{status}</span>
      <span>{children}</span>
    </span>
  );
}

type KpiTone = "ok" | "warn" | "err";

function Kpi({
  label,
  value,
  meta,
  tone = "ok",
}: {
  label: string;
  value: ReactNode;
  meta: ReactNode;
  tone?: KpiTone;
}) {
  return (
    <div className="kpi" data-tone={tone}>
      <div className="label">{label}</div>
      <div className="value">{value}</div>
      <div className="meta">{meta}</div>
    </div>
  );
}

function toneForStatus(status: string): KpiTone {
  return status === "available" ? "ok" : "warn";
}

function formatPercent(value: number | null): string {
  return typeof value === "number" ? `${Math.round(value * 100)}%` : "missing";
}

function formatDays(value: number | null): string {
  return typeof value === "number" ? `${value}d` : "missing";
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatDelta(value: number): string {
  return `${value >= 0 ? "+" : ""}${value.toFixed(3)}`;
}

function feedbackTotal(aggregate: TelemetryAggregate): number {
  return (
    aggregate.recommendation_feedback.accepted +
    aggregate.recommendation_feedback.rejected +
    aggregate.recommendation_feedback.ignored
  );
}

function feedbackMeta(aggregate: TelemetryAggregate): string {
  const feedback = aggregate.recommendation_feedback;
  return `${feedback.accepted} accepted · ${feedback.rejected} rejected · ${feedback.ignored} ignored`;
}
