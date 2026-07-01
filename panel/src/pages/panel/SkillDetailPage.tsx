import { useEffect, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { api, type SkillInspectPayload, type SkillInspectRuntimeStatus } from "../../lib/api/client";

interface SkillDetailPageProps {
  skillName: string;
}

type Tone = "ok" | "warn" | "err" | "muted";

export function SkillDetailPage({ skillName }: SkillDetailPageProps) {
  const [inspect, setInspect] = useState<SkillInspectPayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    setInspect(null);
    api
      .skillInspect(skillName, ctrl.signal)
      .then((payload) => {
        if (ctrl.signal.aborted) return;
        setInspect(payload);
        setLoading(false);
      })
      .catch((err: Error) => {
        if (err.name === "AbortError") return;
        setError(err.message);
        setLoading(false);
      });
    return () => ctrl.abort();
  }, [skillName]);

  if (loading) {
    return <div style={emptyStyle}>Loading skill inspect...</div>;
  }
  if (error) {
    return <div style={errorStyle}>{error}</div>;
  }
  if (!inspect) {
    return <div style={emptyStyle}>No inspect data.</div>;
  }

  return <SkillInspectSections inspect={inspect} />;
}

export function SkillInspectSections({ inspect }: { inspect: SkillInspectPayload }) {
  const runtimeEntries = Object.entries(inspect.runtime);
  const evalEvidence = hasEvalEvidence(inspect);
  const safetyEvidence = hasSafetyEvidence(inspect);
  const specFindings = inspect.spec.findings ?? [];

  return (
    <div style={containerStyle} aria-label="Skill inspect detail">
      <InspectSection title="Source">
        <div className="kv" style={{ margin: 0 }}>
          <KeyValue label="Path" value={inspect.source.path} />
          <KeyValue label="Entrypoint" value={inspect.source.entrypoint ?? "missing"} />
          <KeyValue label="Exists" value={yesNo(inspect.source.exists)} />
          <KeyValue label="Git drift" value={inspect.source.working_tree_drift ? "drifted" : "clean"} />
          <KeyValue label="Head tree" value={inspect.source.head_tree_oid ?? "not recorded"} />
        </div>
      </InspectSection>

      <InspectSection title="Spec compatibility">
        <div style={chipRowStyle}>
          <StatusChip label={`portable ${inspect.spec.portable}`} tone={statusTone(inspect.spec.portable)} />
          <StatusChip label={`codex ${inspect.spec.codex}`} tone={statusTone(inspect.spec.codex)} />
          <StatusChip label={`claude ${inspect.spec.claude}`} tone={statusTone(inspect.spec.claude)} />
        </div>
        {specFindings.length > 0 ? (
          <FindingList findings={specFindings} />
        ) : (
          <div style={emptyStyle}>No spec findings.</div>
        )}
      </InspectSection>

      <InspectSection title="Runtime visibility">
        {runtimeEntries.length > 0 ? (
          <div style={runtimeListStyle}>
            {runtimeEntries.map(([agent, status]) => (
              <RuntimeRow key={agent} agent={agent} status={status} />
            ))}
          </div>
        ) : (
          <div style={emptyStyle}>No runtime targets reported.</div>
        )}
      </InspectSection>

      <InspectSection title="Quality and eval">
        {evalEvidence ? (
          <div className="kv" style={{ margin: 0 }}>
            <KeyValue label="Last eval" value={inspect.quality.last_eval ?? "not recorded"} />
            <KeyValue label="Precision" value={formatMetric(inspect.quality.trigger_precision)} />
            <KeyValue label="Recall" value={formatMetric(inspect.quality.trigger_recall)} />
            <KeyValue label="Baseline delta" value={formatMetric(inspect.quality.baseline_delta)} />
          </div>
        ) : (
          <div style={emptyStyle}>No eval evidence recorded.</div>
        )}
      </InspectSection>

      <InspectSection title="Safety and trust">
        {safetyEvidence ? (
          <div className="kv" style={{ margin: 0 }}>
            <KeyValue label="Trust" value={inspect.safety.trust} />
            <KeyValue label="Policy" value={inspect.safety.policy} />
            <KeyValue label="Scripts" value={formatOptionalBool(inspect.safety.scripts_present)} />
            <KeyValue label="Network" value={formatOptionalBool(inspect.safety.network_requested)} />
            <KeyValue label="Quarantine" value={yesNo(Boolean(inspect.safety.quarantined))} />
            <KeyValue label="Reason" value={inspect.safety.reason ?? "none"} />
          </div>
        ) : (
          <div style={emptyStyle}>No safety scan evidence recorded.</div>
        )}
      </InspectSection>

      <InspectSection title="Next actions">
        {inspect.next_actions.length > 0 ? (
          <CommandList commands={inspect.next_actions} />
        ) : (
          <div style={emptyStyle}>No next actions.</div>
        )}
      </InspectSection>
    </div>
  );
}

function InspectSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section style={sectionStyle}>
      <div className="section-title" style={{ marginTop: 0 }}>{title}</div>
      {children}
    </section>
  );
}

function RuntimeRow({ agent, status }: { agent: string; status: SkillInspectRuntimeStatus }) {
  const state = runtimeState(status);
  return (
    <div style={runtimeRowStyle}>
      <div style={{ minWidth: 0 }}>
        <div style={rowTitleStyle}>{agent}</div>
        <div className="mono" style={rowPathStyle}>
          {status.target_path ?? status.materialized_path ?? "no target path"}
        </div>
        <div style={chipRowStyle}>
          <StatusChip label={state.label} tone={state.tone} />
          <StatusChip label={`truth ${status.truth_level}`} tone="muted" />
          {status.health && <StatusChip label={`health ${status.health}`} tone={statusTone(status.health)} />}
        </div>
      </div>
      {status.findings.length > 0 && <FindingList findings={status.findings} compact />}
    </div>
  );
}

function FindingList({
  findings,
  compact = false,
}: {
  findings: Array<{ id: string; severity: string; message: string; next_action?: string | null; suggested_action?: string | null }>;
  compact?: boolean;
}) {
  return (
    <div style={compact ? compactFindingListStyle : findingListStyle}>
      {findings.map((finding) => (
        <div key={`${finding.id}:${finding.message}`} style={findingStyle}>
          <StatusChip label={finding.severity} tone={statusTone(finding.severity)} />
          <div style={{ minWidth: 0 }}>
            <div style={findingMessageStyle}>{finding.message}</div>
            {(finding.next_action || finding.suggested_action) && (
              <code style={commandStyle}>{finding.next_action ?? finding.suggested_action}</code>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

function CommandList({ commands }: { commands: string[] }) {
  return (
    <div style={commandListStyle}>
      {commands.map((command) => (
        <code key={command} style={commandStyle}>{command}</code>
      ))}
    </div>
  );
}

function KeyValue({ label, value }: { label: string; value: string }) {
  return (
    <>
      <div className="k">{label}</div>
      <div className="v" style={{ overflowWrap: "anywhere" }}>{value}</div>
    </>
  );
}

function StatusChip({ label, tone }: { label: string; tone: Tone }) {
  return <span style={{ ...chipStyle, ...toneStyle[tone] }}>{label}</span>;
}

function runtimeState(status: SkillInspectRuntimeStatus): { label: string; tone: Tone } {
  if (matchesStatus(status.enabled_by_agent_config, ["disabled", "disabled-by-config", "false"])) {
    return { label: "disabled-by-config", tone: "err" };
  }
  if (matchesStatus(status.restart_required, ["true", "required", "restart-required", "needs-restart"])) {
    return { label: "needs-restart", tone: "warn" };
  }
  if (status.active_rule_present && !status.projected_to_target) {
    return { label: "missing projection", tone: "warn" };
  }
  if (status.projected_to_target || matchesStatus(status.visible_to_agent, ["visible", "true"])) {
    return { label: "visible", tone: "ok" };
  }
  if (matchesStatus(status.visible_to_agent, ["not_checked"])) {
    return { label: "not checked", tone: "muted" };
  }
  return { label: status.visible_to_agent || "unknown", tone: "muted" };
}

function statusTone(status: string): Tone {
  const normalized = status.toLowerCase();
  if (["pass", "ok", "healthy", "verified", "visible", "clean"].includes(normalized)) return "ok";
  if (["error", "blocked", "fail", "missing", "quarantined"].includes(normalized)) return "err";
  if (["warning", "warn", "unknown", "attention", "drifted"].includes(normalized)) return "warn";
  return "muted";
}

function matchesStatus(value: string | undefined, options: string[]): boolean {
  return Boolean(value && options.includes(value.toLowerCase()));
}

function hasEvalEvidence(inspect: SkillInspectPayload): boolean {
  return Object.values(inspect.quality).some((value) => value !== null && value !== undefined && value !== "");
}

function hasSafetyEvidence(inspect: SkillInspectPayload): boolean {
  return (
    inspect.safety.trust !== "unknown" ||
    inspect.safety.policy !== "unknown" ||
    inspect.safety.quarantined === true ||
    Boolean(inspect.safety.reason) ||
    Boolean(inspect.safety.updated_at)
  );
}

function formatMetric(value: number | null | undefined): string {
  return typeof value === "number" ? value.toFixed(3) : "not recorded";
}

function formatOptionalBool(value: boolean | null | undefined): string {
  return value === null || value === undefined ? "not checked" : yesNo(value);
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

const containerStyle: CSSProperties = { display: "grid", gap: 14, marginBottom: 14 };
const sectionStyle: CSSProperties = { borderBottom: "1px solid var(--line-soft)", paddingBottom: 12 };
const runtimeListStyle: CSSProperties = { display: "grid", gap: 10 };
const runtimeRowStyle: CSSProperties = { display: "grid", gap: 8, padding: "10px 0" };
const rowTitleStyle: CSSProperties = { fontSize: 13, color: "var(--ink-0)", fontWeight: 600 };
const rowPathStyle: CSSProperties = { fontSize: 10.5, color: "var(--ink-3)", overflowWrap: "anywhere", margin: "2px 0 7px" };
const chipRowStyle: CSSProperties = { display: "flex", flexWrap: "wrap", gap: 6 };
const chipStyle: CSSProperties = {
  display: "inline-flex",
  border: "1px solid var(--line-hi)",
  borderRadius: 10,
  padding: "2px 8px",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  whiteSpace: "nowrap",
};
const toneStyle: Record<Tone, CSSProperties> = {
  ok: { color: "var(--ok)", borderColor: "rgba(111, 183, 138, 0.32)", background: "rgba(111, 183, 138, 0.08)" },
  warn: { color: "var(--warn)", borderColor: "rgba(213, 172, 85, 0.34)", background: "rgba(213, 172, 85, 0.08)" },
  err: { color: "var(--err)", borderColor: "rgba(216, 90, 90, 0.34)", background: "rgba(216, 90, 90, 0.08)" },
  muted: { color: "var(--ink-2)", borderColor: "var(--line-hi)", background: "var(--bg-2)" },
};
const findingListStyle: CSSProperties = { display: "grid", gap: 8, marginTop: 10 };
const compactFindingListStyle: CSSProperties = { ...findingListStyle, marginTop: 0 };
const findingStyle: CSSProperties = { display: "grid", gridTemplateColumns: "max-content minmax(0, 1fr)", gap: 8, alignItems: "start" };
const findingMessageStyle: CSSProperties = { color: "var(--ink-1)", fontSize: 12, lineHeight: 1.45 };
const commandListStyle: CSSProperties = { display: "grid", gap: 8 };
const commandStyle: CSSProperties = {
  display: "block",
  border: "1px solid var(--line-soft)",
  borderRadius: 6,
  padding: "6px 8px",
  background: "var(--bg-2)",
  color: "var(--ink-1)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  overflowWrap: "anywhere",
};
const emptyStyle: CSSProperties = { color: "var(--ink-3)", fontSize: 12 };
const errorStyle: CSSProperties = { color: "var(--err)", fontFamily: "var(--font-mono)", fontSize: 11, overflowWrap: "anywhere" };
