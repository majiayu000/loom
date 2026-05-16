import { useEffect, useMemo, useState } from "react";
import { RefreshIcon, ShieldIcon } from "../../components/icons/nav_icons";
import { api, ApiError, type DoctorCheck, type DoctorPayload } from "../../lib/api/client";
import type { PanelDataMode } from "../../lib/api/usePanelData";

interface DoctorPageProps {
  live: boolean;
  mode: PanelDataMode;
  refreshKey: string | null;
}

type DoctorState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: DoctorPayload }
  | { kind: "error"; message: string };

export function DoctorPage({ live, mode, refreshKey }: DoctorPageProps) {
  const [state, setState] = useState<DoctorState>({ kind: "idle" });
  const [manualTick, setManualTick] = useState(0);

  useEffect(() => {
    if (!live) {
      setState({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ kind: "loading" });
    api
      .workspaceDoctor(controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return;
        setState({ kind: "ready", payload });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setState({ kind: "error", message });
      });
    return () => controller.abort();
  }, [live, refreshKey, manualTick]);

  const checks = state.kind === "ready" ? state.payload.checks_v1 ?? [] : [];
  const failed = checks.filter((check) => !check.ok);
  const grouped = useMemo(() => groupChecks(checks), [checks]);

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Doctor</h1>
          <div className="subtitle">Live registry health from the workspace doctor contract.</div>
        </div>
        <div className="header-actions">
          <button className="btn ghost" onClick={() => setManualTick((cur) => cur + 1)} disabled={!live || state.kind === "loading"}>
            <RefreshIcon /> {state.kind === "loading" ? "Refreshing..." : "Refresh"}
          </button>
        </div>
      </div>
      <div className="page-body">
        {!live && (
          <div className="empty" style={{ marginBottom: 16 }}>
            {mode === "offline-stale"
              ? "Doctor is paused while the live API is offline."
              : "Doctor needs the live panel API."}
          </div>
        )}
        {state.kind === "error" && (
          <div className="empty" style={{ marginBottom: 16, color: "var(--err)" }}>
            {state.message}
          </div>
        )}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12, marginBottom: 18 }}>
          <Kpi
            label="Health"
            value={state.kind === "ready" ? (state.payload.healthy ? "healthy" : "attention") : state.kind}
            tone={state.kind === "ready" && !state.payload.healthy ? "err" : undefined}
          />
          <Kpi label="Checks" value={checks.length} />
          <Kpi label="Needs action" value={failed.length} tone={failed.length > 0 ? "pending" : undefined} />
        </div>

        {state.kind === "loading" && <div className="empty mono">loading...</div>}
        {state.kind === "ready" && checks.length === 0 && <div className="empty">No doctor checks returned.</div>}
        {state.kind === "ready" && checks.length > 0 && (
          <div style={{ display: "grid", gap: 12 }}>
            {grouped.map(([section, sectionChecks]) => (
              <div className="card" key={section}>
                <div className="card-head">
                  <h3>{sectionLabel(section)}</h3>
                  <span className={`chip ${sectionChecks.every((check) => check.ok) ? "ok" : "warn"}`}>
                    {sectionChecks.filter((check) => !check.ok).length} / {sectionChecks.length}
                  </span>
                </div>
                <div className="card-body" style={{ padding: 0 }}>
                  <table className="tbl" style={{ fontSize: 12 }}>
                    <tbody>
                      {sectionChecks.map((check) => (
                        <DoctorRow key={check.id} check={check} />
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

function DoctorRow({ check }: { check: DoctorCheck }) {
  return (
    <tr>
      <td style={{ width: 170 }}>
        <span className="row-flex">
          <ShieldIcon />
          <span className="mono dim">{check.id}</span>
        </span>
      </td>
      <td style={{ width: 96 }}>
        <span className={`chip ${check.ok ? "ok" : check.severity === "warning" ? "warn" : "danger"}`}>
          {check.ok ? "ok" : check.severity}
        </span>
      </td>
      <td>
        <div style={{ color: "var(--ink-1)" }}>{check.message}</div>
        {!check.ok && check.next_action && (
          <div style={{ color: "var(--ink-3)", marginTop: 3 }}>{check.next_action}</div>
        )}
      </td>
    </tr>
  );
}

function groupChecks(checks: DoctorCheck[]): Array<[string, DoctorCheck[]]> {
  const groups = new Map<string, DoctorCheck[]>();
  for (const check of checks) {
    const existing = groups.get(check.section);
    if (existing) existing.push(check);
    else groups.set(check.section, [check]);
  }
  return [...groups.entries()];
}

function sectionLabel(section: string): string {
  return section.replace(/_/g, " ");
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
