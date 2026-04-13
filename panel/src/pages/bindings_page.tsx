import { useState } from "react";
import {
  activeLabel,
  matcherKindLabel,
  methodLabel,
  pick,
} from "../i18n";
import type { Locale } from "../i18n";
import { Icon } from "../components/icon";
import type { PanelData, SkillView, V3Binding } from "../types";

function bindingRowIcon(binding: V3Binding) {
  if (!binding.active) return "person";
  if (binding.policy_profile === "strict") return "shield";
  return "person";
}

function bindingAgentIcon(binding: V3Binding) {
  if (binding.agent === "claude") return "bolt";
  if (binding.agent === "codex") return "data_object";
  return "memory";
}

function agentDisplayName(agent?: string) {
  if (agent === "claude") return "Claude";
  if (agent === "codex") return "Codex";
  return agent ?? "Unknown";
}

export function BindingsPage({
  data,
  locale,
  skillViews,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
}) {
  const [selectedBinding, setSelectedBinding] = useState<string>(data.v3.bindings[0]?.binding_id ?? "");
  const active = data.v3.bindings.find((binding) => binding.binding_id === selectedBinding) ?? data.v3.bindings[0];

  const relatedRules = active ? data.v3.rules.filter((rule) => rule.binding_id === active.binding_id) : [];
  const relatedProjections = active
    ? data.v3.projections.filter((projection) => projection.binding_id === active.binding_id)
    : [];
  const relatedSkills = skillViews.filter((skill) =>
    skill.bindings.some((binding) => binding.binding_id === active?.binding_id),
  );
  const defaultTarget = data.v3.targets.find((target) => target.target_id === active?.default_target_id);

  return (
    <div className="stitch-screen-column">
      <div className="stitch-content-header">
        <div>
          <div className="stitch-content-title-row">
            <h1 className="stitch-content-title">{pick(locale, "Workspace Bindings", "工作区绑定")}</h1>
            <span className="stitch-content-badge">GLOBAL</span>
          </div>
          <p className="stitch-content-subtitle">
            {pick(locale, "Binding policy, workspace matcher, target handoff, and dependent skills.", "查看绑定策略、工作区匹配器、目标交接与依赖技能。")}
          </p>
        </div>
      </div>
      <div className="stitch-bindings-layout">
        <section className="panel stitch-panel-reset">
          <div className="table-wrap">
            <table className="stitch-dense-table stitch-bindings-table">
              <thead>
                <tr>
                  <th>{pick(locale, "Binding Name", "绑定名")}</th>
                  <th>{pick(locale, "Matcher", "匹配器")}</th>
                  <th>{pick(locale, "Agent Kind", "代理类型")}</th>
                  <th>{pick(locale, "Default Path", "默认路径")}</th>
                  <th>{pick(locale, "Policy", "策略")}</th>
                  <th>{pick(locale, "Status", "状态")}</th>
                </tr>
              </thead>
              <tbody>
                {data.v3.bindings.map((binding) => {
                  const rowTarget = data.v3.targets.find((target) => target.target_id === binding.default_target_id);
                  const selected = binding.binding_id === active?.binding_id;
                  return (
                    <tr
                      className={`${selected ? "active" : ""} ${binding.active ? "" : "is-inactive"}`.trim() || undefined}
                      key={binding.binding_id}
                      onClick={() => setSelectedBinding(binding.binding_id)}
                    >
                      <td>
                        <div className="stitch-binding-row-head">
                          <div
                            className={`stitch-binding-row-icon ${
                              binding.active ? (binding.policy_profile === "strict" ? "is-primary" : "is-accent") : "is-muted"
                            }`}
                          >
                            <Icon name={bindingRowIcon(binding)} />
                          </div>
                          <div>
                            <div className="stitch-binding-cell-title">{binding.binding_id}</div>
                            <div className="mono-copy">{`ID: ${binding.profile_id}`}</div>
                          </div>
                        </div>
                      </td>
                      <td>
                        <div className="mono-copy">
                          <span className="muted">{`${matcherKindLabel(locale, binding.workspace_matcher.kind)}:`} </span>
                          {binding.workspace_matcher.value}
                        </div>
                      </td>
                      <td>
                        <div className="stitch-agent-pill">
                          <span className={`stitch-agent-icon ${binding.agent === "claude" ? "is-accent" : "is-primary"}`}>
                            <Icon name={bindingAgentIcon(binding)} />
                          </span>
                          <span>{agentDisplayName(binding.agent)}</span>
                        </div>
                      </td>
                      <td><div className="mono-copy">{rowTarget?.path ?? binding.default_target_id}</div></td>
                      <td>
                        <span className={`stitch-status-tag ${binding.policy_profile === "strict" ? "is-danger" : "is-success"}`}>
                          {binding.policy_profile.toUpperCase()}
                        </span>
                      </td>
                      <td>
                        <span className={`stitch-activity ${binding.active ? "is-success" : "is-muted"}`}>
                          <span className="stitch-activity-dot" />
                          {activeLabel(locale, binding.active).toUpperCase()}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>

        <aside className="panel stitch-drawer">
          <div className="detail-panel">
            {!active ? (
              <div className="empty-state">{pick(locale, "no bindings available", "没有可用绑定")}</div>
            ) : (
              <>
                <div className="stitch-drawer-card">
                  <div className="stitch-drawer-card-head">
                    <h3 className="stitch-block-title">{pick(locale, "Binding Summary", "绑定摘要")}</h3>
                  </div>
                  <div className="stitch-binding-hero">
                    <div className="stitch-binding-hero-icon">
                      <Icon name="link" />
                    </div>
                    <div>
                      <h4>{active.binding_id}</h4>
                      <p>{pick(locale, active.active ? "Status: active" : "Status: inactive", active.active ? "状态：启用" : "状态：停用")}</p>
                    </div>
                  </div>
                  <div className="stitch-kpi-grid">
                    <div className="stitch-kpi-card">
                      <p>{pick(locale, "Rules", "规则")}</p>
                      <strong>{relatedRules.length}</strong>
                    </div>
                    <div className="stitch-kpi-card">
                      <p>{pick(locale, "Projections", "投影")}</p>
                      <strong>{relatedProjections.length}</strong>
                    </div>
                  </div>
                </div>

                <div className="stitch-drawer-card">
                  <h3 className="stitch-block-title">{pick(locale, "Dependent Skills", "依赖技能")}</h3>
                  <div className="stitch-thread-list">
                    {relatedSkills.length === 0 ? (
                      <div className="empty-state">{pick(locale, "no related skills", "没有相关技能")}</div>
                    ) : (
                      relatedSkills.map((skill) => (
                        <div className="stitch-thread-row" key={skill.name}>
                          <div>
                            <div className="stitch-thread-title">{skill.name}</div>
                            <div className="stitch-thread-subtitle">{skill.methods.map((method) => methodLabel(locale, method)).join(" · ") || pick(locale, "unprojected", "未投影")}</div>
                          </div>
                          <span className="mono-copy">{`${skill.projections.length} proj`}</span>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                <div className="stitch-drawer-card">
                  <h3 className="stitch-block-title">{pick(locale, "Binding Details", "绑定详情")}</h3>
                  <div className="detail-list">
                    <article className="detail-item">
                      <div className="minor-kicker">{pick(locale, "Workspace matcher", "工作区匹配器")}</div>
                      <div className="primary-copy">{active.workspace_matcher.value}</div>
                      <div className="secondary-copy">{matcherKindLabel(locale, active.workspace_matcher.kind)}</div>
                    </article>
                    <article className="detail-item">
                      <div className="minor-kicker">{pick(locale, "Default target", "默认目标")}</div>
                      <div className="primary-copy">{active.default_target_id}</div>
                      <div className="secondary-copy">{defaultTarget?.path ?? pick(locale, "target path unavailable", "目标路径不可用")}</div>
                    </article>
                  </div>
                </div>
              </>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
