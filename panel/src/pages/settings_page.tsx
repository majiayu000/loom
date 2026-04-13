import {
  formatTime,
  missingLabel,
  pick,
  syncStateLabel,
} from "../i18n";
import type { Locale } from "../i18n";
import { Icon } from "../components/icon";
import type { PanelData } from "../types";

export function SettingsPage({ data, locale }: { data: PanelData; locale: Locale }) {
  const paths = [
    { label: pick(locale, "Workspace Root", "工作区根目录"), value: data.info.root },
    { label: pick(locale, "State Directory", "状态目录"), value: data.info.state_dir },
    { label: pick(locale, "V3 Targets File", "V3 目标文件"), value: data.info.v3_targets_file },
    { label: pick(locale, "Claude Directory", "Claude 目录"), value: data.info.claude_dir },
    { label: pick(locale, "Codex Directory", "Codex 目录"), value: data.info.codex_dir },
    { label: pick(locale, "Remote URL", "远端 URL"), value: data.info.remote_url },
  ];
  const warnings = [...data.remoteWarnings, ...(data.pending.warnings ?? [])];

  return (
    <div className="page-body page-frame">
      <div className="page-head">
        <div>
          <h1 className="page-title">{pick(locale, "Environment", "环境")}</h1>
          <p className="page-subtitle">
            {pick(
              locale,
              "Local control-plane environment, state model posture, and repository-level operating context.",
              "查看本地控制平面环境、状态模型姿态与仓库级运行上下文。",
            )}
          </p>
        </div>
        <div className="page-actions">
          <span className={`status-pill ${data.live ? "is-success" : "is-warning"}`}>
            <Icon name={data.live ? "hub" : "science"} />
            {data.live ? pick(locale, "api connected", "API 已连接") : pick(locale, "api unavailable", "API 不可用")}
          </span>
          <span className="status-pill is-primary">
            <Icon name="schedule" />
            {formatTime(locale, data.lastUpdated)}
          </span>
        </div>
      </div>

      <div className="overview-grid">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Environment Paths", "环境路径")}</h2>
            <span className="minor-chip">{pick(locale, "filesystem truth", "文件系统真实值")}</span>
          </div>
          <div className="panel-content detail-list">
            {paths.map((item) => (
              <article className="detail-item" key={item.label}>
                <div className="minor-kicker">{item.label}</div>
                <div className="mono-copy settings-path-value">{item.value ?? missingLabel(locale)}</div>
              </article>
            ))}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "State Model", "状态模型")}</h2>
            <span className={`minor-chip ${data.v3.available ? "is-success" : "is-warning"}`}>
              {data.v3.available ? pick(locale, "v3 available", "v3 可用") : pick(locale, "v3 unavailable", "v3 不可用")}
            </span>
          </div>
          <div className="detail-panel">
            <article className="detail-item">
              <div className="minor-kicker">{pick(locale, "Counts", "计数")}</div>
              <div className="mini-stats target-mini-stats">
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Skills", "技能")}</div>
                  <div className="mini-value">{data.v3.counts.skills}</div>
                </div>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Bindings", "绑定")}</div>
                  <div className="mini-value">{data.v3.counts.bindings}</div>
                </div>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Targets", "目标")}</div>
                  <div className="mini-value">{data.v3.counts.targets}</div>
                </div>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Proj.", "投影")}</div>
                  <div className="mini-value">{data.v3.counts.projections}</div>
                </div>
              </div>
            </article>
            <article className="detail-item">
              <div className="minor-kicker">{pick(locale, "Checkpoint", "检查点")}</div>
              <div className="detail-list target-detail-list">
                <div className="mono-copy">
                  {pick(locale, "scanned", "已扫描")}: {data.v3.checkpoint?.last_scanned_op_id ?? missingLabel(locale)}
                </div>
                <div className="mono-copy">
                  {pick(locale, "acked", "已确认")}: {data.v3.checkpoint?.last_acked_op_id ?? missingLabel(locale)}
                </div>
                <div className="mono-copy">
                  {pick(locale, "updated", "更新时间")}: {formatTime(locale, data.v3.checkpoint?.updated_at)}
                </div>
              </div>
            </article>
            <article className="detail-item">
              <div className="minor-kicker">{pick(locale, "Remote Posture", "远端姿态")}</div>
              <div className="secondary-copy">
                {syncStateLabel(locale, data.remote.sync_state)} ·{" "}
                {locale === "zh-CN"
                  ? `${data.remote.ahead ?? 0} 领先 / ${data.remote.behind ?? 0} 落后 · ${data.pending.count} 待处理操作`
                  : `${data.remote.ahead ?? 0} ahead / ${data.remote.behind ?? 0} behind · ${data.pending.count} pending ops`}
              </div>
            </article>
          </div>
        </section>
      </div>

      <div className="split-two">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Warnings", "告警")}</h2>
          </div>
          <div className="panel-content warning-list">
            {warnings.length === 0 ? (
              <div className="empty-state">{pick(locale, "no active warnings", "没有活跃告警")}</div>
            ) : (
              warnings.map((warning) => (
                <div className="risk-item is-warning" key={warning}>
                  <div className="secondary-copy">{warning}</div>
                </div>
              ))
            )}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "CLI Surface", "CLI 面板")}</h2>
          </div>
          <div className="panel-content detail-list">
            <div className="mono-copy">{`loom --json --root "${data.info.root ?? "<root>"}" workspace status`}</div>
            <div className="mono-copy">{`loom --json --root "${data.info.root ?? "<root>"}" workspace doctor`}</div>
            <div className="mono-copy">{`loom --json --root "${data.info.root ?? "<root>"}" sync status`}</div>
            <div className="mono-copy">{`loom --json --root "${data.info.root ?? "<root>"}" target list`}</div>
          </div>
        </section>
      </div>
    </div>
  );
}
