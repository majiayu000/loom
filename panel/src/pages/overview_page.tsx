import { formatTime, pick, syncStateLabel } from "../i18n";
import type { Locale } from "../i18n";
import { ShellStat } from "../components/shell_stat";
import { Icon } from "../components/icon";
import {
  formatSyncTone,
  projectionIsDrifted,
  summarizeDetails,
} from "../lib/panel_data";
import { filterOverviewOps, filterOverviewWarnings } from "../lib/panel_selectors";
import type { PanelData, SkillView } from "../types";

export function OverviewPage({
  data,
  locale,
  skillViews,
  query,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
  query: string;
}) {
  const drifted = data.v3.projections.filter(projectionIsDrifted);
  const activeBindings = data.v3.bindings.filter((binding) => binding.active);
  const activeWarnings = filterOverviewWarnings([...data.remoteWarnings, ...(data.pending.warnings ?? [])], query);
  const diagnosticLines = filterOverviewOps(data.pending.ops, query);

  return (
    <div className="page-body page-frame">
      <div className="page-head">
        <div>
          <h1 className="page-title">{pick(locale, "Operational Overview", "运行总览")}</h1>
          <p className="page-subtitle">
            {pick(
              locale,
              "Current control-plane health, sync posture, drift impact, and pending local work.",
              "查看当前控制平面的健康状态、同步姿态、漂移影响与待处理本地操作。",
            )}
          </p>
        </div>
        <div className="page-actions">
          <span className={`status-pill ${formatSyncTone(data.remote.sync_state)}`}>
            <Icon name="sync" />
            {syncStateLabel(locale, data.remote.sync_state)}
          </span>
          <span className={`status-pill ${data.live ? "is-success" : "is-warning"}`}>
            <Icon name={data.live ? "hub" : "science"} />
            {data.live ? pick(locale, "api live", "API 已连接") : pick(locale, "api unavailable", "API 不可用")}
          </span>
        </div>
      </div>

      <div className="stats-grid">
        <ShellStat
          label={pick(locale, "Total Skills", "技能总数")}
          locale={locale}
          value={skillViews.length}
          tone="is-primary"
          foot={locale === "zh-CN" ? `${data.v3.counts.skills || skillViews.length} 已映射` : `${data.v3.counts.skills || skillViews.length} mapped`}
        />
        <ShellStat
          label={pick(locale, "Active Bindings", "活跃绑定")}
          locale={locale}
          value={activeBindings.length}
          tone="is-success"
          foot={locale === "zh-CN" ? `${data.v3.bindings.length} 总计` : `${data.v3.bindings.length} total`}
        />
        <ShellStat
          label={pick(locale, "Managed Targets", "托管目标")}
          locale={locale}
          value={data.v3.targets.filter((target) => target.ownership === "managed").length}
          tone="is-success"
          foot={locale === "zh-CN" ? `${data.v3.targets.length} 已注册` : `${data.v3.targets.length} registered`}
        />
        <ShellStat
          label={pick(locale, "Drifted Projections", "漂移投影")}
          locale={locale}
          value={drifted.length}
          tone={drifted.length > 0 ? "is-danger" : "is-success"}
          foot={locale === "zh-CN" ? `${data.v3.projections.length} 总投影` : `${data.v3.projections.length} total`}
        />
        <ShellStat
          label={pick(locale, "Pending Ops", "待处理操作")}
          locale={locale}
          value={data.pending.count}
          tone={data.pending.count > 0 ? "is-warning" : "is-success"}
          foot={locale === "zh-CN"
            ? `${data.remote.ahead ?? 0}/${data.remote.behind ?? 0} 领先/落后`
            : `${data.remote.ahead ?? 0}/${data.remote.behind ?? 0} ahead/behind`}
        />
      </div>

      <div className="overview-grid">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Priority issues", "优先问题")}</h2>
            <span className="minor-chip is-warning">{pick(locale, "actionable", "需处理")}</span>
          </div>
          <div className="panel-content detail-list">
            {drifted.length === 0 ? (
              <div className="empty-state">{pick(locale, "no drift detected", "没有检测到漂移")}</div>
            ) : (
              drifted.slice(0, 4).map((projection) => (
                <article className="detail-item" key={projection.instance_id}>
                  <div className="minor-kicker">{projection.target_id}</div>
                  <div className="primary-copy">{projection.skill_id}</div>
                  <div className="secondary-copy">
                    {pick(
                      locale,
                      `Projection ${projection.instance_id} differs from the expected source state.`,
                      `投影 ${projection.instance_id} 与预期源状态不一致。`,
                    )}
                  </div>
                  <div className="mono-copy">{formatTime(locale, projection.updated_at ?? data.lastUpdated)}</div>
                </article>
              ))
            )}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Warnings", "告警")}</h2>
            <span className={`minor-chip ${activeWarnings.length > 0 ? "is-warning" : "is-success"}`}>
              {activeWarnings.length}
            </span>
          </div>
          <div className="panel-content warning-list">
            {activeWarnings.length === 0 ? (
              <div className="empty-state">{pick(locale, "no active warnings", "没有活跃告警")}</div>
            ) : (
              activeWarnings.slice(0, 6).map((warning) => (
                <div className="risk-item is-warning" key={warning}>
                  <div className="secondary-copy">{warning}</div>
                </div>
              ))
            )}
          </div>
        </section>
      </div>

      <section className="panel">
        <div className="panel-head">
          <h2 className="panel-title">{pick(locale, "Recent queue activity", "最近队列活动")}</h2>
          <span className="minor-chip is-primary">{pick(locale, "local records", "本地记录")}</span>
        </div>
        <div className="panel-content detail-list">
          {diagnosticLines.length === 0 ? (
            <div className="empty-state">{pick(locale, "no queued activity matched search", "没有匹配搜索条件的队列活动")}</div>
          ) : (
            diagnosticLines.map((op) => (
              <article className="detail-item" key={op.request_id}>
                <div className="minor-kicker">{formatTime(locale, op.created_at)}</div>
                <div className="primary-copy">{op.command.toUpperCase().split(" ").join("_")}</div>
                <div className="secondary-copy">{summarizeDetails(op.details)}</div>
              </article>
            ))
          )}
        </div>
      </section>
    </div>
  );
}
