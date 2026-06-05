import type { SkillDiagnoseCheck, SkillDiagnosePayload } from "../../lib/api/client";

export function SkillDiagnosePanel({
  loading,
  error,
  diagnose,
}: {
  loading: boolean;
  error: string | null;
  diagnose: SkillDiagnosePayload | null;
}) {
  if (loading) {
    return <div style={{ color: "var(--ink-3)", fontSize: 12 }}>Loading...</div>;
  }
  if (error) {
    return (
      <div style={{ color: "var(--err)", fontSize: 11, fontFamily: "var(--font-mono)" }}>
        {error}
      </div>
    );
  }
  if (!diagnose) {
    return <div className="empty" style={{ padding: "18px 4px" }}>No diagnose data loaded.</div>;
  }

  const grouped = groupDiagnoseChecks(diagnose.checks);
  const failed = diagnose.summary.failed_check_count;
  const warnings = diagnose.summary.warning_check_count;

  return (
    <div style={{ display: "grid", gap: 12 }}>
      <div className="card">
        <div className="card-head">
          <h3>Diagnose</h3>
          <span className={`chip ${statusChipClass(diagnose.status)}`}>{diagnose.status}</span>
        </div>
        <div
          className="card-body"
          style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 10 }}
        >
          <MiniStat label="Failed" value={failed} tone={failed > 0 ? "err" : "ok"} />
          <MiniStat label="Warnings" value={warnings} tone={warnings > 0 ? "warn" : "ok"} />
          <MiniStat label="Checks" value={diagnose.checks.length} />
        </div>
      </div>

      {diagnose.checks.length === 0 ? (
        <div className="empty" style={{ padding: "18px 4px" }}>No diagnose checks returned.</div>
      ) : (
        grouped.map(([section, checks]) => (
          <div className="card" key={section}>
            <div className="card-head">
              <h3>{sectionLabel(section)}</h3>
              <span className={`chip ${checks.every((check) => check.ok) ? "present" : "missing"}`}>
                {checks.filter((check) => !check.ok).length} / {checks.length}
              </span>
            </div>
            <div className="card-body" style={{ padding: 0 }}>
              <table className="tbl" style={{ fontSize: 12 }}>
                <tbody>
                  {checks.map((check) => (
                    <DiagnoseCheckRow key={check.id} check={check} />
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        ))
      )}
    </div>
  );
}

function DiagnoseCheckRow({ check }: { check: SkillDiagnoseCheck }) {
  return (
    <tr>
      <td style={{ width: 190 }}>
        <span className="mono dim">{check.id}</span>
      </td>
      <td style={{ width: 96 }}>
        <span className={`chip ${severityChipClass(check)}`}>
          {check.ok ? "ok" : check.severity}
        </span>
      </td>
      <td>
        <div style={{ color: "var(--ink-1)" }}>{check.message}</div>
        {!check.ok && check.next_action && (
          <div className="mono" style={{ color: "var(--ink-3)", marginTop: 3 }}>
            next_action: {check.next_action}
          </div>
        )}
      </td>
    </tr>
  );
}

function MiniStat({
  label,
  value,
  tone,
}: {
  label: string;
  value: string | number;
  tone?: "ok" | "warn" | "err";
}) {
  const color = tone === "ok" ? "var(--ok)" : tone === "warn" ? "var(--warn)" : tone === "err" ? "var(--err)" : "var(--ink-0)";
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value" style={{ color }}>
        {value}
      </div>
    </div>
  );
}

function groupDiagnoseChecks(checks: SkillDiagnoseCheck[]): Array<[string, SkillDiagnoseCheck[]]> {
  const groups = new Map<string, SkillDiagnoseCheck[]>();
  for (const check of checks) {
    const existing = groups.get(check.section);
    if (existing) existing.push(check);
    else groups.set(check.section, [check]);
  }
  return [...groups.entries()];
}

function statusChipClass(status: string): string {
  if (status === "healthy") return "present";
  if (status === "attention") return "missing";
  if (status === "blocked") return "non-compliant";
  return "";
}

function severityChipClass(check: SkillDiagnoseCheck): string {
  if (check.ok || check.severity === "ok") return "present";
  if (check.severity === "warning") return "missing";
  return "non-compliant";
}

function sectionLabel(section: string): string {
  return section.replace(/_/g, " ");
}
