import { Fragment, useMemo, useState } from "react";
import { pick } from "../i18n";
import type { Locale } from "../i18n";
import { Icon } from "../components/icon";
import {
  projectionIsDrifted,
  projectionToneClass,
} from "../lib/panel_data";
import {
  buildProjectionMatrix,
  filterProjectionViews,
  type ProjectionHealthFilter,
  type ProjectionMethodFilter,
} from "../lib/panel_selectors";
import type { PanelData, SkillView, V3Projection } from "../types";

const METHOD_OPTIONS: ProjectionMethodFilter[] = ["all", "symlink", "copy"];
const HEALTH_OPTIONS: ProjectionHealthFilter[] = ["all", "healthy", "drifted", "warning"];

function projectionStatusText(projection: V3Projection | null, locale: Locale) {
  if (!projection) return pick(locale, "No projection", "没有投影");
  if (projectionIsDrifted(projection)) return pick(locale, "Drift detected", "检测到漂移");
  if (projection.health === "warning") return pick(locale, "Warning", "警告");
  return pick(locale, "Healthy", "健康");
}

export function ProjectionsPage({
  data,
  locale,
  skillViews,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
}) {
  const [selectedProjection, setSelectedProjection] = useState<V3Projection | null>(data.v3.projections[0] ?? null);
  const [method, setMethod] = useState<ProjectionMethodFilter>("all");
  const [health, setHealth] = useState<ProjectionHealthFilter>("all");
  const [scope, setScope] = useState("all");

  const filtered = useMemo(
    () => filterProjectionViews(skillViews, data.v3.projections, data.v3.targets, { method, health, scope }),
    [data.v3.projections, data.v3.targets, health, method, scope, skillViews],
  );
  const matrix = useMemo(() => buildProjectionMatrix(filtered.projections), [filtered.projections]);
  const matrixTemplateColumns = `180px repeat(${Math.max(filtered.targets.length, 1)}, 140px)`;
  const drifted = filtered.projections.filter(projectionIsDrifted);

  return (
    <div className="stitch-screen-column">
      <div className="stitch-breadcrumb-bar">
        <div className="stitch-breadcrumbs">
          <span>{pick(locale, "Projections", "投影")}</span>
          <Icon name="chevron_right" />
          <span className="is-active">{pick(locale, "Matrix view", "矩阵视图")}</span>
        </div>
        <div className="page-actions">
          <span className="stitch-inline-meta is-accent">
            <Icon name="sync" />
            <span>{pick(locale, "Read-only projection state", "只读投影状态")}</span>
          </span>
        </div>
      </div>

      <div className="projection-grid stitch-projection-layout">
        <section className="panel stitch-panel-reset">
          <div className="stitch-matrix-shell">
            {filtered.targets.length === 0 || filtered.skills.length === 0 ? (
              <div className="empty-state">{pick(locale, "no projections matched current filters", "没有匹配当前筛选条件的投影")}</div>
            ) : (
              <div className="stitch-matrix-grid" style={{ gridTemplateColumns: matrixTemplateColumns }}>
                <div className="stitch-matrix-corner">
                  <span>{pick(locale, "Source \\ Target", "源 \\ 目标")}</span>
                </div>
                {filtered.targets.map((target) => (
                  <div className="stitch-matrix-head" key={target.target_id}>
                    <div className="stitch-matrix-head-card">
                      <div className="stitch-binding-cell-title">{target.target_id}</div>
                      <div className="mono-copy">{target.path}</div>
                    </div>
                  </div>
                ))}
                {filtered.skills.map((skill) => (
                  <Fragment key={skill.name}>
                    <div className="stitch-matrix-source">
                      <div className="stitch-matrix-source-head">
                        <Icon name="bolt" />
                        <span className="stitch-matrix-row-title">{skill.name}</span>
                      </div>
                      <span className="mono-copy">{`${skill.projections.length} projections`}</span>
                    </div>
                    {filtered.targets.map((target) => {
                      const cell = matrix.get(`${skill.name}::${target.target_id}`) ?? [];
                      const projection = cell[0] ?? null;
                      const toneClass = projectionIsDrifted(projection)
                        ? "is-drifted"
                        : projection?.health === "warning"
                          ? "is-warning"
                          : projection
                            ? "is-success"
                            : "is-empty";
                      return (
                        <button
                          className={`stitch-matrix-cell ${toneClass}`}
                          key={`${skill.name}-${target.target_id}`}
                          onClick={() => setSelectedProjection(projection)}
                          type="button"
                        >
                          {!projection ? (
                            <div className="stitch-matrix-empty">
                              <Icon name="remove" />
                            </div>
                          ) : (
                            <div className="stitch-matrix-card">
                              <div className="stitch-projection-card-top">
                                <span className={`material-symbols-outlined ${projectionToneClass(projection)}`}>
                                  {projectionIsDrifted(projection)
                                    ? "warning"
                                    : projection.health === "warning"
                                      ? "sync_problem"
                                      : "check_circle"}
                                </span>
                                <span className="stitch-inline-tag">{projection.method.toUpperCase()}</span>
                              </div>
                              <span className={`stitch-projection-foot ${projectionToneClass(projection)}`}>
                                {projectionStatusText(projection, locale)}
                              </span>
                            </div>
                          )}
                        </button>
                      );
                    })}
                  </Fragment>
                ))}
              </div>
            )}
          </div>
        </section>

        <aside className="panel stitch-filter-sidebar">
          <div className="detail-panel">
            <div className="stitch-drawer-card">
              <h3 className="stitch-block-title">{pick(locale, "Projection Filter", "投影筛选")}</h3>
              <div className="stitch-filter-group">
                <label>{pick(locale, "Method", "方法")}</label>
                <div className="stitch-rail-tabs">
                  {METHOD_OPTIONS.map((option) => (
                    <button
                      className={`stitch-tab ${method === option ? "active" : ""}`}
                      key={option}
                      onClick={() => setMethod(option)}
                      type="button"
                    >
                      {option === "all" ? pick(locale, "All", "全部") : option === "symlink" ? pick(locale, "Symlink", "符号链接") : pick(locale, "Copy", "复制")}
                    </button>
                  ))}
                </div>
              </div>
              <div className="stitch-filter-group">
                <label>{pick(locale, "Health", "健康")}</label>
                <div className="stitch-checklist">
                  {HEALTH_OPTIONS.map((option) => (
                    <label key={option}>
                      <input
                        checked={health === option}
                        onChange={() => setHealth(option)}
                        type="radio"
                        name="projection-health"
                      />
                      <span>
                        {option === "all"
                          ? pick(locale, "All", "全部")
                          : option === "healthy"
                            ? pick(locale, "Healthy", "健康")
                            : option === "drifted"
                              ? pick(locale, "Drifted", "漂移")
                              : pick(locale, "Warning", "警告")}
                      </span>
                    </label>
                  ))}
                </div>
              </div>
              <div className="stitch-filter-group">
                <label>{pick(locale, "Target scope", "目标范围")}</label>
                <select className="stitch-select" onChange={(event) => setScope(event.target.value)} value={scope}>
                  <option value="all">{pick(locale, "All targets", "全部目标")}</option>
                  {data.v3.targets.map((target) => (
                    <option key={target.target_id} value={target.target_id}>{target.target_id}</option>
                  ))}
                </select>
              </div>
            </div>

            <div className="stitch-drawer-card">
              <h3 className="stitch-block-title">{pick(locale, "Topology Stats", "拓扑统计")}</h3>
              <div className="stitch-kpi-grid">
                <div className="stitch-kpi-card stitch-kpi-card-primary">
                  <p>{pick(locale, "Visible Targets", "可见目标")}</p>
                  <strong>{filtered.targets.length}</strong>
                </div>
                <div className="stitch-kpi-card stitch-kpi-card-success">
                  <p>{pick(locale, "Filtered Projections", "筛选后投影")}</p>
                  <strong>{filtered.projections.length}</strong>
                </div>
              </div>
            </div>

            <div className="stitch-drawer-card stitch-alert-card">
              <div className="stitch-alert-head">
                <Icon name="warning" />
                <span>{pick(locale, "Selected Projection", "当前投影")}</span>
              </div>
              <p className="secondary-copy">
                {selectedProjection
                  ? `${selectedProjection.skill_id} · ${selectedProjection.target_id} · ${projectionStatusText(selectedProjection, locale)}`
                  : pick(locale, "Select a projection cell to inspect details.", "点击一个投影单元查看详情。")}
              </p>
            </div>

            {drifted.length > 0 ? (
              <div className="stitch-drawer-card stitch-alert-card">
                <div className="stitch-alert-head">
                  <Icon name="report" />
                  <span>{pick(locale, "Drift Summary", "漂移摘要")}</span>
                </div>
                <p className="secondary-copy">
                  {pick(
                    locale,
                    `${drifted.length} projection(s) are currently drifted under the active filters.`,
                    `当前筛选条件下有 ${drifted.length} 个投影处于漂移状态。`,
                  )}
                </p>
              </div>
            ) : null}
          </div>
        </aside>
      </div>
    </div>
  );
}
