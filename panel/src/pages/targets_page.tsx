import { useState } from "react";
import {
  capabilityLabel,
  countLabel,
  ownershipLabel,
  pick,
} from "../i18n";
import type { Locale } from "../i18n";
import { Icon } from "../components/icon";
import { formatShortPath, projectionIsDrifted } from "../lib/panel_data";
import type { PanelData } from "../types";

function agentDisplayName(agent?: string) {
  if (agent === "claude") return "Claude";
  if (agent === "codex") return "Codex";
  return agent ?? "Unknown";
}

export function TargetsPage({ data, locale }: { data: PanelData; locale: Locale }) {
  const [selectedTargetId, setSelectedTargetId] = useState<string>(data.v3.targets[0]?.target_id ?? "");
  const active = data.v3.targets.find((target) => target.target_id === selectedTargetId) ?? data.v3.targets[0];

  const relatedBindings = active
    ? data.v3.bindings.filter((binding) => binding.default_target_id === active.target_id)
    : [];
  const relatedRules = active
    ? data.v3.rules.filter((rule) => rule.target_id === active.target_id)
    : [];
  const relatedProjections = active
    ? data.v3.projections.filter((projection) => projection.target_id === active.target_id)
    : [];

  return (
    <div className="page-body page-frame">
      <div className="page-head">
        <div>
          <h1 className="page-title">{pick(locale, "Targets Registry", "目标注册表")}</h1>
          <p className="page-subtitle">
            {pick(
              locale,
              "Registered filesystem targets, ownership boundaries, capabilities, and dependent projections.",
              "查看已注册的文件系统目标、所有权边界、能力与依赖投影。",
            )}
          </p>
        </div>
        <div className="page-actions">
          <span className="status-pill is-primary">
            <Icon name="folder_managed" />
            {countLabel(locale, data.v3.targets.length, "target", "targets", "个目标")}
          </span>
        </div>
      </div>

      <div className="bindings-grid">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Targets Table", "目标表")}</h2>
            <span className="minor-chip">
              {locale === "zh-CN"
                ? `${data.v3.targets.filter((target) => target.ownership === "managed").length} 托管`
                : `${data.v3.targets.filter((target) => target.ownership === "managed").length} managed`}
            </span>
          </div>
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{pick(locale, "Target", "目标")}</th>
                  <th>{pick(locale, "Path", "路径")}</th>
                  <th>{pick(locale, "Ownership", "所有权")}</th>
                  <th>{pick(locale, "Capabilities", "能力")}</th>
                  <th>{pick(locale, "Bindings", "绑定")}</th>
                  <th>{pick(locale, "Projections", "投影")}</th>
                </tr>
              </thead>
              <tbody>
                {data.v3.targets.map((target) => {
                  const bindings = data.v3.bindings.filter((binding) => binding.default_target_id === target.target_id);
                  const projections = data.v3.projections.filter((projection) => projection.target_id === target.target_id);
                  return (
                    <tr
                      className={target.target_id === active?.target_id ? "active" : undefined}
                      key={target.target_id}
                      onClick={() => setSelectedTargetId(target.target_id)}
                    >
                      <td>
                        <div className="primary-copy">{target.target_id}</div>
                        <div className="mono-copy">{agentDisplayName(target.agent)}</div>
                      </td>
                      <td>
                        <div className="mono-copy">{target.path}</div>
                      </td>
                      <td>
                        <span
                          className={`minor-chip ${
                            target.ownership === "managed"
                              ? "is-success"
                              : target.ownership === "observed"
                                ? "is-warning"
                                : "is-danger"
                          }`}
                        >
                          {ownershipLabel(locale, target.ownership)}
                        </span>
                      </td>
                      <td>
                        <div className="target-capability-list">
                          {target.capabilities.symlink ? <span className="minor-chip">{capabilityLabel(locale, "symlink")}</span> : null}
                          {target.capabilities.copy ? <span className="minor-chip">{capabilityLabel(locale, "copy")}</span> : null}
                          {target.capabilities.watch ? <span className="minor-chip">{capabilityLabel(locale, "watch")}</span> : null}
                        </div>
                      </td>
                      <td><div className="primary-copy">{bindings.length}</div></td>
                      <td><div className="primary-copy">{projections.length}</div></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>

        <aside className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Target Inspector", "目标检查器")}</h2>
            {active ? <span className="metric-chip is-primary">{active.target_id}</span> : null}
          </div>
          <div className="detail-panel">
            {!active ? (
              <div className="empty-state">{pick(locale, "no targets available", "没有可用目标")}</div>
            ) : (
              <>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Filesystem Path", "文件系统路径")}</div>
                  <div className="primary-copy">{formatShortPath(active.path, locale)}</div>
                  <div className="mono-copy">{active.path}</div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Boundary", "边界")}</div>
                  <div className="primary-copy">{ownershipLabel(locale, active.ownership)}</div>
                  <div className="secondary-copy">
                    {active.ownership === "managed"
                      ? pick(locale, "Loom can safely own projection lifecycle here.", "Loom 可以安全接管这里的投影生命周期。")
                      : active.ownership === "observed"
                        ? pick(locale, "Directory is watched, but mutations should stay conservative.", "目录会被观察，但变更应保持保守。")
                        : pick(locale, "External path: show state, surface risk, avoid destructive assumptions.", "外部路径：展示状态、暴露风险、避免破坏性假设。")}
                  </div>
                  <div className="target-capability-list target-capability-list-spaced">
                    <span className="minor-chip">{agentDisplayName(active.agent)}</span>
                    {active.capabilities.symlink ? <span className="minor-chip">{capabilityLabel(locale, "symlink")}</span> : null}
                    {active.capabilities.copy ? <span className="minor-chip">{capabilityLabel(locale, "copy")}</span> : null}
                    {active.capabilities.watch ? <span className="minor-chip">{capabilityLabel(locale, "watch")}</span> : null}
                  </div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Dependency Counts", "依赖计数")}</div>
                  <div className="mini-stats target-mini-stats">
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Bindings", "绑定")}</div>
                      <div className="mini-value">{relatedBindings.length}</div>
                    </div>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Rules", "规则")}</div>
                      <div className="mini-value">{relatedRules.length}</div>
                    </div>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Proj.", "投影")}</div>
                      <div className="mini-value">{relatedProjections.length}</div>
                    </div>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Drift", "漂移")}</div>
                      <div className="mini-value">{relatedProjections.filter(projectionIsDrifted).length}</div>
                    </div>
                  </div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Dependent Bindings", "依赖绑定")}</div>
                  <div className="detail-list target-detail-list">
                    {relatedBindings.length === 0 ? (
                      <div className="empty-state">{pick(locale, "no dependent bindings", "没有依赖绑定")}</div>
                    ) : (
                      relatedBindings.map((binding) => (
                        <div className="table-row-summary" key={binding.binding_id}>
                          <div className="primary-copy">{binding.binding_id}</div>
                          <div className="secondary-copy">
                            {binding.workspace_matcher.value} · {binding.policy_profile}
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </article>
              </>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
