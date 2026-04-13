import { pick } from "../i18n";
import type { Locale } from "../i18n";

export function ShellStat({
  label,
  locale,
  value,
  tone,
  foot,
}: {
  label: string;
  locale: Locale;
  value: string | number;
  tone?: string;
  foot?: string;
}) {
  return (
    <div className={`card stat-card ${tone ?? ""}`}>
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      <div className="stat-foot">
        <span className={`metric-chip ${tone ?? ""}`}>{foot ?? pick(locale, "stable", "稳定")}</span>
      </div>
    </div>
  );
}
