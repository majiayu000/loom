import type { Skill } from "../lib/types";
import { SkillConvergencePanel } from "./panel/SkillConvergencePanel";

export function SkillMDetail({ skill, convergenceSupported, onApplied }: { skill: Skill | null; convergenceSupported: boolean; onApplied: () => void }) {
  if (!skill) {
    return <aside className="skill-detail panel"><div className="panel-empty">选择一个 skill 查看来源、目标和投影统计。</div></aside>;
  }
  const targetCount = skill.targets.length + (skill.observedTargetIds?.length ?? 0);
  return (
    <aside className="skill-detail panel" aria-label={`${skill.name} detail`}>
      <div className="det-head">
        <div className="det-title"><span className="sm-glyph">{skill.name.slice(0, 2).toUpperCase()}</span><div><h2>{skill.name}</h2><p>{skill.description || "No backend description."}</p></div></div>
        <span className="sec-badge good">{skill.sourceStatus}</span>
      </div>
      <div className="det-metrics">
        <div><span>Bindings</span><b>{skill.bindingCount}</b><em>routing rules</em></div>
        <div><span>Projections</span><b>{skill.projectionCount}</b><em>materialized edges</em></div>
        <div><span>Latest rev</span><b>{skill.latestRev}</b><em>backend reported</em></div>
        <div><span>Targets</span><b>{targetCount}</b><em>observed + projected</em></div>
      </div>
      <SkillConvergencePanel key={skill.name} skillName={skill.name} supported={convergenceSupported} onApplied={onApplied} />
    </aside>
  );
}
