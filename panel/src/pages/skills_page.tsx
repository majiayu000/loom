import { useDeferredValue, useEffect, useMemo, useState } from "react";
import {
  formatTime,
  healthLabel,
  missingLabel,
  pick,
  syncStateLabel,
} from "../i18n";
import type { Locale } from "../i18n";
import { Icon } from "../components/icon";
import {
  formatShortPath,
  projectionIsDrifted,
  projectionToneClass,
} from "../lib/panel_data";
import { filterSkillViews } from "../lib/panel_selectors";
import type { PanelData, SkillView } from "../types";

export function SkillsPage({
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
  const deferredQuery = useDeferredValue(query);
  const filtered = useMemo(() => filterSkillViews(skillViews, deferredQuery), [deferredQuery, skillViews]);

  const [selectedSkill, setSelectedSkill] = useState<string>(filtered[0]?.name ?? "");

  useEffect(() => {
    if (!filtered.some((skill) => skill.name === selectedSkill)) {
      setSelectedSkill(filtered[0]?.name ?? "");
    }
  }, [filtered, selectedSkill]);

  const active = filtered.find((skill) => skill.name === selectedSkill) ?? filtered[0];
  const activeProjection = active?.projections[0] ?? null;
  const sourcePath = active
    ? `${data.info.root ?? "<root>"}/skills/${active.name}`
    : `${data.info.root ?? "<root>"}/skills/<skill>`;
  const dependencyText = active
    ? (active.bindings.map((binding) => `${binding.binding_id}@${binding.profile_id}`).slice(0, 2).join(", ")
      || active.targets.map((target) => target.target_id).slice(0, 2).join(", ")
      || missingLabel(locale))
    : missingLabel(locale);
  const lastModified = formatTime(locale, activeProjection?.updated_at ?? data.lastUpdated);

  return (
    <div className="stitch-workspace">
      <section className="stitch-rail">
        <div className="stitch-rail-head">
          <div className="stitch-rail-head-row">
            <h2 className="stitch-rail-title">{pick(locale, "Skill Sources", "技能源")}</h2>
            <span className="minor-chip is-primary">
              {locale === "zh-CN" ? `${filtered.length} 项匹配` : `${filtered.length} matched`}
            </span>
          </div>
        </div>
        <div className="skill-list">
          {filtered.length === 0 ? (
            <div className="empty-state">{pick(locale, "no skill matched query", "没有匹配的技能")}</div>
          ) : (
            filtered.map((skill) => (
              <button
                className={`skill-row ${skill.name === active?.name ? "active" : ""}`}
                key={skill.name}
                onClick={() => setSelectedSkill(skill.name)}
                type="button"
              >
                <div className="stitch-skill-row-top">
                  <span className="stitch-skill-name">{`skills/${skill.name}`}</span>
                  <span className={`material-symbols-outlined ${skill.driftedCount > 0 ? "is-danger" : "is-success"}`}>
                    {skill.driftedCount > 0 ? "warning" : "check_circle"}
                  </span>
                </div>
                <div className="stitch-skill-hash">
                  rev: {(skill.projections[0]?.last_applied_rev ?? "pending").slice(0, 10)}
                </div>
                <div className="stitch-skill-tags">
                  {(skill.methods.length > 0 ? skill.methods : [pick(locale, "unprojected", "未投影")]).slice(0, 2).map((method) => (
                    <span className="stitch-inline-tag" key={method}>
                      {method === pick(locale, "unprojected", "未投影") ? method : method.toUpperCase()}
                    </span>
                  ))}
                </div>
              </button>
            ))
          )}
        </div>
      </section>

      <section className="stitch-detail-shell">
        {!active ? (
          <div className="empty-state">{pick(locale, "select a skill to inspect", "选择一个技能查看详情")}</div>
        ) : (
          <>
            <div className="stitch-detail-header">
              <div>
                <div className="stitch-detail-title-row">
                  <Icon name="bolt" />
                  <h2>{`skills/${active.name}`}</h2>
                </div>
                <div className="stitch-detail-meta-row">
                  <div className="stitch-inline-meta">
                    <Icon name="folder" />
                    <span>{sourcePath}</span>
                  </div>
                  <div className="stitch-inline-meta is-accent">
                    <Icon name="account_tree" />
                    <span>{syncStateLabel(locale, data.remote.sync_state)}</span>
                  </div>
                </div>
              </div>
              <div className="page-actions">
                <span className={`status-pill ${active.driftedCount > 0 ? "is-danger" : "is-success"}`}>
                  <Icon name={active.driftedCount > 0 ? "warning" : "check_circle"} />
                  {active.driftedCount > 0
                    ? pick(locale, `${active.driftedCount} drifted`, `${active.driftedCount} 个漂移`)
                    : pick(locale, "healthy", "健康")}
                </span>
              </div>
            </div>

            <div className="stitch-grid-12">
              <div className="stitch-col-main">
                <div className="stitch-section-block">
                  <h3 className="stitch-block-title">
                    <span className="stitch-block-dot" />
                    {pick(locale, "Projection Instances", "投影实例")}
                  </h3>
                  <div className="stitch-table-shell">
                    <table className="stitch-dense-table">
                      <thead>
                        <tr>
                          <th>TARGET_ID</th>
                          <th>BINDING</th>
                          <th>PATH</th>
                          <th className="align-right">STATUS</th>
                        </tr>
                      </thead>
                      <tbody>
                        {active.projections.length === 0 ? (
                          <tr>
                            <td colSpan={4}>
                              <div className="empty-state">{pick(locale, "no active projections", "没有活跃投影")}</div>
                            </td>
                          </tr>
                        ) : (
                          active.projections.map((projection) => (
                            <tr key={projection.instance_id}>
                              <td>{projection.target_id}</td>
                              <td>{projection.binding_id}</td>
                              <td>{formatShortPath(projection.materialized_path, locale)}</td>
                              <td className="align-right">
                                <span className={`stitch-status-tag ${projectionToneClass(projection)}`}>
                                  {projectionIsDrifted(projection)
                                    ? healthLabel(locale, projection.health)
                                    : pick(locale, "HEALTHY", "健康")}
                                </span>
                              </td>
                            </tr>
                          ))
                        )}
                      </tbody>
                    </table>
                  </div>
                </div>
              </div>

              <div className="stitch-col-side">
                <div className="stitch-panel-block">
                  <h3 className="stitch-block-title">{pick(locale, "Metadata", "元数据")}</h3>
                  <div className="stitch-meta-list">
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Dependencies", "依赖")}</span>
                      <span>{dependencyText}</span>
                    </div>
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Last Modified", "最新修订")}</span>
                      <span>{lastModified}</span>
                    </div>
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Bindings", "绑定")}</span>
                      <span>{active.bindings.length}</span>
                    </div>
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Targets", "目标")}</span>
                      <span>{active.targets.length}</span>
                    </div>
                  </div>
                </div>

                <div className="stitch-panel-block">
                  <h3 className="stitch-block-title">{pick(locale, "CLI command", "CLI 命令")}</h3>
                  <div className="detail-list">
                    <article className="detail-item">
                      <div className="minor-kicker">{pick(locale, "Inspect with CLI", "用 CLI 查看")}</div>
                      <div className="mono-copy">{`loom --json --root "${data.info.root ?? "<root>"}" skill show ${active.name}`}</div>
                    </article>
                  </div>
                </div>
              </div>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
