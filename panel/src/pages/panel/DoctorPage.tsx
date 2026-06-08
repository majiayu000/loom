import { useEffect, useMemo, useState } from "react";
import { RefreshIcon, ShieldIcon } from "../../components/icons/nav_icons";
import { api, ApiError, type DoctorCheck, type DoctorPayload } from "../../lib/api/client";
import type { PanelDataMode } from "../../lib/api/usePanelData";

interface DoctorPageProps {
  apiReachable: boolean;
  mode: PanelDataMode;
  refreshKey: string | null;
}

type DoctorState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: DoctorPayload }
  | { kind: "error"; message: string };

export function DoctorPage({ apiReachable, mode, refreshKey }: DoctorPageProps) {
  const [state, setState] = useState<DoctorState>({ kind: "idle" });
  const [manualTick, setManualTick] = useState(0);

  useEffect(() => {
    if (!apiReachable) {
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
  }, [apiReachable, refreshKey, manualTick]);

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
          <button className="btn ghost" onClick={() => setManualTick((cur) => cur + 1)} disabled={!apiReachable || state.kind === "loading"}>
            <RefreshIcon /> {state.kind === "loading" ? "Refreshing..." : "Refresh"}
          </button>
        </div>
      </div>
      <div className="page-body">
        {!apiReachable && (
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
            title="Aggregate doctor verdict across registry checks."
          />
          <Kpi
            label="Checks"
            value={checks.length}
            title="Total registry integrity probes executed by `loom workspace doctor`"
          />
          <Kpi
            label="Needs action"
            value={failed.length}
            tone={failed.length > 0 ? "pending" : undefined}
            title="Failed doctor checks. Independent from pending sync."
          />
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
                    {sectionChecks.filter((check) => !check.ok).length} issues / {sectionChecks.length} checks
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
  const label = doctorCheckLabel(check);
  const context = doctorCheckContext(check);

  return (
    <tr>
      <td style={{ width: 170 }}>
        <span className="row-flex doctor-check-label">
          <ShieldIcon />
          <span>
            <span className="doctor-check-name">{label}</span>
            <span className="mono dim doctor-check-id">{context ? `${check.id} · ${context}` : check.id}</span>
          </span>
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
        {check.id === "agent_skill_inventory" && <AgentInventoryDetails details={check.details} />}
      </td>
    </tr>
  );
}

type AgentInventoryRow = {
  agent: string;
  default_path: string;
  present: boolean;
  registered_targets?: Array<{ target_id?: string; ownership?: string }>;
};

function AgentInventoryDetails({ details }: { details?: Record<string, unknown> }) {
  const agents = Array.isArray(details?.agents)
    ? (details.agents.filter(isAgentInventoryRow) as AgentInventoryRow[])
    : [];
  if (agents.length === 0) return null;

  return (
    <div style={{ marginTop: 8, display: "grid", gap: 6 }}>
      {agents.map((agent) => (
        <div
          key={agent.agent}
          style={{
            display: "grid",
            gridTemplateColumns: "92px minmax(0, 1fr) 92px",
            gap: 8,
            alignItems: "center",
          }}
        >
          <span className="mono">{agent.agent}</span>
          <span className="mono dim" title={agent.default_path} style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
            {agent.default_path}
          </span>
          <span className={`chip ${agent.present ? "ok" : "warn"}`}>
            {agent.present ? "present" : "missing"}
          </span>
          {agent.registered_targets && agent.registered_targets.length > 0 && (
            <div style={{ gridColumn: "2 / 4", display: "flex", gap: 6, flexWrap: "wrap" }}>
              {agent.registered_targets.map((target) => (
                <span className="chip" key={target.target_id ?? `${agent.agent}-${target.ownership}`}>
                  {target.ownership ?? "target"} · {target.target_id ?? "unknown"}
                </span>
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function isAgentInventoryRow(value: unknown): value is AgentInventoryRow {
  if (!value || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.agent === "string" &&
    typeof row.default_path === "string" &&
    typeof row.present === "boolean"
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

function doctorCheckLabel(check: DoctorCheck): string {
  const idRoot = check.id.split(":")[0];
  const labels: Record<string, string> = {
    agent_skill_inventory: "Agent skill directories",
    binding_target_agent_match: "Binding target agent",
    binding_target_exists: "Binding default target",
    git_fsck: "Git integrity",
    history_branch: "History branch",
    pending_queue_warnings: "Pending queue warnings",
    projection_path_exists: "Projection path",
    projection_source_exists: "Projection source skill",
    schema_file: "Registry schema file",
    snapshot_load: "Registry snapshot",
    target_path_exists: "Target path",
  };
  return labels[idRoot] ?? titleize(idRoot);
}

function doctorCheckContext(check: DoctorCheck): string | null {
  const detailKeys = ["target_id", "binding_id", "instance_id", "skill_id", "path"];
  for (const key of detailKeys) {
    const value = check.details?.[key];
    if (typeof value === "string" && value.trim()) return value;
  }

  const [, suffix] = check.id.split(":", 2);
  return suffix || null;
}

function titleize(value: string): string {
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function Kpi({
  label,
  value,
  tone,
  title,
}: {
  label: string;
  value: string | number;
  tone?: "pending" | "err";
  title?: string;
}) {
  const color = tone === "pending" ? "var(--pending)" : tone === "err" ? "var(--err)" : "var(--ink-0)";
  return (
    <div className="kpi" title={title}>
      <div className="label">{label}</div>
      <div className="value" style={{ color }}>
        {value}
      </div>
    </div>
  );
}
